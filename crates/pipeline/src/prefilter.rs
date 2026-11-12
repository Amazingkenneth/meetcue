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

/// Whether a finished sentence from the other side goes to the model, which
/// decides by its meaning whether the user is expected to answer. Wording and
/// punctuation don't decide this: "说一下你对微服务的理解" asks for an answer
/// without a question word or mark, and recognition often drops the "?".
/// Only fragments too short to mean anything and back-channel tags are skipped.
pub fn worth_checking(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && !is_tag_question(trimmed)
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

/// Whether `next`, said right after `prev` by the same speaker with nobody
/// answering in between, finishes the sentence `prev` started. Recognizers
/// end a sentence at every pause and often put a full stop there, so a
/// question cut in two rarely looks unfinished: "如何制定数据中心IP。" /
/// "V6的迁移方案，同时保证。" / "存量用户不受影响？". Anything that follows a
/// sentence not yet closed by a question mark is taken as its rest, and so is
/// a short tail ending in one ("…你会如何。" / "调整？"). A full sentence after
/// a question mark may be a new question, which the model decides.
pub fn finishes(prev: &str, next: &str) -> bool {
    if continues(prev, next) {
        return true;
    }
    let next = strip_fillers(next).trim();
    let n = normalize(next);
    if n.is_empty() || TAG_QUESTIONS.iter().any(|t| n == *t) || FILLERS.contains(&n.as_str()) {
        return false;
    }
    let asks = next.ends_with(['?', '？']);
    if meaningful_len(next) < MIN_LEN && (asks || BACK_CHANNELS.contains(&n.as_str())) {
        return asks;
    }
    !prev.trim().ends_with(['?', '？'])
}

/// Replies such as "对。" that the other side says while listening.
const BACK_CHANNELS: &[&str] = &[
    "对", "对对", "好", "好的", "是", "是的", "嗯嗯", "哦", "噢", "行", "可以", "yes", "yeah",
    "yep", "sure", "fine", "great", "cool", "ok", "okay", "right",
];

/// Words that carry no part of a question on their own: acknowledgements,
/// thanks and connectives such as "好的。" / "那么，" / "OK, thanks."
const NOT_A_FRAGMENT: &[&str] = &[
    "谢谢",
    "感谢",
    "明白",
    "了解",
    "收到",
    "没问题",
    "不错",
    "很好",
    "挺好",
    "好嘞",
    "哈哈",
    "你好",
    "您好",
    "大家好",
    "那么",
    "然后",
    "接下来",
    "下面",
    "那",
    "下一个问题",
    "下一题",
    "最后一个问题",
    "thankyou",
    "thanks",
    "gotit",
    "isee",
    "alright",
    "nice",
    "good",
    "mhm",
    "hmm",
    "hello",
    "hi",
    "hey",
    "next",
    "nextquestion",
];

/// Particles that close a Chinese sentence ("我们开始吧。"), so it is not the
/// first half of the next one.
const ZH_CLOSING_PARTICLES: &[char] = &['吧', '了', '啊', '呀', '哈', '嘛', '呢', '吗', '哦', '啦'];

/// Most a sentence may hold, in display units (one per Chinese character,
/// half per letter), to count as the first words of a question cut off by a
/// pause.
const FRAGMENT_MAX_UNITS: f64 = 8.0;

/// Whether `text` is a short piece of an unfinished sentence, such as "上任后。"
/// or "你觉得在", that the speaker paused after before saying the rest of the
/// question ("一年的计划是什么？"). Recognizers end a sentence at each pause and
/// often put a full stop there, so punctuation doesn't decide it: anything this
/// short that neither asks something itself nor is only an acknowledgement or
/// connective is taken as the start of what follows.
pub fn is_fragment(text: &str) -> bool {
    let text = strip_fillers(text);
    if text.is_empty() || text.ends_with(['?', '？', '!', '！']) {
        return false;
    }
    let core = text.trim_end_matches(|c: char| !c.is_alphanumeric());
    // A Chinese sentence closed by a particle is complete. Recognizers put a
    // full stop at every pause in Chinese, but in English they keep it for
    // the end of a sentence ("Let's begin.") and leave a cut-off one open.
    if core.ends_with(ZH_CLOSING_PARTICLES)
        || (!core.chars().last().is_some_and(crate::text::is_cjk) && text.ends_with('.'))
    {
        return false;
    }
    let units: f64 = text
        .chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| if crate::text::is_cjk(c) { 1.0 } else { 0.5 })
        .sum();
    if units > FRAGMENT_MAX_UNITS {
        return false;
    }
    // Strip every acknowledgement and connective; something must remain.
    // Longest first, so "好的" is not read as "好" and a stray "的".
    let mut words: Vec<&str> = BACK_CHANNELS
        .iter()
        .chain(FILLERS)
        .chain(TAG_QUESTIONS)
        .chain(NOT_A_FRAGMENT)
        .copied()
        .collect();
    words.sort_by_key(|w| std::cmp::Reverse(w.len()));
    let mut rest = normalize(text);
    while let Some(w) = words.iter().find(|w| rest.starts_with(**w)) {
        rest.drain(..w.len());
    }
    !rest.is_empty()
}

