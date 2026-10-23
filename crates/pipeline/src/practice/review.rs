//! The practice review: per-answer figures, the session summary, model
//! prompts for follow-ups, line classification and feedback, and the
//! Markdown export.

use meetcue_core::settings::{HintLevel, PlaybackMode};
use meetcue_core::UsagePurpose;

use super::schedule::AnswerRecord;
use super::{FillerCount, PracticeReport, QuestionReview, ReviewSummary};
use crate::prompts;
use crate::provider::LlmRequest;
use crate::text::{self, Lang};

/// Counts filler words. Chinese fillers are counted as substrings, English
/// ones as whole words or phrases, case-insensitively.
pub fn count_fillers(
    answer: &str,
    fillers_zh: &[String],
    fillers_en: &[String],
) -> Vec<FillerCount> {
    let words: Vec<String> = answer
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty() && !w.chars().any(text::is_cjk))
        .map(str::to_lowercase)
        .collect();
    let mut out = Vec::new();
    for f in fillers_zh {
        let f = f.trim();
        if f.is_empty() {
            continue;
        }
        let n = answer.matches(f).count() as u32;
        if n > 0 {
            out.push(FillerCount {
                word: f.to_string(),
                count: n,
            });
        }
    }
    for f in fillers_en {
        let phrase: Vec<String> = f.split_whitespace().map(str::to_lowercase).collect();
        if phrase.is_empty() || words.len() < phrase.len() {
            continue;
        }
        let n = words
            .windows(phrase.len())
            .filter(|w| *w == phrase.as_slice())
            .count() as u32;
        if n > 0 {
            out.push(FillerCount {
                word: f.trim().to_string(),
                count: n,
            });
        }
    }
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.word.cmp(&b.word)));
    out
}

/// Speaking pace: characters per minute for Chinese, words per minute for
/// English. Returns the value and its unit (`cpm` or `wpm`).
pub fn pace(answer: &str, duration_ms: i64) -> Option<(f64, &'static str)> {
    if duration_ms < 1_000 || answer.trim().is_empty() {
        return None;
    }
    let minutes = duration_ms as f64 / 60_000.0;
    match text::dominant_lang(answer) {
        Lang::Zh => {
            let n = answer.chars().filter(|&c| text::is_cjk(c)).count();
            Some((n as f64 / minutes, "cpm"))
        }
        Lang::En => {
            let n = answer
                .split_whitespace()
                .filter(|w| w.chars().any(char::is_alphanumeric))
                .count();
            Some((n as f64 / minutes, "wpm"))
        }
    }
}

pub fn question_review(
    r: &AnswerRecord,
    fillers_zh: &[String],
    fillers_en: &[String],
) -> QuestionReview {
    let start = r.first_word_ms.or(r.first_voice_ms);
    let start_latency_ms = match (r.asked_at_ms, start) {
        (Some(a), Some(s)) => Some((s - a).max(0)),
        _ => None,
    };
    let answer_ms = match (start, r.ended_at_ms) {
        (Some(s), Some(e)) if e > s => Some(e - s),
        _ => None,
    };
    let (pace_value, unit) = answer_ms
        .and_then(|ms| pace(&r.answer, ms))
        .map(|(v, u)| (Some((v * 10.0).round() / 10.0), u))
        .unwrap_or((None, ""));
    QuestionReview {
        question: r.question.clone(),
        answer: r.answer.clone(),
        reference: r.reference.clone().filter(|s| !s.trim().is_empty()),
        start_latency_ms,
        answer_ms,
        points_done: r.points_done,
        points_total: r.points_total,
        fillers: count_fillers(&r.answer, fillers_zh, fillers_en),
        pace: pace_value,
        pace_unit: unit.into(),
        feedback: Vec::new(),
        follow_up: None,
        skipped: r.skipped,
    }
}

