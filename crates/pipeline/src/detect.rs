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
}

/// Builds the detection request. `transcript` already ends with the sentence
/// to classify as its last `[Them]` line.
pub fn request(model: &str, transcript: &str, output_language: &str) -> LlmRequest {
    LlmRequest::new(model, UsagePurpose::Detect)
        .system(
            prompts::render(prompts::DETECT, &[("output_language", output_language)]),
            true,
        )
        .user(transcript.to_string())
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

        assert!(parse("not json").is_none());
        assert!(parse("{broken").is_none());
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
        assert!(r.system.contains("Simplified Chinese"));
        assert!(r.cache_system);
        assert_eq!(r.messages.len(), 1);
    }
}
