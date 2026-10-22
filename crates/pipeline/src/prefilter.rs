//! Rule-based question pre-filter for the remote channel.
//!
//! Runs on every remote sentence without calling a model; only sentences that
//! pass are sent to the language model for question detection.

use crate::text::{meaningful_len, normalize};

const ZH_MARKERS: &[&str] = &[
    "吗",
    "呢",
    "怎么",
    "如何",
    "为什么",
    "为何",
    "是否",
    "能不能",
    "可不可以",
    "哪些",
    "哪个",
    "多少",
    "什么",
    "你觉得",
    "你认为",
    "说说",
    "讲讲",
    "介绍一下",
    "有没有",
    "会不会",
    "是不是",
];

const ZH_PREFIXES: &[&str] = &["请", "麻烦"];

const EN_MARKERS: &[&str] = &[
    "what",
    "how",
    "why",
    "which",
    "who",
    "when",
    "where",
    "could you",
    "can you",
    "would you",
    "tell me",
    "walk me through",
    "describe",
    "explain",
    "do you",
    "did you",
    "have you",
    "are you",
    "is there",
];

/// Short confirmations that end with a question mark but expect no answer.
const TAG_QUESTIONS: &[&str] = &[
    "对吧",
    "是吧",
    "是吗",
    "对不对",
    "好吗",
    "好吧",
    "对吗",
    "行吗",
    "明白吗",
    "懂吗",
    "right",
    "ok",
    "okay",
    "isntit",
    "youknow",
    "yeah",
    "really",
    "sure",
];

/// Minimum meaningful characters for a sentence to be considered a question.
const MIN_LEN: usize = 4;

/// Whether a remote sentence might be a question worth sending to the model.
pub fn might_be_question(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || is_tag_question(trimmed) {
        return false;
    }
    if trimmed.contains('?') || trimmed.contains('？') {
        return true;
    }
    if ZH_PREFIXES.iter().any(|p| trimmed.starts_with(p)) {
        return true;
    }
    if ZH_MARKERS.iter().any(|m| trimmed.contains(m)) {
        return true;
    }
    let lower = format!(" {} ", trimmed.to_lowercase().replace(['.', ',', '!'], " "));
    EN_MARKERS.iter().any(|m| lower.contains(&format!(" {m} ")))
}

/// "对吧？", "Right?" and similar back-channel questions.
pub fn is_tag_question(text: &str) -> bool {
    let n = normalize(text);
    meaningful_len(text) < MIN_LEN || TAG_QUESTIONS.iter().any(|t| n == *t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_questions() {
        assert!(might_be_question("你们的网络架构是怎么做高可用的"));
        assert!(might_be_question("控制器挂了还能转发吗"));
        assert!(might_be_question("请介绍一下你最近的项目"));
        assert!(might_be_question("南向用的协议是哪个？"));
        assert!(might_be_question("说说你对微服务的理解"));
        assert!(!might_be_question("好的，我们开始吧。"));
        assert!(!might_be_question("嗯，明白了。"));
    }

    #[test]
    fn english_questions() {
        assert!(might_be_question("How do you handle failover"));
        assert!(might_be_question("Could you walk me through the design"));
        assert!(might_be_question("Tell me about your last project."));
        assert!(!might_be_question("Thanks, that makes sense."));
        assert!(!might_be_question("Somehow it worked"));
    }

    #[test]
    fn tag_questions_are_ignored() {
        assert!(!might_be_question("对吧？"));
        assert!(!might_be_question("是吗？"));
        assert!(!might_be_question("Right?"));
        assert!(!might_be_question("OK?"));
        assert!(might_be_question("这个方案对吗，有什么风险？"));
    }
}
