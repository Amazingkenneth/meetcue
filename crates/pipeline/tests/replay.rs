//! Replays labelled interviews from `samples/replay/*.json` through the
//! engine and checks the question cards that come out.
//!
//! Each file lists what was said, sentence by sentence, as recognition would
//! deliver it. A remote sentence the detection model should be asked about
//! carries the model's reply under `detect`; a sentence without one must be
//! handled without the model (joined to a question, skipped as a repeat or a
//! back-channel). `questions` is the text of every card at the end, in the
//! order they were asked. This guards the splitting and joining rules, which
//! change often, against the cases that went wrong before.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use meetcue_core::{Channel, MeetCueEvent};
use meetcue_pipeline::engine::{Engine, EngineConfig, Input, Output};
use meetcue_pipeline::provider::AsrResult;
use meetcue_pipeline::usage::CostMeter;
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    lines: Vec<Line>,
    questions: Vec<String>,
}

#[derive(Deserialize)]
struct Line {
    ch: String,
    t0: i64,
    t1: i64,
    text: String,
    #[serde(default)]
    detect: Option<serde_json::Value>,
}

struct Run {
    engine: Engine,
    now: i64,
    /// Detection requests not answered yet, oldest first.
    detects: Vec<u64>,
    hints: Vec<u64>,
    cards: BTreeMap<u32, (u32, String)>,
}

impl Run {
    fn absorb(&mut self, out: Vec<Output>) {
        for o in out {
            match o {
                Output::Detect { req, .. } => self.detects.push(req),
                Output::Hints { req, .. } => self.hints.push(req),
                Output::Cancel { req } => {
                    self.detects.retain(|r| *r != req);
                    self.hints.retain(|r| *r != req);
                }
                Output::Emit(MeetCueEvent::QuestionNew {
                    id, version, text, ..
                }) => {
                    let e = self.cards.entry(id).or_insert((0, String::new()));
                    if version >= e.0 {
                        *e = (version, text);
                    }
                }
                Output::Emit(MeetCueEvent::QuestionRemoved { id }) => {
                    self.cards.remove(&id);
                }
                _ => {}
            }
        }
    }

    fn at(&mut self, t: i64) {
        while self.now + 50 <= t {
            self.now += 50;
            let out = self.engine.handle(Input::Tick, self.now);
            self.absorb(out);
        }
        self.now = self.now.max(t);
    }

    fn input(&mut self, input: Input) {
        let out = self.engine.handle(input, self.now);
        self.absorb(out);
    }

    /// Ends every hint request at once, so slots never run out.
    fn finish_hints(&mut self) {
        for req in std::mem::take(&mut self.hints) {
            self.input(Input::HintsDone { req });
        }
    }
}

fn replay(case: &Case) -> (Vec<String>, Vec<String>) {
    let cfg = EngineConfig {
        early_trigger: false,
        speculative_hints: true,
        skip_repeated_sentences: true,
        ..Default::default()
    };
    let mut run = Run {
        engine: Engine::new(cfg, CostMeter::new(vec![], Default::default(), 0)),
        now: 0,
        detects: Vec::new(),
        hints: Vec::new(),
        cards: BTreeMap::new(),
    };
    let out = run.engine.start(0);
    run.absorb(out);
    let mut problems = Vec::new();
    for line in &case.lines {
        let channel = if line.ch == "me" {
            Channel::Me
        } else {
            Channel::Remote
        };
        let before = run.detects.len();
        run.at(line.t0);
        let half: String = line
            .text
            .chars()
            .take(line.text.chars().count() / 2)
            .collect();
        run.input(Input::Asr {
            channel,
            result: AsrResult::Partial {
                text: half,
                t0_ms: line.t0,
                t1_ms: line.t0,
            },
        });
        run.at(line.t1);
        run.input(Input::Asr {
            channel,
            result: AsrResult::Final {
                text: line.text.clone(),
                t0_ms: line.t0,
                t1_ms: line.t1,
            },
        });
        run.at(line.t1 + 100);
        run.input(Input::Asr {
            channel,
            result: AsrResult::Endpoint { t_ms: line.t1 },
        });
        let asked = run.detects.len() > before;
        match (&line.detect, asked) {
            (Some(reply), true) => {
                let req = run.detects.pop().unwrap();
                run.input(Input::DetectDone {
                    req,
                    reply: Ok(reply.to_string()),
                });
            }
            (Some(_), false) => problems.push(format!("not sent to detection: {}", line.text)),
            (None, true) => {
                problems.push(format!("sent to detection: {}", line.text));
                let req = run.detects.pop().unwrap();
                run.input(Input::DetectDone {
                    req,
                    reply: Ok(r#"{"isQuestion":false}"#.into()),
                });
            }
            (None, false) => {}
        }
        run.finish_hints();
    }
    let cards = run.cards.into_values().map(|(_, text)| text).collect();
    (cards, problems)
}

#[test]
fn labelled_interviews_give_the_expected_cards() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/replay");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("samples/replay")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty());
    let mut failures: HashMap<String, String> = HashMap::new();
    for file in &files {
        let case: Case = serde_json::from_str(&std::fs::read_to_string(file).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let (cards, problems) = replay(&case);
        if cards != case.questions || !problems.is_empty() {
            failures.insert(
                file.file_name().unwrap().to_string_lossy().into(),
                format!(
                    "{}\n  expected {:?}\n  got      {:?}\n  {}",
                    case.name,
                    case.questions,
                    cards,
                    problems.join("\n  ")
                ),
            );
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        failures.into_values().collect::<Vec<_>>().join("\n\n")
    );
}