/// Builds the per-question reviews. Follow-up answers are attached to the
/// question they followed, as `follow_up`, and also listed on their own.
pub fn reviews(
    records: &[AnswerRecord],
    fillers_zh: &[String],
    fillers_en: &[String],
) -> Vec<QuestionReview> {
    let mut out: Vec<QuestionReview> = Vec::new();
    for r in records {
        let q = question_review(r, fillers_zh, fillers_en);
        if r.is_follow_up {
            if let Some(prev) = out.iter_mut().rev().find(|p| p.follow_up.is_none()) {
                prev.follow_up = Some(r.question.clone());
            }
        }
        out.push(q);
    }
    out
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, n) = values.fold((0.0, 0u32), |(s, n), v| (s + v, n + 1));
    (n > 0).then(|| sum / n as f64)
}

pub fn summarize(questions: &[QuestionReview]) -> ReviewSummary {
    let answered: Vec<&QuestionReview> = questions.iter().filter(|q| !q.skipped).collect();
    ReviewSummary {
        questions: questions.len() as u32,
        answered: answered.len() as u32,
        avg_start_latency_ms: mean(
            answered
                .iter()
                .filter_map(|q| q.start_latency_ms)
                .map(|v| v as f64),
        ),
        avg_coverage: mean(
            answered
                .iter()
                .filter(|q| q.points_total > 0)
                .map(|q| q.points_done as f64 / q.points_total as f64),
        ),
        total_fillers: answered
            .iter()
            .flat_map(|q| q.fillers.iter().map(|f| f.count))
            .sum(),
        avg_pace: mean(answered.iter().filter_map(|q| q.pace)),
    }
}

/// Prompt C: one follow-up question for an answer.
pub fn follow_up_request(
    model: &str,
    question: &str,
    answer: &str,
    output_language: Lang,
) -> LlmRequest {
    LlmRequest::new(model, UsagePurpose::FollowUp)
        .user(prompts::render(
            prompts::FOLLOW_UP,
            &[
                ("question", question),
                (
                    "answer",
                    if answer.trim().is_empty() {
                        "(no answer)"
                    } else {
                        answer
                    },
                ),
                ("output_language", output_language.prompt_name()),
            ],
        ))
        .max_tokens(prompts::FOLLOW_UP_MAX_TOKENS)
}

/// Cleans a generated follow-up: one line, no quotes or labels.
pub fn clean_follow_up(reply: &str) -> Option<String> {
    let line = reply.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line
        .trim_start_matches(|c: char| c == '-' || c == '*' || c.is_whitespace())
        .trim_start_matches("Follow-up:")
        .trim_start_matches("追问：")
        .trim()
        .trim_matches(|c| matches!(c, '"' | '“' | '”' | '「' | '」'))
        .trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// Prompt C: label subtitle lines as questions or answers.
pub fn classify_request(model: &str, lines: &[(u32, String)]) -> LlmRequest {
    let body: String = lines.iter().map(|(i, t)| format!("{i}. {t}\n")).collect();
    LlmRequest::new(model, UsagePurpose::Classify)
        .system(prompts::CLASSIFY, false)
        .user(body)
        .max_tokens(40 + 16 * lines.len() as u32)
}

/// The JSON array somewhere in a model reply.
fn json_array(reply: &str) -> Option<serde_json::Value> {
    let start = reply.find('[')?;
    let end = reply.rfind(']')?;
    serde_json::from_str(reply.get(start..=end)?).ok()
}

/// Line numbers labelled as questions.
pub fn parse_classification(reply: &str) -> Option<Vec<u32>> {
    let arr = json_array(reply)?;
    Some(
        arr.as_array()?
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("question"))
            .filter_map(|v| v.get("i").and_then(|i| i.as_u64()).map(|i| i as u32))
            .collect(),
    )
}

/// Prompt D: three pieces of feedback against the reference answer.
pub fn feedback_request(
    model: &str,
    question: &str,
    reference: &str,
    answer: &str,
    output_language: Lang,
) -> LlmRequest {
    LlmRequest::new(model, UsagePurpose::Review)
        .user(prompts::render(
            prompts::REVIEW,
            &[
                ("question", question),
                ("reference", reference),
                (
                    "answer",
                    if answer.trim().is_empty() {
                        "(no answer)"
                    } else {
                        answer
                    },
                ),
                ("output_language", output_language.prompt_name()),
            ],
        ))
        .max_tokens(prompts::REVIEW_MAX_TOKENS)
}

