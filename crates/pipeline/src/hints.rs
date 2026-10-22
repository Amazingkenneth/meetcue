//! Incremental parser for the hint stream (prompt B).
//!
//! Expected format: 3–5 JSON lines `{"n":1,"head":"…","rest":"…"}`, a line
//! `---`, then a sample answer. Models occasionally wrap the JSON in a code
//! fence, add blank lines, emit several objects on one line, put a trailing
//! comma after an object, split a line across chunks or forget the
//! separator. The parser accepts all of these.

use serde_json::Value;

use crate::text::truncate_chars;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HintItem {
    Point { n: u32, head: String, rest: String },
    Answer(String),
}

/// Upper bounds applied to model output so a runaway line cannot break the layout.
const MAX_HEAD_CHARS: usize = 24;
const MAX_REST_CHARS: usize = 60;
pub const MAX_POINTS: u32 = 6;

#[derive(Debug)]
pub struct HintStreamParser {
    line: String,
    in_answer: bool,
    points: u32,
    pending: String,
    chunk_chars: usize,
    answer_started: bool,
}

impl Default for HintStreamParser {
    fn default() -> Self {
        Self::new(30)
    }
}

impl HintStreamParser {
    /// `chunk_chars` controls how much answer text is batched per item.
    pub fn new(chunk_chars: usize) -> Self {
        Self {
            line: String::new(),
            in_answer: false,
            points: 0,
            pending: String::new(),
            chunk_chars,
            answer_started: false,
        }
    }

    pub fn points(&self) -> u32 {
        self.points
    }

    pub fn push(&mut self, text: &str) -> Vec<HintItem> {
        let mut out = Vec::new();
        for c in text.chars() {
            if c == '\n' {
                let line = std::mem::take(&mut self.line);
                self.process_line(&line, true, &mut out);
            } else {
                self.line.push(c);
            }
        }
        // Answer text is streamed without waiting for the end of the line.
        if self.in_answer && !self.line.is_empty() && !self.line.trim_start().starts_with("```") {
            let partial = std::mem::take(&mut self.line);
            self.append_answer(&partial, false);
        }
        self.drain_answer(false, &mut out);
        out
    }

    pub fn finish(&mut self) -> Vec<HintItem> {
        let mut out = Vec::new();
        let line = std::mem::take(&mut self.line);
        if !line.is_empty() {
            self.process_line(&line, false, &mut out);
        }
        self.drain_answer(true, &mut out);
        out
    }

    fn process_line(&mut self, raw: &str, newline: bool, out: &mut Vec<HintItem>) {
        let trimmed = raw.trim();
        if trimmed.starts_with("```") {
            return;
        }
        if self.in_answer {
            self.append_answer(raw, newline);
            self.drain_answer(false, out);
            return;
        }
        if trimmed.is_empty() {
            return;
        }
        if is_separator(trimmed) {
            self.in_answer = true;
            return;
        }
        let points = parse_points(trimmed);
        if !points.is_empty() {
            for (n, head, rest) in points {
                if self.points >= MAX_POINTS {
                    break;
                }
                self.points += 1;
                out.push(HintItem::Point {
                    n: n.unwrap_or(self.points),
                    head,
                    rest,
                });
            }
            return;
        }
        if trimmed.starts_with('{') || trimmed.starts_with('[') || trimmed == "]" {
            // Malformed JSON fragment: skip it rather than show it as answer text.
            return;
        }
        if self.points > 0 {
            // Separator missing: the first prose line starts the answer.
            self.in_answer = true;
            self.append_answer(raw, newline);
            self.drain_answer(false, out);
        }
    }

    fn append_answer(&mut self, text: &str, newline: bool) {
        let text = if !self.answer_started {
            text.trim_start()
        } else {
            text
        };
        if text.is_empty() && !newline {
            return;
        }
        if !text.is_empty() {
            self.answer_started = true;
        }
        self.pending.push_str(text);
        if newline && self.answer_started {
            self.pending.push('\n');
        }
    }

    fn drain_answer(&mut self, all: bool, out: &mut Vec<HintItem>) {
        let len = self.pending.chars().count();
        if len == 0 {
            return;
        }
        if all {
            let text = std::mem::take(&mut self.pending);
            let text = text.trim_end().to_string();
            if !text.is_empty() {
                out.push(HintItem::Answer(text));
            }
        } else if len >= self.chunk_chars {
            out.push(HintItem::Answer(std::mem::take(&mut self.pending)));
        }
    }
}

fn is_separator(line: &str) -> bool {
    line.len() >= 3
        && (line.chars().all(|c| c == '-')
            || line.chars().all(|c| c == '*')
            || line.chars().all(|c| c == '='))
}

