//! The knowledge base: the user's files split into passages, and a local
//! BM25 search that picks the passages relevant to each question.
//!
//! Short material fits in the system prompt whole. Longer material gets a
//! short overview there, and each hint request carries the passages that
//! match its question, so a long résumé or project archive can be used
//! without making every request large.

use std::collections::{HashMap, HashSet};

use meetcue_core::settings::Material;

use crate::materials;

/// Overview of each file placed in the system prompt when retrieval is used.
pub const OVERVIEW_CHARS: usize = 1_200;
/// Characters of retrieved passages added to each hint request.
pub const RETRIEVED_CHARS: usize = 2_400;
/// Target passage length; paragraphs are kept together up to this size.
pub const PASSAGE_CHARS: usize = 600;
/// At most this many passages per request.
const MAX_PASSAGES: usize = 5;

const K1: f64 = 1.2;
const B: f64 = 0.75;

/// Common words that say nothing about the topic of a question.
const STOPWORDS: &[&str] = &[
    "a", "about", "after", "all", "also", "am", "an", "and", "any", "are", "as", "at", "be",
    "been", "but", "by", "can", "could", "did", "do", "does", "for", "from", "had", "has", "have",
    "how", "if", "in", "into", "is", "it", "its", "me", "my", "of", "on", "or", "our", "please",
    "so", "some", "tell", "than", "that", "the", "their", "them", "then", "there", "these", "they",
    "this", "to", "us", "was", "we", "were", "what", "when", "where", "which", "who", "why",
    "will", "with", "would", "you", "your",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    /// File name the passage comes from.
    pub source: String,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct KnowledgeBase {
    passages: Vec<Passage>,
    terms: Vec<HashMap<String, u32>>,
    lens: Vec<u32>,
    df: HashMap<String, u32>,
    avg_len: f64,
}

impl KnowledgeBase {
    pub fn build(materials: &[Material]) -> Self {
        let mut kb = Self::default();
        for m in materials {
            for text in split_passages(&m.text, PASSAGE_CHARS) {
                let mut tf: HashMap<String, u32> = HashMap::new();
                let words = terms(&text);
                for w in &words {
                    *tf.entry(w.clone()).or_insert(0) += 1;
                }
                for w in tf.keys() {
                    *kb.df.entry(w.clone()).or_insert(0) += 1;
                }
                kb.lens.push(words.len() as u32);
                kb.terms.push(tf);
                kb.passages.push(Passage {
                    source: m.name.clone(),
                    text,
                });
            }
        }
        let total: u64 = kb.lens.iter().map(|&l| l as u64).sum();
        kb.avg_len = total as f64 / kb.lens.len().max(1) as f64;
        kb
    }

    pub fn is_empty(&self) -> bool {
        self.passages.is_empty()
    }

    pub fn len(&self) -> usize {
        self.passages.len()
    }

    /// The best-matching passages for a query, most relevant first, within
    /// `max_chars` in total. Passages sharing no term with the query are
    /// never returned.
    pub fn search(&self, query: &str, max_chars: usize) -> Vec<&Passage> {
        let query: HashSet<String> = terms(query).into_iter().collect();
        if query.is_empty() || self.is_empty() {
            return Vec::new();
        }
        let n = self.passages.len() as f64;
        let mut scored: Vec<(f64, usize)> = self
            .terms
            .iter()
            .enumerate()
            .filter_map(|(i, tf)| {
                let len = self.lens[i] as f64;
                let score: f64 = query
                    .iter()
                    .filter_map(|q| {
                        let f = *tf.get(q)? as f64;
                        let df = *self.df.get(q)? as f64;
                        let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                        Some(idf * f * (K1 + 1.0) / (f + K1 * (1.0 - B + B * len / self.avg_len)))
                    })
                    .sum();
                (score > 0.0).then_some((score, i))
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut used = 0;
        let mut out = Vec::new();
        for (_, i) in scored {
            let p = &self.passages[i];
            let size = p.text.chars().count();
            if used + size > max_chars {
                continue;
            }
            used += size;
            out.push(p);
            if out.len() == MAX_PASSAGES {
                break;
            }
        }
        out
    }
}

/// Passages as they appear in the prompt, each headed by its file name.
pub fn render(passages: &[&Passage]) -> String {
    passages
        .iter()
        .map(|p| format!("[{}]\n{}", p.source, p.text))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// What goes into the prompts for a set of files: the system prompt
/// excerpt, and the index searched per question (empty when everything
/// already fits in the excerpt).
pub fn prepare(materials: &[Material]) -> (String, KnowledgeBase) {
    let total: usize = materials.iter().map(|m| m.text.chars().count()).sum();
    if total <= materials::SUMMARY_CHARS {
        return (
            materials::summary(materials, materials::SUMMARY_CHARS),
            KnowledgeBase::default(),
        );
    }
    (
        materials::summary(materials, OVERVIEW_CHARS),
        KnowledgeBase::build(materials),
    )
}

/// Splits text into passages of about `target` characters, keeping
/// paragraphs together and breaking long ones at sentence ends.
pub fn split_passages(text: &str, target: usize) -> Vec<String> {
    let mut pieces: Vec<String> = Vec::new();
    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        if para.chars().count() <= target {
            pieces.push(para.to_string());
            continue;
        }
        let mut cur = String::new();
        for c in para.chars() {
            cur.push(c);
            let len = cur.chars().count();
            let sentence_end = matches!(c, '.' | '!' | '?' | '\n' | '。' | '！' | '？' | '；');
            if (sentence_end && len >= target / 2) || len >= target {
                pieces.push(cur.trim().to_string());
                cur.clear();
            }
        }
        if !cur.trim().is_empty() {
            pieces.push(cur.trim().to_string());
        }
    }
    let mut out: Vec<String> = Vec::new();
    for piece in pieces {
        match out.last_mut() {
            Some(last) if last.chars().count() + piece.chars().count() < target => {
                last.push_str("\n\n");
                last.push_str(&piece);
            }
            _ => out.push(piece),
        }
    }
    out
}

/// Search terms: lower-case words for alphabetic scripts, character pairs
/// for Chinese and Japanese (which have no spaces), stop words dropped.
pub fn terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut cjk: Vec<char> = Vec::new();
    let flush_word = |word: &mut String, out: &mut Vec<String>| {
        if word.chars().count() >= 2 && !STOPWORDS.contains(&word.as_str()) {
            out.push(std::mem::take(word));
        }
        word.clear();
    };
    let flush_cjk = |cjk: &mut Vec<char>, out: &mut Vec<String>| {
        if cjk.len() == 1 {
            out.push(cjk[0].to_string());
        }
        for pair in cjk.windows(2) {
            out.push(pair.iter().collect());
        }
        cjk.clear();
    };
    for c in text.chars() {
        if is_cjk(c) {
            flush_word(&mut word, &mut out);
            cjk.push(c);
        } else if c.is_alphanumeric() {
            flush_cjk(&mut cjk, &mut out);
            word.extend(c.to_lowercase());
        } else {
            flush_word(&mut word, &mut out);
            flush_cjk(&mut cjk, &mut out);
        }
    }
    flush_word(&mut word, &mut out);
    flush_cjk(&mut cjk, &mut out);
    out
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(name: &str, text: &str) -> Material {
        Material {
            name: name.into(),
            path: name.into(),
            text: text.into(),
        }
    }

    #[test]
    fn terms_mix_words_and_character_pairs() {
        assert_eq!(
            terms("Tell me about the BGP rollout, 网络架构!"),
            ["bgp", "rollout", "网络", "络架", "架构"]
        );
        assert_eq!(terms("我"), ["我"]);
    }

    #[test]
    fn passages_keep_paragraphs_together() {
        let text = format!("short one\n\nshort two\n\n{}", "Sentence here. ".repeat(60));
        let parts = split_passages(&text, 200);
        assert!(parts[0].starts_with("short one\n\nshort two"));
        assert!(parts.iter().all(|p| p.chars().count() <= 200));
        assert!(parts.len() > 3);
    }

    #[test]
    fn finds_the_relevant_passage() {
        let resume = [
            "Led the migration of the payment service to Kubernetes, cutting deploy time from 40 to 6 minutes.",
            "Built a BGP EVPN fabric for three data centers; failover under one second.",
            "Mentored four junior engineers and ran the weekly design review.",
            "负责星环网络项目的容灾演练，RTO 从 30 分钟降到 5 分钟。",
        ]
        .join("\n\n");
        let kb = KnowledgeBase::build(&[m("cv.md", &resume)]);
        // Paragraphs are merged into one passage up to the target size.
        assert_eq!(kb.len(), 1);

        let long = resume
            .split("\n\n")
            .map(|p| format!("{p} {}", "filler ".repeat(80)))
            .collect::<Vec<_>>()
            .join("\n\n");
        let kb = KnowledgeBase::build(&[m("cv.md", &long)]);
        assert!(kb.len() >= 4);
        let hits = kb.search("How did you handle failover in the data center?", 5_000);
        assert!(hits[0].text.contains("BGP EVPN"), "{hits:?}");
        let hits = kb.search("讲讲你做过的容灾演练", 5_000);
        assert!(hits[0].text.contains("星环网络"), "{hits:?}");
        assert!(kb.search("what is your favourite colour", 5_000).is_empty());
        // The character budget is respected.
        assert!(kb.search("failover Kubernetes mentored", 700).len() == 1);
    }

    #[test]
    fn short_material_skips_retrieval() {
        let (summary, kb) = prepare(&[m("agenda.md", "SDN rollout")]);
        assert!(summary.contains("SDN rollout"));
        assert!(kb.is_empty());
        let long = "word ".repeat(2_000);
        let (summary, kb) = prepare(&[m("notes.md", &long)]);
        assert!(summary.chars().count() < OVERVIEW_CHARS + 50);
        assert!(!kb.is_empty());
    }

    #[test]
    fn render_names_sources() {
        let p = Passage {
            source: "cv.md".into(),
            text: "BGP".into(),
        };
        assert_eq!(render(&[&p, &p]), "[cv.md]\nBGP\n\n[cv.md]\nBGP");
    }
}