/// Parses the feedback reply: a JSON array of strings, or numbered lines.
pub fn parse_feedback(reply: &str) -> Vec<String> {
    if let Some(arr) = json_array(reply).and_then(|v| v.as_array().cloned()) {
        let items: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect();
        if !items.is_empty() {
            return items.into_iter().take(3).collect();
        }
    }
    reply
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches(|c: char| {
                    c.is_ascii_digit() || matches!(c, '.' | ')' | '、' | '-' | '*')
                })
                .trim()
                .to_string()
        })
        .filter(|l| !l.is_empty() && !l.starts_with("```"))
        .take(3)
        .collect()
}

/// Replaces `{{name}}` placeholders in a translated string.
fn fill(template: &str, vars: &[(&str, String)]) -> String {
    let mut s = template.to_string();
    for (k, v) in vars {
        s = s.replace(&format!("{{{{{k}}}}}"), v);
    }
    s
}

fn secs(ms: i64) -> String {
    format!("{:.1}", ms as f64 / 1000.0)
}

fn quote(text: &str) -> String {
    text.lines().map(|l| format!("> {l}\n")).collect()
}

/// Renders the report as Markdown. `t` translates `review.*` keys of the
/// interface locale files, so the export matches the interface language.
pub fn markdown(report: &PracticeReport, t: &dyn Fn(&str) -> String) -> String {
    let mut md = format!("# {}\n\n", report.title);
    let playback = match report.playback {
        PlaybackMode::RealPipeline => t("practice.playbackReal"),
        PlaybackMode::SimulatedLatency => t("practice.playbackSimulated"),
    };
    let level = match report.hint_level {
        HintLevel::Full => t("practice.levelFull"),
        HintLevel::HeadsOnly => t("practice.levelHeads"),
        HintLevel::Delayed => t("practice.levelDelayed"),
        HintLevel::Off => t("practice.levelOff"),
    };
    md.push_str(&format!(
        "{} · {} · {}\n\n",
        report
            .started_at
            .replace('T', " ")
            .chars()
            .take(16)
            .collect::<String>(),
        playback,
        level
    ));
    if report.simulated {
        md.push_str(&format!("> {}\n\n", t("review.simulatedNote")));
    }

    let s = &report.summary;
    md.push_str(&format!("## {}\n\n", t("review.summary")));
    md.push_str("| | |\n| --- | --- |\n");
    md.push_str(&format!(
        "| {} | {} / {} |\n",
        t("review.answered"),
        s.answered,
        s.questions
    ));
    if let Some(v) = s.avg_start_latency_ms {
        md.push_str(&format!(
            "| {} | {} |\n",
            t("review.avgStart"),
            fill(&t("review.seconds"), &[("value", secs(v as i64))])
        ));
    }
    if let Some(v) = s.avg_coverage {
        md.push_str(&format!(
            "| {} | {}% |\n",
            t("review.avgCoverage"),
            (v * 100.0).round()
        ));
    }
    md.push_str(&format!(
        "| {} | {} |\n",
        t("review.fillers"),
        s.total_fillers
    ));
    if let Some(v) = s.avg_pace {
        md.push_str(&format!("| {} | {:.0} |\n", t("review.pace"), v));
    }
    md.push('\n');

    md.push_str(&format!("## {}\n\n", t("review.questions")));
    for (i, q) in report.questions.iter().enumerate() {
        md.push_str(&format!("### {}. {}\n\n", i + 1, q.question));
        if q.skipped {
            md.push_str(&format!("*{}*\n\n", t("review.skipped")));
            continue;
        }
        let mut facts = Vec::new();
        if let Some(v) = q.start_latency_ms {
            facts.push(format!(
                "{}: {}",
                t("review.timeToStart"),
                fill(&t("review.seconds"), &[("value", secs(v))])
            ));
        }
        if let Some(v) = q.answer_ms {
            facts.push(format!(
                "{}: {}",
                t("review.answerLength"),
                fill(&t("review.seconds"), &[("value", secs(v))])
            ));
        }
        if q.points_total > 0 {
            facts.push(format!(
                "{}: {}/{}",
                t("review.coverage"),
                q.points_done,
                q.points_total
            ));
        }
        if let Some(p) = q.pace {
            let unit = if q.pace_unit == "cpm" {
                t("review.cpm")
            } else {
                t("review.wpm")
            };
            facts.push(format!("{}: {:.0} {}", t("review.pace"), p, unit));
        }
        if !q.fillers.is_empty() {
            let f: Vec<String> = q
                .fillers
                .iter()
                .map(|f| format!("{} ×{}", f.word, f.count))
                .collect();
            facts.push(format!("{}: {}", t("review.fillers"), f.join(", ")));
        }
        for f in facts {
            md.push_str(&format!("- {f}\n"));
        }
        md.push('\n');
        md.push_str(&format!("**{}**\n\n", t("review.yourAnswer")));
        if q.answer.trim().is_empty() {
            md.push_str(&format!("> {}\n\n", t("review.noTranscript")));
        } else {
            md.push_str(&quote(&q.answer));
            md.push('\n');
        }
        if let Some(r) = &q.reference {
            md.push_str(&format!("**{}**\n\n", t("review.reference")));
            md.push_str(&quote(r));
            md.push('\n');
        }
        if !q.feedback.is_empty() {
            md.push_str(&format!("**{}**\n\n", t("review.feedback")));
            for (i, f) in q.feedback.iter().enumerate() {
                md.push_str(&format!("{}. {f}\n", i + 1));
            }
            md.push('\n');
        }
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zh() -> Vec<String> {
        ["嗯", "啊", "那个", "就是", "然后"]
            .map(String::from)
            .to_vec()
    }

    fn en() -> Vec<String> {
        ["um", "uh", "like", "you know"].map(String::from).to_vec()
    }

    #[test]
    fn fillers_in_both_languages() {
        let f = count_fillers(
            "嗯，那个，我们就是用 BGP，嗯然后 um you know, I like it. Unlike…",
            &zh(),
            &en(),
        );
        let get = |w: &str| f.iter().find(|x| x.word == w).map(|x| x.count);
        assert_eq!(get("嗯"), Some(2));
        assert_eq!(get("那个"), Some(1));
        assert_eq!(get("就是"), Some(1));
        assert_eq!(get("then"), None);
        assert_eq!(get("um"), Some(1));
        assert_eq!(get("you know"), Some(1));
        assert_eq!(get("like"), Some(1), "'Unlike' is not 'like'");
        assert_eq!(f[0].word, "嗯", "most frequent first");
    }

    #[test]
    fn pace_units() {
        let (v, u) = pace("我们用两台控制器做主备", 6_000).unwrap();
        assert_eq!(u, "cpm");
        assert!((v - 110.0).abs() < 0.01);
        let (v, u) = pace("We run two controllers in active standby", 3_000).unwrap();
        assert_eq!(u, "wpm");
        assert!((v - 140.0).abs() < 0.01);
        assert_eq!(pace("", 5_000), None);
        assert_eq!(pace("hi", 500), None);
    }

    fn record(asked: i64, first: i64, end: i64, answer: &str) -> AnswerRecord {
        AnswerRecord {
            item_id: 1,
            question: "Q?".into(),
            asked_at_ms: Some(asked),
            first_voice_ms: Some(first - 200),
            first_word_ms: Some(first),
            ended_at_ms: Some(end),
            answer: answer.into(),
            points_done: 2,
            points_total: 4,
            ..Default::default()
        }
    }

    #[test]
    fn review_and_summary() {
        let mut follow = record(20_000, 21_000, 25_000, "ONOS");
        follow.is_follow_up = true;
        follow.question = "Which controller?".into();
        let skipped = AnswerRecord {
            skipped: true,
            question: "Skipped?".into(),
            ..Default::default()
        };
        let recs = vec![
            record(1_000, 3_000, 9_000, "um we use two controllers"),
            follow,
            skipped,
        ];
        let qs = reviews(&recs, &zh(), &en());
        assert_eq!(qs[0].start_latency_ms, Some(2_000));
        assert_eq!(qs[0].answer_ms, Some(6_000));
        assert_eq!(qs[0].follow_up.as_deref(), Some("Which controller?"));
        assert_eq!(qs[0].pace_unit, "wpm");
        assert!(qs[2].skipped);
        let s = summarize(&qs);
        assert_eq!((s.questions, s.answered), (3, 2));
        assert_eq!(s.avg_start_latency_ms, Some(1_500.0));
        assert_eq!(s.avg_coverage, Some(0.5));
        assert_eq!(s.total_fillers, 1);
    }

    #[test]
    fn model_replies_are_parsed_leniently() {
        assert_eq!(
            parse_classification("```json\n[{\"i\":1,\"type\":\"question\"},{\"i\":2,\"type\":\"answer\"},{\"i\":3,\"type\":\"question\"}]\n```"),
            Some(vec![1, 3])
        );
        assert_eq!(parse_classification("no idea"), None);
        assert_eq!(
            parse_feedback("[\"Missed failover\", \"Say 'ECMP' first\", \"Clear structure\"]"),
            vec!["Missed failover", "Say 'ECMP' first", "Clear structure"]
        );
        assert_eq!(
            parse_feedback("1) 漏掉了故障切换\n2) 先说结论\n3) 结构清晰\n"),
            vec!["漏掉了故障切换", "先说结论", "结构清晰"]
        );
        assert_eq!(
            clean_follow_up("\n\"Which controller did you choose, and why?\"\n"),
            Some("Which controller did you choose, and why?".into())
        );
        assert_eq!(clean_follow_up("  \n"), None);
    }

    #[test]
    fn requests_render_their_prompts() {
        let r = feedback_request("m", "Q?", "Ref", "Ans", Lang::Zh);
        let body = &r.messages[0].content;
        assert!(body.contains("Reference answer: Ref") && body.contains("Simplified Chinese"));
        let r = classify_request("m", &[(1, "Hi?".into()), (2, "Hello.".into())]);
        assert!(r.messages[0].content.contains("1. Hi?\n2. Hello."));
        let r = follow_up_request("m", "Q?", "", Lang::En);
        assert!(r.messages[0].content.contains("(no answer)"));
    }

    #[test]
    fn markdown_export() {
        let qs = reviews(
            &[record(1_000, 3_000, 9_000, "um we use two controllers")],
            &zh(),
            &en(),
        );
        let mut report = PracticeReport {
            id: "r1".into(),
            title: "Network interview".into(),
            started_at: "2026-10-23T09:30:00Z".into(),
            duration_ms: 60_000,
            playback: PlaybackMode::SimulatedLatency,
            hint_level: HintLevel::Full,
            simulated: true,
            summary: summarize(&qs),
            questions: qs,
        };
        report.questions[0].reference = Some("Two controllers, active/standby.".into());
        report.questions[0].feedback = vec!["a".into(), "b".into(), "c".into()];
        let md = markdown(&report, &|k| match k {
            "review.seconds" => "{{value}} s".into(),
            other => other.to_string(),
        });
        assert!(
            md.starts_with("# Network interview\n\n2026-10-23 09:30 · practice.playbackSimulated")
        );
        assert!(md.contains("> review.simulatedNote"));
        assert!(md.contains("- review.timeToStart: 2.0 s"));
        assert!(md.contains("- review.coverage: 2/4"));
        assert!(md.contains("> um we use two controllers"));
        assert!(md.contains("> Two controllers, active/standby."));
        assert!(md.contains("3. c"));
    }
}
