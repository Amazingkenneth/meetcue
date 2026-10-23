//! Importing practice files: subtitles (`.srt`, `.vtt`, `.txt`) and question
//! lists (`.txt`, `.md`, `.csv`).
//!
//! Parsing is deliberately lenient: real files mix encodings, line endings,
//! timestamp styles and formatting tags.

use meetcue_core::{codes, MeetCueError};

use super::{PracticeItem, PracticeKind, PracticeScript};
use crate::textio;

/// One subtitle cue before speaker turns are merged.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub speaker: Option<String>,
    pub text: String,
}

/// Consecutive cues of one speaker closer than this become one turn.
const MERGE_GAP_MS: i64 = 2_000;

fn prc_err(detail: impl Into<String>) -> MeetCueError {
    MeetCueError::new(codes::PRACTICE_FILE, detail)
}

/// Parses a practice file. The kind follows from the extension; `.txt` is
/// treated as subtitles when most lines carry a timestamp or speaker name.
pub fn parse_file(name: &str, bytes: &[u8]) -> Result<PracticeScript, MeetCueError> {
    let text = textio::decode(bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let ext = name
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let title = file_stem(name);
    let script = match ext.as_str() {
        "srt" => subtitles(title, parse_srt(&text)),
        "vtt" => subtitles(title, parse_vtt(&text)),
        "csv" => questions(title, parse_csv(&text)?),
        "md" | "markdown" => questions(title, parse_question_lines(&text)),
        "txt" | "text" => {
            if looks_like_subtitles(&text) {
                subtitles(title, parse_text_subtitles(&text))
            } else {
                questions(title, parse_question_lines(&text))
            }
        }
        other => return Err(prc_err(format!("unsupported file type .{other}"))),
    };
    if script.items.is_empty() {
        return Err(prc_err("no questions or subtitle lines found"));
    }
    Ok(script)
}

fn file_stem(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    match base.rfind('.') {
        Some(i) if i > 0 => base[..i].to_string(),
        _ => base.to_string(),
    }
}

/// Parses `HH:MM:SS,mmm`, `HH:MM:SS.mmm`, `MM:SS.mmm`, `HH:MM:SS` or `MM:SS`.
pub fn parse_timestamp(s: &str) -> Option<i64> {
    let s = s.trim();
    let (clock, frac) = match s.find([',', '.']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut nums = Vec::with_capacity(3);
    for p in &parts {
        if p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        nums.push(p.parse::<i64>().ok()?);
    }
    let (h, m, sec) = match nums.as_slice() {
        [m, s] => (0, *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    if m >= 60 && parts.len() == 3 || sec >= 60 {
        return None;
    }
    let ms = match frac {
        None => 0,
        Some(f) => {
            if f.is_empty() || f.len() > 3 || !f.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            // "5" means 500 ms, "05" 50 ms.
            f.parse::<i64>().ok()? * 10i64.pow(3 - f.len() as u32)
        }
    };
    Some(((h * 60 + m) * 60 + sec) * 1000 + ms)
}

/// Parses a `start --> end [settings]` timing line.
fn parse_timing(line: &str) -> Option<(i64, i64)> {
    let (a, b) = line.split_once("-->")?;
    let start = parse_timestamp(a)?;
    let end = parse_timestamp(b.split_whitespace().next()?)?;
    Some((start, end))
}

/// Removes formatting tags (`<i>`, `</b>`, `{\an8}`) and returns the voice
/// name of a VTT `<v Name>` tag, if any.
fn clean_markup(text: &str) -> (String, Option<String>) {
    let mut out = String::with_capacity(text.len());
    let mut voice = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                let mut tag = String::new();
                for t in chars.by_ref() {
                    if t == '>' {
                        break;
                    }
                    tag.push(t);
                }
                // <v Name>, <v.loud Name>
                if let Some(rest) = tag.strip_prefix('v') {
                    if rest.starts_with([' ', '.']) {
                        let name = rest.split_once(' ').map(|(_, n)| n.trim()).unwrap_or("");
                        if !name.is_empty() && voice.is_none() {
                            voice = Some(name.to_string());
                        }
                    }
                }
            }
            '{' if chars.peek() == Some(&'\\') => {
                for t in chars.by_ref() {
                    if t == '}' {
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    let out = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    (collapse_ws(&out), voice)
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Splits `Name: text` (ASCII or full-width colon). The name must be short
/// and free of sentence punctuation so ordinary sentences with a colon and
/// clock times are not mistaken for speakers.
pub fn split_speaker(line: &str) -> Option<(String, String)> {
    let i = line.find([':', '：'])?;
    let name = line[..i].trim();
    let colon_len = line[i..].chars().next()?.len_utf8();
    let rest = line[i + colon_len..].trim();
    if name.is_empty() || rest.is_empty() {
        return None;
    }
    if name.chars().count() > 24 || name.split_whitespace().count() > 4 {
        return None;
    }
    if name
        .chars()
        .any(|c| ".,;!?。，；！？、\"“”()（）[]".contains(c))
    {
        return None;
    }
    if name
        .chars()
        .all(|c| c.is_ascii_digit() || c.is_whitespace())
    {
        return None;
    }
    if rest.starts_with("//") {
        return None; // a URL
    }
    if name.ends_with(|c: char| c.is_ascii_digit())
        && rest.starts_with(|c: char| c.is_ascii_digit())
    {
        return None; // a clock time such as 10:30
    }
    Some((name.to_string(), rest.to_string()))
}

fn cue_from_lines(lines: &[&str], start: Option<i64>, end: Option<i64>) -> Option<Cue> {
    let (text, voice) = clean_markup(&lines.join(" "));
    if text.is_empty() {
        return None;
    }
    let (speaker, text) = match voice {
        Some(v) => (Some(v), text),
        None => match split_speaker(&text) {
            Some((n, t)) => (Some(n), t),
            None => (None, text),
        },
    };
    Some(Cue {
        start_ms: start,
        end_ms: end,
        speaker,
        text,
    })
}

fn blocks(text: &str) -> Vec<Vec<&str>> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(line.trim());
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn parse_srt(text: &str) -> Vec<Cue> {
    blocks(text)
        .into_iter()
        .filter_map(|b| {
            let t = b.iter().position(|l| l.contains("-->"))?;
            let (start, end) = parse_timing(b[t])?;
            cue_from_lines(&b[t + 1..], Some(start), Some(end))
        })
        .collect()
}

pub fn parse_vtt(text: &str) -> Vec<Cue> {
    blocks(text)
        .into_iter()
        .filter(|b| {
            let first = b[0];
            !(first.starts_with("WEBVTT")
                || first.starts_with("NOTE")
                || first.starts_with("STYLE")
                || first.starts_with("REGION"))
                || b.iter().any(|l| l.contains("-->"))
        })
        .filter_map(|b| {
            let t = b.iter().position(|l| l.contains("-->"))?;
            let (start, end) = parse_timing(b[t])?;
            cue_from_lines(&b[t + 1..], Some(start), Some(end))
        })
        .collect()
}

/// A leading `[00:01]`, `(00:01)` or bare `00:01` timestamp.
fn split_leading_time(line: &str) -> (Option<i64>, &str) {
    let l = line.trim_start();
    for (open, close) in [('[', ']'), ('(', ')')] {
        if let Some(rest) = l.strip_prefix(open) {
            if let Some(i) = rest.find(close) {
                if let Some(ts) = parse_timestamp(&rest[..i]) {
                    return (Some(ts), rest[i + 1..].trim_start());
                }
            }
        }
    }
    if let Some((first, rest)) = l.split_once(char::is_whitespace) {
        if let Some(ts) = parse_timestamp(first) {
            return (Some(ts), rest.trim_start());
        }
    }
    (None, l)
}

pub fn parse_text_subtitles(text: &str) -> Vec<Cue> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let (ts, rest) = split_leading_time(l);
            cue_from_lines(&[rest], ts, None)
        })
        .collect()
}

fn looks_like_subtitles(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return false;
    }
    let mut timed = 0;
    let mut speakers = std::collections::HashSet::new();
    let mut with_speaker = 0;
    for l in &lines {
        let (ts, rest) = split_leading_time(l);
        if ts.is_some() {
            timed += 1;
        }
        if let Some((name, _)) = split_speaker(rest) {
            with_speaker += 1;
            speakers.insert(name);
        }
    }
    timed * 2 >= lines.len() || (with_speaker * 2 >= lines.len() && speakers.len() >= 2)
}

/// Strips Markdown list markers and emphasis from a question line.
fn strip_list_marker(line: &str) -> &str {
    let l = line.trim();
    for m in ["- [ ] ", "- [x] ", "- ", "* ", "+ "] {
        if let Some(r) = l.strip_prefix(m) {
            return r.trim();
        }
    }
    let digits = l.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && digits < l.len() {
        let rest = &l[digits..];
        for m in [". ", ") ", "、", "．"] {
            if let Some(r) = rest.strip_prefix(m) {
                return r.trim();
            }
        }
    }
    l
}

pub fn parse_question_lines(text: &str) -> Vec<PracticeItem> {
    let mut items = Vec::new();
    let mut category: Option<String> = None;
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.chars().all(|c| matches!(c, '-' | '=' | '*' | '_')) {
            continue;
        }
        if let Some(h) = l.strip_prefix('#') {
            let h = h.trim_start_matches('#').trim();
            category = (!h.is_empty()).then(|| h.to_string());
            continue;
        }
        let q = strip_list_marker(l).replace("**", "");
        let q = collapse_ws(&q);
        if q.is_empty() {
            continue;
        }
        items.push(question_item(q, None, category.clone(), Vec::new()));
    }
    items
}

fn question_item(
    text: String,
    reference: Option<String>,
    category: Option<String>,
    followups: Vec<String>,
) -> PracticeItem {
    PracticeItem {
        id: 0,
        text,
        speaker: None,
        start_ms: None,
        end_ms: None,
        is_me: false,
        is_question: true,
        reference,
        category,
        followups,
    }
}

fn column(headers: &[String], names: &[&str]) -> Option<usize> {
    headers.iter().position(|h| names.contains(&h.as_str()))
}

/// CSV with a `question` column and optional `reference_answer`, `category`
/// and `followups` (separated by `|`). Without a recognised header row, the
/// first column of every row is a question.
pub fn parse_csv(text: &str) -> Result<Vec<PracticeItem>, MeetCueError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());
    let mut rows = Vec::new();
    for r in reader.records() {
        let r = r.map_err(|e| prc_err(format!("CSV: {e}")))?;
        rows.push(r.iter().map(str::to_string).collect::<Vec<_>>());
    }
    let Some(first) = rows.first() else {
        return Ok(Vec::new());
    };
    let headers: Vec<String> = first
        .iter()
        .map(|h| h.trim().to_lowercase().replace([' ', '-'], "_"))
        .collect();
    let q_col = column(&headers, &["question", "questions", "问题", "题目"]);
    let (q, reference, category, followups, body) = match q_col {
        Some(q) => (
            q,
            column(
                &headers,
                &[
                    "reference_answer",
                    "reference",
                    "answer",
                    "参考回答",
                    "参考答案",
                ],
            ),
            column(&headers, &["category", "group", "分类", "分组"]),
            column(&headers, &["followups", "follow_ups", "followup", "追问"]),
            &rows[1..],
        ),
        None => (0, None, None, None, &rows[..]),
    };
    let get = |row: &Vec<String>, c: Option<usize>| {
        c.and_then(|c| row.get(c))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    Ok(body
        .iter()
        .filter_map(|row| {
            let text = get(row, Some(q))?;
            let followups = get(row, followups)
                .map(|f| {
                    f.split('|')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            Some(question_item(
                text,
                get(row, reference),
                get(row, category),
                followups,
            ))
        })
        .collect())
}

fn number(items: &mut [PracticeItem]) {
    for (i, it) in items.iter_mut().enumerate() {
        it.id = i as u32 + 1;
    }
}

fn questions(title: String, mut items: Vec<PracticeItem>) -> PracticeScript {
    number(&mut items);
    PracticeScript {
        kind: PracticeKind::Questions,
        title,
        items,
        speakers: Vec::new(),
        me_speaker: None,
        needs_classification: false,
    }
}

fn ends_with_question(text: &str) -> bool {
    let t = text.trim_end();
    t.ends_with('?') || t.ends_with('？') || t.ends_with('吗') || t.ends_with('呢')
}

/// Merges consecutive cues of the same speaker into turns.
pub fn merge_turns(cues: Vec<Cue>) -> Vec<Cue> {
    let mut out: Vec<Cue> = Vec::new();
    for c in cues {
        if let Some(last) = out.last_mut() {
            let same = c.speaker.is_some() && c.speaker == last.speaker;
            let close = match (last.end_ms.or(last.start_ms), c.start_ms) {
                (Some(a), Some(b)) => b - a <= MERGE_GAP_MS,
                _ => true,
            };
            if same && close {
                let joiner = if last.text.ends_with(|ch: char| ch.is_ascii())
                    && c.text.starts_with(|ch: char| ch.is_ascii())
                {
                    " "
                } else {
                    ""
                };
                last.text = format!("{}{joiner}{}", last.text, c.text);
                last.end_ms = c.end_ms.or(c.start_ms).or(last.end_ms);
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn subtitles(title: String, cues: Vec<Cue>) -> PracticeScript {
    let cues = merge_turns(cues);
    let mut speakers: Vec<String> = Vec::new();
    for c in &cues {
        if let Some(s) = &c.speaker {
            if !speakers.contains(s) {
                speakers.push(s.clone());
            }
        }
    }
    let mut items: Vec<PracticeItem> = cues
        .into_iter()
        .map(|c| PracticeItem {
            id: 0,
            is_question: ends_with_question(&c.text),
            text: c.text,
            speaker: c.speaker,
            start_ms: c.start_ms,
            end_ms: c.end_ms,
            is_me: false,
            reference: None,
            category: None,
            followups: Vec::new(),
        })
        .collect();
    number(&mut items);
    let needs_classification = speakers.is_empty();
    let mut script = PracticeScript {
        kind: PracticeKind::Subtitles,
        title,
        items,
        speakers,
        me_speaker: None,
        needs_classification,
    };
    if let Some(me) = guess_me(&script) {
        set_me(&mut script, Some(&me));
    }
    script
}

/// Guesses which speaker is the user: the one who asks the smallest share of
/// questions (interviewers ask, candidates answer).
pub fn guess_me(script: &PracticeScript) -> Option<String> {
    if script.speakers.len() < 2 {
        return None;
    }
    script
        .speakers
        .iter()
        .map(|s| {
            let lines: Vec<&PracticeItem> = script
                .items
                .iter()
                .filter(|i| i.speaker.as_deref() == Some(s))
                .collect();
            let q = lines.iter().filter(|i| ends_with_question(&i.text)).count();
            (q as f64 / lines.len().max(1) as f64, s.clone())
        })
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, s)| s)
}

/// Marks the user's lines. Their text becomes the reference answer for the
/// review and is not played.
pub fn set_me(script: &mut PracticeScript, speaker: Option<&str>) {
    script.me_speaker = speaker.map(str::to_string);
    for it in &mut script.items {
        it.is_me = speaker.is_some() && it.speaker.as_deref() == speaker;
    }
}

/// Applies model labels for subtitles without speakers: questions are played,
/// answers are the user's turns.
pub fn apply_classification(script: &mut PracticeScript, questions: &[u32]) {
    for it in &mut script.items {
        it.is_question = questions.contains(&it.id);
        it.is_me = !it.is_question;
    }
    script.needs_classification = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        assert_eq!(parse_timestamp("00:00:01,200"), Some(1_200));
        assert_eq!(parse_timestamp("00:00:01.200"), Some(1_200));
        assert_eq!(parse_timestamp("01:02:03.5"), Some(3_723_500));
        assert_eq!(parse_timestamp("02:03.040"), Some(123_040));
        assert_eq!(parse_timestamp("1:02:03"), Some(3_723_000));
        assert_eq!(parse_timestamp("00:05"), Some(5_000));
        assert_eq!(parse_timestamp("ab:cd"), None);
        assert_eq!(parse_timestamp("5"), None);
        assert_eq!(parse_timestamp("00:61"), None);
    }

    #[test]
    fn srt_with_speakers_crlf_and_tags() {
        let srt =
            "1\r\n00:00:01,200 --> 00:00:04,800\r\n面试官：先请你<i>简单</i>介绍一下自己？\r\n\r\n\
                   2\r\n00:00:05,500 --> 00:00:08,000\r\n小林：好的。\r\n\r\n\
                   3\r\n00:00:08,100 --> 00:00:12,000\r\n小林：我做数据中心网络八年了。\r\n";
        let s = parse_file("interview.srt", srt.as_bytes()).unwrap();
        assert_eq!(s.kind, PracticeKind::Subtitles);
        assert_eq!(s.title, "interview");
        assert_eq!(s.speakers, vec!["面试官", "小林"]);
        assert_eq!(s.items.len(), 2, "two cues of 小林 merge into one turn");
        assert_eq!(s.items[0].text, "先请你简单介绍一下自己？");
        assert_eq!(s.items[1].text, "好的。我做数据中心网络八年了。");
        assert_eq!(s.items[1].start_ms, Some(5_500));
        assert_eq!(s.items[1].end_ms, Some(12_000));
        assert_eq!(s.me_speaker.as_deref(), Some("小林"));
        assert!(s.items[1].is_me && !s.items[0].is_me);
        assert!(!s.needs_classification);
    }

    #[test]
    fn srt_with_dot_millis_and_no_index() {
        let srt = "00:00:01.000 --> 00:00:02.000\nHello there\n\n00:00:03.000 --> 00:00:04.000\nHow are you?\n";
        let cues = parse_srt(srt);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[1].start_ms, Some(3_000));
        assert_eq!(cues[0].speaker, None);
    }

    #[test]
    fn vtt_voices_short_timestamps_and_settings() {
        let vtt = "WEBVTT\n\nNOTE written by hand\n\n1\n00:01.200 --> 00:04.800 align:start\n<v Interviewer>Could you introduce yourself?</v>\n\n00:05.500 --> 00:12.000\n<v.loud Alex>Sure. I work on networks.\n";
        let s = parse_file("a.vtt", vtt.as_bytes()).unwrap();
        assert_eq!(s.speakers, vec!["Interviewer", "Alex"]);
        assert_eq!(s.items[0].start_ms, Some(1_200));
        assert_eq!(s.items[0].text, "Could you introduce yourself?");
        assert_eq!(s.me_speaker.as_deref(), Some("Alex"));
    }

    #[test]
    fn text_subtitles_detected_and_parsed() {
        let txt = "[00:01] Interviewer: Could you introduce yourself?\n[00:05] Alex: Sure.\n(00:09) Interviewer: Why networking?\n";
        let s = parse_file("talk.txt", txt.as_bytes()).unwrap();
        assert_eq!(s.kind, PracticeKind::Subtitles);
        assert_eq!(s.items.len(), 3);
        assert_eq!(s.items[2].start_ms, Some(9_000));
        assert_eq!(s.items[2].speaker.as_deref(), Some("Interviewer"));

        let plain = "Hello everyone.\nWhat do you do?\nI build networks.\n";
        let s = parse_file("plain.txt", plain.as_bytes()).unwrap();
        assert_eq!(s.kind, PracticeKind::Questions, "no timestamps or speakers");
    }

    #[test]
    fn subtitles_without_speakers_need_classification() {
        let srt = "1\n00:00:01,000 --> 00:00:02,000\n你好，请介绍一下自己？\n\n2\n00:00:03,000 --> 00:00:05,000\n我是网络工程师。\n";
        let mut s = parse_file("x.srt", srt.as_bytes()).unwrap();
        assert!(s.needs_classification);
        assert_eq!(s.items.len(), 2, "no speakers: lines are not merged");
        apply_classification(&mut s, &[1]);
        assert!(s.items[0].is_question && !s.items[0].is_me);
        assert!(s.items[1].is_me && !s.needs_classification);
    }

    #[test]
    fn speaker_prefix_rules() {
        assert_eq!(
            split_speaker("Alex: hi"),
            Some(("Alex".into(), "hi".into()))
        );
        assert_eq!(
            split_speaker("面试官：你好"),
            Some(("面试官".into(), "你好".into()))
        );
        assert_eq!(split_speaker("We met at 10:30 today."), None);
        assert_eq!(split_speaker("See https://example.com"), None);
        assert_eq!(split_speaker("Well, here is the thing: it fails"), None);
        assert_eq!(split_speaker("12: twelve"), None);
    }

    #[test]
    fn question_lines_with_groups_and_markers() {
        let md = "# Basics\n- What is the control plane?\n* How does **BGP** choose a path?\n\n---\n## Design\n1. Design a network for two data centers.\n2) Why?\n";
        let s = parse_file("q.md", md.as_bytes()).unwrap();
        assert_eq!(s.kind, PracticeKind::Questions);
        let texts: Vec<&str> = s.items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "What is the control plane?",
                "How does BGP choose a path?",
                "Design a network for two data centers.",
                "Why?"
            ]
        );
        assert_eq!(s.items[0].category.as_deref(), Some("Basics"));
        assert_eq!(s.items[2].category.as_deref(), Some("Design"));
        assert_eq!(s.items[3].id, 4);
    }

    #[test]
    fn csv_with_header_quotes_and_followups() {
        let csv = "question,reference_answer,category,followups\n\"What is SDN?\",\"Control and data planes, separated.\",Basics,\"Which controllers?|Risks?\"\nWhat is VXLAN?,,,\n";
        let s = parse_file("q.csv", csv.as_bytes()).unwrap();
        assert_eq!(s.items.len(), 2);
        assert_eq!(
            s.items[0].reference.as_deref(),
            Some("Control and data planes, separated.")
        );
        assert_eq!(s.items[0].followups, vec!["Which controllers?", "Risks?"]);
        assert_eq!(s.items[1].reference, None);
        assert_eq!(s.items[1].category, None);
    }

    #[test]
    fn csv_without_header_and_with_chinese_header() {
        let s = parse_file("q.csv", "什么是 SDN？\n什么是 BGP？\n".as_bytes()).unwrap();
        assert_eq!(s.items.len(), 2);
        let s = parse_file(
            "q.csv",
            "问题,参考回答\n什么是 SDN？,控制与转发分离\n".as_bytes(),
        )
        .unwrap();
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.items[0].reference.as_deref(), Some("控制与转发分离"));
    }

    #[test]
    fn gbk_and_bom_files() {
        let (gbk, _, _) =
            encoding_rs::GBK.encode("面试官：为什么选择网络方向？\n小林：因为喜欢。\n");
        let s = parse_file("a.txt", &gbk).unwrap();
        assert_eq!(s.items[0].text, "为什么选择网络方向？");
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice("question\nWhat is SDN?\n".as_bytes());
        let s = parse_file("b.csv", &bom).unwrap();
        assert_eq!(s.items[0].text, "What is SDN?");
    }

    #[test]
    fn errors() {
        assert_eq!(
            parse_file("a.docx", b"x").unwrap_err().code,
            codes::PRACTICE_FILE
        );
        assert!(parse_file("a.srt", b"").is_err());
    }
}
