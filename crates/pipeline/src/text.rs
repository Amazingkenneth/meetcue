//! Language-aware text utilities used for matching without calling a model.
//!
//! Chinese text has no spaces, so it is compared by overlapping character
//! pairs (bigrams). English is compared by lowercase words with stop words
//! removed. Mixed text produces both kinds of units.

use std::collections::HashSet;

/// English words too common to signal that a point was covered.
const EN_STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "if", "then", "so", "to", "of", "in", "on", "at", "for",
    "with", "by", "from", "as", "is", "are", "was", "were", "be", "been", "being", "it", "its",
    "this", "that", "these", "those", "i", "we", "you", "he", "she", "they", "me", "us", "our",
    "my", "your", "their", "do", "does", "did", "have", "has", "had", "not", "no", "can", "will",
    "would", "should", "could", "there", "here", "what", "which", "who", "how", "why", "when",
    "about", "into", "than", "also", "just", "very", "all", "any", "some",
];

pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF   // Extension A
        | 0x4E00..=0x9FFF // Unified Ideographs
        | 0xF900..=0xFAFF // Compatibility Ideographs
        | 0x20000..=0x2A6DF)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() && !is_cjk(c)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    /// The language name used in prompts.
    pub fn prompt_name(self) -> &'static str {
        match self {
            Lang::Zh => "Simplified Chinese",
            Lang::En => "English",
        }
    }
}

/// The dominant language of a text: Chinese if CJK characters outnumber
/// Latin words (a Chinese sentence with a few English terms stays Chinese).
pub fn dominant_lang(text: &str) -> Lang {
    let cjk = text.chars().filter(|&c| is_cjk(c)).count();
    let words = latin_words(text).count();
    // One English word carries roughly as much as two Chinese characters.
    // Empty text defaults to Chinese.
    if (cjk > 0 && cjk >= words * 2) || (words == 0 && cjk == 0) {
        Lang::Zh
    } else {
        Lang::En
    }
}

fn latin_words(text: &str) -> impl Iterator<Item = String> + '_ {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if is_word_char(chars[i]) {
            let start = i;
            while i < chars.len()
                && (is_word_char(chars[i])
                    // keep "1.3", "v2.0" and "don't" together
                    || ((chars[i] == '.' || chars[i] == '\'')
                        && i + 1 < chars.len()
                        && is_word_char(chars[i + 1])
                        && i > start))
            {
                i += 1;
            }
            out.push(chars[start..i].iter().collect::<String>().to_lowercase());
        } else {
            i += 1;
        }
    }
    out.into_iter()
}

/// Lowercase Latin words, stop words included.
pub fn words(text: &str) -> Vec<String> {
    latin_words(text).collect()
}

/// Matching units of a text: CJK character bigrams (a lone character counts
/// as itself) plus lowercase Latin words that are not stop words.
pub fn units(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut run: Vec<char> = Vec::new();
    let flush = |run: &mut Vec<char>, out: &mut Vec<String>| {
        match run.len() {
            0 => {}
            1 => out.push(run[0].to_string()),
            _ => {
                for pair in run.windows(2) {
                    out.push(pair.iter().collect());
                }
            }
        }
        run.clear();
    };
    for c in text.chars() {
        if is_cjk(c) {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
        }
    }
    flush(&mut run, &mut out);
    out.extend(
        latin_words(text)
            .filter(|w| !EN_STOPWORDS.contains(&w.as_str()))
            .map(stem),
    );
    out
}

/// Drops a plural or tense ending so "regions", "deployed" and "deploying"
/// match "region" and "deploy". Short words and "-ss" words stay as they are.
fn stem(word: String) -> String {
    for suffix in ["ing", "ed", "s"] {
        if word.len() > suffix.len() + 3 && word.ends_with(suffix) && !word.ends_with("ss") {
            return word[..word.len() - suffix.len()].to_string();
        }
    }
    word
}

pub fn unit_set(text: &str) -> HashSet<String> {
    units(text).into_iter().collect()
}

/// Share of `a`'s units that also appear in `b` (0 when `a` has none).
pub fn containment(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() {
        return 0.0;
    }
    a.iter().filter(|u| b.contains(*u)).count() as f64 / a.len() as f64
}

/// Lowercase text with whitespace and punctuation removed, for substring checks.
pub fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Number of letters, digits and CJK characters (ignores spaces and punctuation).
pub fn meaningful_len(text: &str) -> usize {
    text.chars().filter(|c| c.is_alphanumeric()).count()
}

/// Splits a phrase into terms on whitespace and punctuation.
pub fn split_terms(text: &str) -> Vec<String> {
    text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                ',' | '，'
                    | '、'
                    | ';'
                    | '；'
                    | '/'
                    | '|'
                    | '·'
                    | '。'
                    | '：'
                    | ':'
                    | '('
                    | ')'
                    | '（'
                    | '）'
            )
    })
    .map(str::trim)
    .filter(|t| !t.is_empty())
    .map(String::from)
    .collect()
}

/// Character-level Levenshtein distance divided by the longer length.
pub fn edit_ratio(a: &str, b: &str) -> f64 {
    let a: Vec<char> = normalize(a).chars().collect();
    let b: Vec<char> = normalize(b).chars().collect();
    let longest = a.len().max(b.len());
    if longest == 0 {
        return 0.0;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()] as f64 / longest as f64
}

/// Truncates to at most `max` characters, adding an ellipsis when cut.
pub fn truncate_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        text.to_string()
    } else {
        let mut s: String = text.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

/// Visual width budget: one CJK character counts as one, while Latin text
/// counts one per two characters, so "≤14 characters" works for both scripts.
pub fn display_units(text: &str) -> f64 {
    text.chars()
        .map(|c| if is_cjk(c) { 1.0 } else { 0.5 })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_mix_scripts() {
        let u = units("用 BGP 做多路径");
        assert!(u.contains(&"bgp".to_string()));
        assert!(u.contains(&"多路".to_string()));
        assert!(u.contains(&"路径".to_string()));
        assert!(u.contains(&"用".to_string()));
        let u = units("The controller uses OpenFlow 1.3");
        assert_eq!(u, vec!["controller", "uses", "openflow", "1.3"]);
        let u = units("regions deployed deploying services access");
        assert_eq!(u, vec!["region", "deploy", "deploy", "service", "access"]);
    }

    #[test]
    fn language_detection() {
        assert_eq!(dominant_lang("你们的网络是怎么做高可用的？"), Lang::Zh);
        assert_eq!(dominant_lang("南向协议用的是 OpenFlow 吗"), Lang::Zh);
        assert_eq!(
            dominant_lang("How do you make the network highly available?"),
            Lang::En
        );
        assert_eq!(dominant_lang("What is SDN 控制器?"), Lang::En);
    }

    #[test]
    fn edit_distance_ratio() {
        assert_eq!(edit_ratio("abc", "abc"), 0.0);
        assert!((edit_ratio("控制器挂了还能转发吗", "控制器挂了还能转发吗？") - 0.0).abs() < 1e-9);
        assert!(edit_ratio("你们怎么做", "你们怎么做高可用的") > 0.3);
        assert!(edit_ratio("你们是怎么做高可用的", "你们是怎么做高可用") < 0.3);
    }

    #[test]
    fn terms_and_truncation() {
        assert_eq!(
            split_terms("主备 心跳，切换/BFD"),
            vec!["主备", "心跳", "切换", "BFD"]
        );
        assert_eq!(truncate_chars("一二三四五", 3), "一二…");
        assert_eq!(truncate_chars("一二三", 3), "一二三");
        assert_eq!(meaningful_len("对吧？"), 2);
    }
}
