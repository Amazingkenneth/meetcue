//! End-to-end: the SDN interview replayed through the real async backend with
//! mock providers, on tokio's paused clock (runs instantly, deterministic).

use std::sync::Arc;
use std::time::Duration;

use meetcue_core::{MeetCueEvent, PointStatus, QuestionStatus, TimedEvent};
use meetcue_pipeline::engine::{EngineConfig, ModelRef};
use meetcue_pipeline::providers::mock::{MockAsr, MockLlm};
use meetcue_pipeline::{HintBackend, LocalHintBackend, SessionConfig};

pub async fn run_sdn() -> (
    Vec<MeetCueEvent>,
    Vec<TimedEvent>,
    meetcue_pipeline::backend::SessionSummary,
) {
    let backend = LocalHintBackend::new();
    let mut rx = backend.events();
    let llm = Arc::new(MockLlm::sdn());
    let model = ModelRef {
        provider: "anthropic".into(),
        model: "claude-haiku-5-5".into(),
    };
    let engine = EngineConfig {
        detect: model.clone(),
        hints: model,
        role: "网络架构师".into(),
        ..Default::default()
    };
    let mut cfg = SessionConfig::new(engine, Some(Arc::new(MockAsr::sdn())), llm.clone(), llm);
    cfg.asr_model = ModelRef {
        provider: "soniox".into(),
        model: "stt-rt-v5".into(),
    };
    backend.start(cfg).await.unwrap();
    tokio::time::sleep(Duration::from_secs(70)).await;
    let summary = backend.stop().await.unwrap();
    let history = backend.history();
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    (events, history, summary)
}

#[tokio::test(start_paused = true)]
async fn sdn_interview_end_to_end() {
    let (events, history, summary) = run_sdn().await;

    // Three questions, in order, with the expected titles.
    let questions: Vec<(u32, String, Vec<String>)> = events
        .iter()
        .filter_map(|e| match e {
            MeetCueEvent::QuestionNew {
                id,
                short,
                sub_questions,
                version: 1,
                ..
            } => Some((*id, short.clone(), sub_questions.clone())),
            _ => None,
        })
        .collect();
    let shorts: Vec<&str> = questions.iter().map(|q| q.1.as_str()).collect();
    assert_eq!(
        shorts,
        ["网络高可用架构", "SDN 控制器部署", "控制器故障还能转发吗"]
    );
    assert_eq!(questions[1].2.len(), 2, "compound question split into two");

    // Queue: Q1 answered by coverage + silence, Q3 interrupts Q2 and waits.
    let statuses: Vec<(u32, QuestionStatus)> = events
        .iter()
        .filter_map(|e| match e {
            MeetCueEvent::QuestionStatus { id, status } => Some((*id, *status)),
            _ => None,
        })
        .collect();
    use QuestionStatus::*;
    assert_eq!(
        statuses,
        [
            (1, Active),
            (1, Answered),
            (2, Active),
            (3, Pending),
            (2, Answered),
            (3, Active),
            (3, Answered),
        ]
    );

    // Every question's points end up all said.
    for id in 1..=3 {
        let last = events.iter().rev().find_map(|e| match e {
            MeetCueEvent::Coverage { id: q, statuses } if *q == id => Some(statuses.clone()),
            _ => None,
        });
        let last = last.unwrap_or_else(|| panic!("no coverage for question {id}"));
        assert!(
            last.iter().all(|s| *s == PointStatus::Done),
            "question {id}: {last:?}"
        );
    }

    // Points never arrive before their question.
    let mut seen = std::collections::HashSet::new();
    for e in &events {
        match e {
            MeetCueEvent::QuestionNew { id, .. } => {
                seen.insert(*id);
            }
            MeetCueEvent::QuestionPoint { id, .. } | MeetCueEvent::AnswerDelta { id, .. } => {
                assert!(seen.contains(id), "point before question {id}");
            }
            _ => {}
        }
    }

    // Sample answers stream in and finish.
    let answer: String = events
        .iter()
        .filter_map(|e| match e {
            MeetCueEvent::AnswerDelta { id: 1, text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(answer.contains("**双活数据中心**"), "{answer}");
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, MeetCueEvent::AnswerDone { .. }))
            .count(),
        3
    );

    // Latency: one sample per question, well under the 1.5 s target with mocks;
    // the first question was triggered early from a partial result.
    let samples: Vec<(u32, i64, bool)> = events
        .iter()
        .filter_map(|e| match e {
            MeetCueEvent::LatencySample {
                id,
                total_ms,
                early,
                ..
            } => Some((*id, *total_ms, *early)),
            _ => None,
        })
        .collect();
    assert_eq!(samples.len(), 3);
    for (id, total, _) in &samples {
        assert!(*total > 0 && *total < 1500, "question {id}: {total} ms");
    }
    assert!(samples[0].2, "early trigger used");

    // The echo of question 1 on the microphone is not treated as the user.
    assert!(!events.iter().any(|e| matches!(
        e,
        MeetCueEvent::Transcript { channel: meetcue_core::Channel::Me, is_final: true, text, .. }
            if text.contains("怎么做高可用")
    )));

    // Usage is metered with known prices.
    assert!(summary.total_micros > 0);
    assert!(events
        .iter()
        .any(|e| matches!(e, MeetCueEvent::Usage { cost_micros: Some(c), .. } if *c > 0)));
    // No errors.
    let errors: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, MeetCueEvent::Error { .. }))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");

    // History holds the session for late subscribers, without partials.
    assert!(history.len() > 20);
    assert!(!history.iter().any(|t| matches!(
        t.event,
        MeetCueEvent::Transcript {
            is_final: false,
            ..
        }
    )));
    assert!(history.windows(2).all(|w| w[0].t_ms <= w[1].t_ms));
}