/// Extracts every `{…}` object on a line that has a non-empty `head`.
fn parse_points(line: &str) -> Vec<(Option<u32>, String, String)> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
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
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start.take() {
                        if let Some(p) = parse_point(&line[s..=i]) {
                            out.push(p);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn parse_point(obj: &str) -> Option<(Option<u32>, String, String)> {
    let v: Value = serde_json::from_str(obj).ok()?;
    let head = v.get("head")?.as_str()?.trim();
    if head.is_empty() {
        return None;
    }
    let rest = v.get("rest").and_then(Value::as_str).unwrap_or("").trim();
    let n = v
        .get("n")
        .and_then(|n| {
            n.as_u64()
                .or_else(|| n.as_str().and_then(|s| s.parse().ok()))
        })
        .map(|n| n as u32);
    Some((
        n,
        truncate_chars(head, MAX_HEAD_CHARS),
        truncate_chars(rest, MAX_REST_CHARS),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(chunks: &[&str]) -> (Vec<(u32, String, String)>, String) {
        let mut p = HintStreamParser::new(30);
        let mut items = Vec::new();
        for c in chunks {
            items.extend(p.push(c));
        }
        items.extend(p.finish());
        let mut points = Vec::new();
        let mut answer = String::new();
        for i in items {
            match i {
                HintItem::Point { n, head, rest } => points.push((n, head, rest)),
                HintItem::Answer(t) => answer.push_str(&t),
            }
        }
        (points, answer)
    }

    const IDEAL: &str = "{\"n\":1,\"head\":\"双活\",\"rest\":\"两地 分担\"}\n{\"n\":2,\"head\":\"冗余\",\"rest\":\"堆叠\"}\n---\n我们采用**双活**架构，核心设备做**冗余**。";

    #[test]
    fn ideal_output() {
        let (points, answer) = run(&[IDEAL]);
        assert_eq!(points.len(), 2);
        assert_eq!(points[0], (1, "双活".into(), "两地 分担".into()));
        assert_eq!(answer, "我们采用**双活**架构，核心设备做**冗余**。");
    }

    #[test]
    fn arbitrary_chunk_boundaries() {
        let chars: Vec<char> = IDEAL.chars().collect();
        for size in [1, 2, 3, 7, 13] {
            let chunks: Vec<String> = chars.chunks(size).map(|c| c.iter().collect()).collect();
            let refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
            let (points, answer) = run(&refs);
            assert_eq!(points.len(), 2, "chunk size {size}");
            assert_eq!(
                answer, "我们采用**双活**架构，核心设备做**冗余**。",
                "chunk size {size}"
            );
        }
    }

    #[test]
    fn code_fences_blank_lines_and_trailing_commas() {
        let text = "```json\n\n{\"n\":1,\"head\":\"A\",\"rest\":\"x\"},\n\n{\"n\":2,\"head\":\"B\",\"rest\":\"y\"},\n```\n---\nAnswer text.";
        let (points, answer) = run(&[text]);
        assert_eq!(points.len(), 2);
        assert_eq!(answer, "Answer text.");
    }

    #[test]
    fn several_objects_per_line_and_array() {
        let text = "[{\"n\":1,\"head\":\"A\",\"rest\":\"x\"}, {\"n\":2,\"head\":\"B\"}]\n---\nok";
        let (points, answer) = run(&[text]);
        assert_eq!(points.len(), 2);
        assert_eq!(points[1], (2, "B".into(), String::new()));
        assert_eq!(answer, "ok");
    }

    #[test]
    fn missing_separator() {
        let text = "{\"n\":1,\"head\":\"A\",\"rest\":\"x\"}\n{\"n\":2,\"head\":\"B\",\"rest\":\"y\"}\nI would start with A, then B.";
        let (points, answer) = run(&[text]);
        assert_eq!(points.len(), 2);
        assert_eq!(answer, "I would start with A, then B.");
    }

    #[test]
    fn half_lines_and_garbage_are_skipped() {
        let text = "Sure! Here are the points:\n{\"n\":1,\"head\":\"A\",\"rest\":\"x\"}\n{\"n\":2,\"head\":\n{\"n\":3,\"head\":\"C\",\"rest\":\"z\"}\n---\nfine";
        let (points, answer) = run(&[text]);
        assert_eq!(
            points.iter().map(|p| p.1.as_str()).collect::<Vec<_>>(),
            ["A", "C"]
        );
        assert_eq!(answer, "fine");
    }

    #[test]
    fn missing_numbers_and_string_numbers() {
        let text = "{\"head\":\"A\"}\n{\"n\":\"7\",\"head\":\"B\"}\n---\n";
        let (points, answer) = run(&[text]);
        assert_eq!(points[0].0, 1);
        assert_eq!(points[1].0, 7);
        assert!(answer.is_empty());
    }

    #[test]
    fn answer_is_streamed_in_chunks() {
        let mut p = HintStreamParser::new(10);
        let mut answers = Vec::new();
        for item in p.push("{\"n\":1,\"head\":\"A\"}\n---\n") {
            assert!(matches!(item, HintItem::Point { .. }));
        }
        for c in ["一二三四五", "六七八九十", "十一", "十二"] {
            for item in p.push(c) {
                if let HintItem::Answer(t) = item {
                    answers.push(t);
                }
            }
        }
        for item in p.finish() {
            if let HintItem::Answer(t) = item {
                answers.push(t);
            }
        }
        assert_eq!(answers, ["一二三四五六七八九十", "十一十二"]);
    }

    #[test]
    fn multiline_answer_keeps_line_breaks() {
        let (_, answer) = run(&["{\"head\":\"A\"}\n---\nline one\nline two\n"]);
        assert_eq!(answer, "line one\nline two");
    }

    #[test]
    fn point_count_is_capped() {
        let text: String = (1..=9)
            .map(|i| format!("{{\"n\":{i},\"head\":\"P{i}\"}}\n"))
            .collect();
        let (points, _) = run(&[&text]);
        assert_eq!(points.len(), MAX_POINTS as usize);
    }
}
