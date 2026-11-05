//! LLM question detection (prompt A) and its tolerant result parser.

use serde::Deserialize;
use serde_json::Value;

use meetcue_core::UsagePurpose;

use crate::prompts;
use crate::provider::LlmRequest;
use crate::text::{display_units, truncate_chars};

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Detection {
    pub is_question: bool,
    pub directed_to_me: bool,
    pub merged: bool,
    pub interruption: bool,
    pub short: String,
    pub sub_questions: Vec<String>,
    pub tags: Vec<String>,
    /// How many [Them] lines just before the last one set this question up.
    pub lead_in: u32,
}

/// Builds the detection request. `transcript` already ends with the sentence
/// to classify as its last `[Them]` line.
pub fn request(model: &str, transcript: &str, output_language: &str) -> LlmRequest {
    LlmRequest::new(model, UsagePurpose::Detect)
        // The language goes last so the instructions stay the same, and
        // cached, whichever language the meeting is in.
        .system(prompts::DETECT, true)
        .user(format!(
            "{}\n\nOutput language: {output_language}",
            transcript.trim_end()
        ))
        .max_tokens(prompts::DETECT_MAX_TOKENS)
        .temperature(0.0)
}

/// Parses the model reply. Accepts surrounding prose or code fences and
/// booleans written as strings.
pub fn parse(reply: &str) -> Option<Detection> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    if end <= start {
        return None;
    }
    let mut v: Value = serde_json::from_str(&reply[start..=end]).ok()?;
    let obj = v.as_object_mut()?;
    for key in ["isQuestion", "directedToMe", "merged", "interruption"] {
        if let Some(Value::String(s)) = obj.get(key) {
            let b = matches!(s.to_ascii_lowercase().as_str(), "true" | "yes" | "1");
            obj.insert(key.into(), Value::Bool(b));
        }
    }
    // A count written as text or out of range is read leniently, not as a
    // reason to discard the whole reply.
    if let Some(v) = obj.get("leadIn") {
        let n = match v {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        }
        .filter(|n| n.is_finite() && *n > 0.0)
        .map_or(0, |n| n.min(10.0) as u32);
        obj.insert("leadIn".into(), Value::from(n));
    }
    for key in ["subQuestions", "tags"] {
        if let Some(Value::String(s)) = obj.get(key) {
            let list = if s.is_empty() {
                vec![]
            } else {
                vec![Value::String(s.clone())]
            };
            obj.insert(key.into(), Value::Array(list));
        }
    }
    let mut d: Detection = serde_json::from_value(v).ok()?;
    d.short = d.short.trim().to_string();
    d.tags.retain(|t| !t.trim().is_empty());
    d.tags.truncate(3);
    d.sub_questions.retain(|t| !t.trim().is_empty());
    Some(d)
}

/// Whether `reply` already holds a whole JSON object, so it can be used
/// without waiting for the rest of the stream (closing events, usage).
pub fn is_complete(reply: &str) -> bool {
    let Some(start) = reply.find('{') else {
        return false;
    };
    let (mut depth, mut in_str, mut escaped) = (0usize, false, false);
    for c in reply[start..].chars() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Fallback short title when the model gave none: the question itself, cut
/// to fit the queue card.
pub fn short_title(short: &str, text: &str) -> String {
    let base = if short.trim().is_empty() {
        text.trim()
    } else {
        short.trim()
    };
    let base = base.trim_end_matches(['?', '？', '。', '.', '!', '！']);
    if display_units(base) <= 14.0 {
        return base.to_string();
    }
    // Cut by display width: 14 CJK characters or about 28 Latin characters.
    let mut width = 0.0;
    let mut n = 0;
    for c in base.chars() {
        width += if crate::text::is_cjk(c) { 1.0 } else { 0.5 };
        if width > 13.0 {
            break;
        }
        n += 1;
    }
    truncate_chars(base, n + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_and_messy_replies() {
        let d = parse(r#"{"isQuestion":true,"directedToMe":true,"merged":false,"short":"高可用架构","subQuestions":[],"tags":["指向你"]}"#).unwrap();
        assert!(d.is_question && d.directed_to_me && !d.merged && !d.interruption);
        assert_eq!(d.short, "高可用架构");

        let d = parse("Here you go:\n```json\n{\"isQuestion\":\"true\",\"directedToMe\":\"no\",\"tags\":\"插话\"}\n```").unwrap();
        assert!(d.is_question);
        assert!(!d.directed_to_me);
        assert_eq!(d.tags, ["插话"]);
        assert!(d.sub_questions.is_empty());

        let d = parse(r#"{"isQuestion":true,"leadIn":2}"#).unwrap();
        assert_eq!(d.lead_in, 2);
        assert_eq!(
            parse(r#"{"isQuestion":true,"leadIn":"1"}"#)
                .unwrap()
                .lead_in,
            1
        );
        assert_eq!(
            parse(r#"{"isQuestion":true,"leadIn":-3}"#).unwrap().lead_in,
            0
        );
        assert_eq!(
            parse(r#"{"isQuestion":true,"leadIn":null}"#)
                .unwrap()
                .lead_in,
            0
        );

        assert!(parse("not json").is_none());
        assert!(parse("{broken").is_none());
    }

    #[test]
    fn complete_replies_are_recognised_early() {
        assert!(!is_complete(""));
        assert!(!is_complete(r#"{"isQuestion":true,"short":"a}"#));
        assert!(!is_complete(
            r#"{"isQuestion":true,"tags":["x"],"sub":{"a":1}"#
        ));
        assert!(is_complete(
            r#"{"isQuestion":true,"short":"a}b\"c","sub":{"a":1}}"#
        ));
        assert!(is_complete("```json\n{\"isQuestion\":false}"));
    }

    #[test]
    fn short_titles_fit() {
        assert_eq!(
            short_title("", "控制器挂了还能转发吗？"),
            "控制器挂了还能转发吗"
        );
        let long = short_title("", "你们的网络架构整体上是怎么设计并实现高可用的呢");
        assert!(display_units(&long) <= 14.0, "{long}");
        assert!(long.ends_with('…'));
        let en = short_title(
            "",
            "How do you make the whole network highly available across sites?",
        );
        assert!(display_units(&en) <= 14.0, "{en}");
        assert_eq!(short_title("SDN 部署", "whatever"), "SDN 部署");
    }

    #[test]
    fn request_shape() {
        let r = request("m", "[Them] 你好吗？\n", "Simplified Chinese");
        assert_eq!(r.max_tokens, 200);
        assert!(r.cache_system);
        // The system prompt is the same in every language; the language is in the message.
        assert_eq!(r.system, request("m", "x", "English").system);
        assert!(r.messages[0]
            .content
            .ends_with("Output language: Simplified Chinese"));
        assert_eq!(r.messages.len(), 1);
    }
}
