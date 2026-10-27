//! Local progress tracking ("which points has the user already said") and
//! echo filtering. Both use text overlap only; no model is called.

use std::collections::{HashMap, HashSet};

use meetcue_core::PointStatus;

use crate::text::{containment, normalize, split_terms, unit_set};

/// A term counts as said when this share of its units appears in the answer.
const TERM_THRESHOLD: f64 = 0.5;
/// A point is done when this share of its terms has been said.
const POINT_THRESHOLD: f64 = 0.5;
/// A microphone sentence is an echo when this share of it matches a remote sentence.
pub const ECHO_THRESHOLD: f64 = 0.7;

/// Precomputed vocabulary of one hint point.
#[derive(Debug, Clone)]
pub struct PointVocab {
    head_norm: String,
    terms: Vec<HashSet<String>>,
}

impl PointVocab {
    pub fn new(head: &str, rest: &str) -> Self {
        let terms = split_terms(head)
            .into_iter()
            .chain(split_terms(rest))
            .map(|t| unit_set(&t))
            .filter(|u| !u.is_empty())
            .collect();
        Self {
            head_norm: normalize(head),
            terms,
        }
    }

    pub fn is_covered(&self, answer: &AnswerText) -> bool {
        if !self.head_norm.is_empty() && answer.norm.contains(&self.head_norm) {
            return true;
        }
        if self.terms.is_empty() {
            return false;
        }
        let said = self
            .terms
            .iter()
            .filter(|t| containment(t, &answer.units) >= TERM_THRESHOLD)
            .count();
        said as f64 / self.terms.len() as f64 >= POINT_THRESHOLD
    }
}

/// Everything the user has said for one question, in matchable form.
#[derive(Debug, Default, Clone)]
pub struct AnswerText {
    norm: String,
    units: HashSet<String>,
}

impl AnswerText {
    pub fn push(&mut self, sentence: &str) {
        self.norm.push_str(&normalize(sentence));
        self.units.extend(unit_set(sentence));
    }

    pub fn is_empty(&self) -> bool {
        self.norm.is_empty()
    }
}

/// Status of every point: covered points are `Done`, the first remaining one is
/// `Next`, the rest `Todo`. Once a point is done it stays done. Points the
/// user ticked or unticked by hand (`manual`, by index) keep that choice.
pub fn statuses(
    points: &[PointVocab],
    previous: &[PointStatus],
    answer: &AnswerText,
    manual: &HashMap<usize, bool>,
) -> Vec<PointStatus> {
    let mut out: Vec<PointStatus> = points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let done = match manual.get(&i) {
                Some(&done) => done,
                None => previous.get(i) == Some(&PointStatus::Done) || p.is_covered(answer),
            };
            if done {
                PointStatus::Done
            } else {
                PointStatus::Todo
            }
        })
        .collect();
    if let Some(next) = out.iter_mut().find(|s| **s != PointStatus::Done) {
        *next = PointStatus::Next;
    }
    out
}

/// Whether a microphone sentence is the remote side leaking through the
/// speakers rather than the user speaking.
pub fn is_echo<'a>(
    mic_sentence: &str,
    remote_sentences: impl IntoIterator<Item = &'a str>,
) -> bool {
    let mine = unit_set(mic_sentence);
    if mine.len() < 2 {
        return false;
    }
    remote_sentences
        .into_iter()
        .any(|r| containment(&mine, &unit_set(r)) >= ECHO_THRESHOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(text: &str) -> AnswerText {
        let mut a = AnswerText::default();
        a.push(text);
        a
    }

    #[test]
    fn chinese_point_matching() {
        let p = PointVocab::new("跨机房", "节点分布 避免单点");
        assert!(!p.is_covered(&answer("控制器是三节点集群部署")));
        assert!(p.is_covered(&answer("三个节点分布在不同机房，避免单点故障")));
        // The head said verbatim is enough.
        let p = PointVocab::new("双活数据中心", "两地部署 流量分担");
        assert!(p.is_covered(&answer("我们是双活数据中心")));
    }

    #[test]
    fn english_point_matching() {
        let p = PointVocab::new("Active-active", "two regions, shared traffic");
        assert!(!p.is_covered(&answer("We have a single data center")));
        assert!(p.is_covered(&answer("We run active active across two regions")));
        let p = PointVocab::new("Southbound", "OpenFlow 1.3 NETCONF");
        assert!(p.is_covered(&answer(
            "For the southbound side we use OpenFlow 1.3 and NETCONF"
        )));
        assert!(!p.is_covered(&answer("The northbound API is REST")));
        // Plurals and tenses still count.
        let p = PointVocab::new("Multi-region", "deploy services in two regions");
        assert!(p.is_covered(&answer(
            "Each service is deployed in a second region as well"
        )));
    }

    #[test]
    fn statuses_mark_next_and_keep_done() {
        let points = vec![
            PointVocab::new("集群部署", "三节点 Raft"),
            PointVocab::new("跨机房", "节点分布"),
            PointVocab::new("南向协议", "OpenFlow"),
        ];
        let none = HashMap::new();
        let s = statuses(&points, &[], &AnswerText::default(), &none);
        assert_eq!(s, [PointStatus::Next, PointStatus::Todo, PointStatus::Todo]);
        let s = statuses(&points, &s, &answer("我们是三节点集群部署，用 Raft"), &none);
        assert_eq!(s, [PointStatus::Done, PointStatus::Next, PointStatus::Todo]);
        // Saying point 3 first skips point 2.
        let s2 = statuses(&points, &s, &answer("南向协议是 OpenFlow"), &none);
        assert_eq!(
            s2,
            [PointStatus::Done, PointStatus::Next, PointStatus::Done]
        );
    }

    #[test]
    fn manual_ticks_win_over_tracking() {
        let points = vec![
            PointVocab::new("集群部署", "三节点 Raft"),
            PointVocab::new("跨机房", "节点分布"),
        ];
        let said = answer("我们是三节点集群部署，用 Raft");
        let manual = HashMap::from([(0, false), (1, true)]);
        let s = statuses(&points, &[PointStatus::Done], &said, &manual);
        assert_eq!(s, [PointStatus::Next, PointStatus::Done]);
    }

    #[test]
    fn echo_detection() {
        let remote = ["你们的网络架构是怎么做高可用的？"];
        assert!(is_echo("你们的网络架构是怎么做高可用的", remote));
        assert!(is_echo("网络架构是怎么做高可用", remote));
        assert!(!is_echo("我们主要是两地双活数据中心", remote));
        assert!(!is_echo("嗯", remote));
        assert!(is_echo(
            "how do you make it highly available",
            ["So, how do you make it highly available?"]
        ));
    }
}