/// `text` without the spoken fillers it starts with ("嗯，那个，").
pub fn without_fillers(text: &str) -> &str {
    strip_fillers(text)
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
    fn first_words_cut_off_by_a_pause() {
        for t in [
            "上任后。",
            "上任后",
            "嗯，你入职以后。",
            "那你觉得在",
            "对于这个项目，",
            "After you join,",
        ] {
            assert!(is_fragment(t), "{t}");
        }
        for t in [
            "好的，谢谢。",
            "嗯。",
            "我们开始吧。",
            "你好。",
            "下一个问题。",
            "OK, thanks.",
            "Let's begin.",
            "上任后呢？",
            "我们公司主要做跨境电商的支付系统。",
        ] {
            assert!(!is_fragment(t), "{t}");
        }
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
    fn rest_of_a_question_after_a_full_stop() {
        assert!(finishes("如何制定数据中心IP。", "V6的迁移方案，同时保证。"));
        assert!(finishes("V6的迁移方案，同时保证。", "存量用户不受影响？"));
        assert!(finishes("我们发现了这些问题，那你会如何。", "调整？"));
        assert!(finishes("备份系统如何体现真正的容灾。", "能力？"));
        assert!(finishes("某个边缘的PO。", "P。"));
        assert!(!finishes("你有什么优势。", "对。"));
        assert!(!finishes("如何评估一项新系统投资？", "是否。"));
        assert!(finishes(
            "How would you design a zero trust",
            "network for a remote team?"
        ));
        assert!(finishes(
            "What is a hash map?",
            "And how is it different from a tree?"
        ));
        // A full new sentence after a question is left to the model.
        assert!(!finishes(
            "What is a hash map?",
            "How do you handle on-call incidents?"
        ));
        assert!(!finishes("说说你对微服务的理解？", "好的"));
        // Back-channels and short remarks never join.
        assert!(!finishes("说说你对微服务的理解。", "嗯。"));
        assert!(!finishes("说说你对微服务的理解。", "对吧？"));
        assert!(!finishes("Tell me about your last project.", "Right?"));
        assert!(!finishes("Tell me about your last project.", "OK."));
    }

    #[test]
    fn every_meaningful_sentence_is_checked() {
        assert!(worth_checking("说一下你对微服务的理解"));
        assert!(worth_checking("下面聊聊你上一份工作"));
        assert!(worth_checking("I'd love to hear about your last role"));
        assert!(worth_checking(
            "Okay so the next part is about system design"
        ));
        assert!(!worth_checking("对吧？"));
        assert!(!worth_checking("嗯"));
        assert!(!worth_checking("Right?"));
        assert!(!worth_checking("  "));
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
