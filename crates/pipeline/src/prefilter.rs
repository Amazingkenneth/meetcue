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
    "谈谈",
    "聊聊",
    "分享一下",
    "看法",
    "想法",
    "区别",
    "举个例子",
    "展开说",
    "详细说",
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
    "what's",
    "how's",
    "talk about",
    "talk me through",
    "talk us through",
    "walk us through",
    "tell us",
    "thoughts on",
    "your thoughts",
    "your take",
    "your experience",
    "give me an example",
    "give us an example",
    "share",
    "elaborate",
    "compare",
    "difference between",
    "pros and cons",
    "curious about",
    "i'd like to hear",
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

/// Words that open the rest of a question the speaker started before a pause
/// ("…idea A" / "and idea B").
const ZH_JOIN_STARTS: &[&str] = &[
    "和",
    "以及",
    "还有",
    "另外",
    "并且",
    "而且",
    "或者",
    "或是",
    "跟",
    "与",
    "同时",
    "包括",
    "顺便",
    "再说说",
    "再讲讲",
    "再谈谈",
];

const EN_JOIN_STARTS: &[&str] = &[
    "and",
    "also",
    "plus",
    "or",
    "as well as",
    "along with",
    "together with",
    "what about",
    "how about",
    "as for",
    "in addition",
    "compared to",
    "compared with",
    "versus",
    "vs",
];

/// Endings that leave a sentence unfinished.
const ZH_JOIN_ENDS: &[&str] = &[
    "和", "以及", "还有", "跟", "与", "或者", "关于", "对于", "，", "、", ",",
];

const EN_JOIN_ENDS: &[&str] = &[
    "and", "or", "about", "between", "of", "on", "the", "with", "both", "like", "your",
];

/// Spoken fillers skipped before looking for a joining word.
const FILLERS: &[&str] = &[
    "嗯", "呃", "啊", "那个", "就是", "um", "uh", "er", "so", "well",
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

/// Whether `next` carries on the question `prev` started: it opens with a
/// joining word ("and…", "还有…") or `prev` breaks off mid-phrase ("…and",
/// "…，"). Speakers pause mid-question, and the recognizer then ends the
/// sentence there.
pub fn continues(prev: &str, next: &str) -> bool {
    let next = strip_fillers(next);
    if next.is_empty() {
        return false;
    }
    let prev = prev.trim();
    let prev_lower = prev.to_lowercase();
    let prev_words: Vec<&str> = prev_lower
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .collect();
    let dangling = ZH_JOIN_ENDS.iter().any(|e| prev.ends_with(e))
        || (!prev.ends_with(['.', '?', '!', '。', '？', '！'])
            && prev_words.last().is_some_and(|w| EN_JOIN_ENDS.contains(w)));
    if dangling {
        return true;
    }
    if ZH_JOIN_STARTS.iter().any(|s| next.starts_with(s)) {
        return true;
    }
    let lower = format!(
        "{} ",
        next.to_lowercase().replace([',', '.', '?', '!'], " ")
    );
    EN_JOIN_STARTS
        .iter()
        .any(|s| lower.starts_with(&format!("{s} ")))
}

fn strip_fillers(text: &str) -> &str {
    let mut rest = text.trim();
    loop {
        let lower = rest.to_lowercase();
        let Some(f) = FILLERS.iter().find(|f| {
            lower.starts_with(**f)
                && rest[f.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_ascii_alphanumeric())
        }) else {
            return rest;
        };
        rest = rest[f.len()..].trim_start_matches(|c: char| {
            c.is_whitespace() || matches!(c, ',' | '，' | '、' | '.' | '…')
        });
    }
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
    fn requests_without_question_words() {
        assert!(might_be_question(
            "Talk about your thoughts on microservices."
        ));
        assert!(might_be_question("What's your take on remote work"));
        assert!(might_be_question(
            "Share a time you disagreed with your manager."
        ));
        assert!(might_be_question("谈谈你对微服务的看法"));
        assert!(!might_be_question("Thanks, that makes sense."));
    }

    #[test]
    fn continuations_of_a_question() {
        let q = "Talk about your thoughts on idea A";
        assert!(continues(q, "and idea B."));
        assert!(continues(q, "Um, and also idea B"));
        assert!(continues(q, "What about idea B?"));
        assert!(continues("Compare Kafka and", "RabbitMQ for us."));
        assert!(continues("说说你对微服务的理解，", "以及服务网格"));
        assert!(continues("说说你对微服务的理解", "嗯，还有服务网格"));
        assert!(continues("你怎么看 A 和", "B 的区别"));
        assert!(!continues(q, "Okay, great."));
        assert!(!continues(q, "Andrew will join later."));
        assert!(!continues("What is your name?", "Next one."));
        assert!(!continues("说说你对微服务的理解", "好的"));
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
