//! Briefing material: text extraction (txt, md, docx, pdf), the condensed
//! excerpt placed in the hint prompt, and domain terms for recognition.
//!
//! Everything runs locally; only the excerpt is sent to the model.

use std::collections::HashMap;
use std::io::Read;

use meetcue_core::settings::Material;
use meetcue_core::{codes, MeetCueError};

use crate::textio::decode;

/// Characters of material placed in the system prompt.
pub const SUMMARY_CHARS: usize = 3_000;
/// Most recognizers accept a limited vocabulary list.
pub const MAX_HOTWORDS: usize = 50;

fn err(detail: impl Into<String>) -> MeetCueError {
    MeetCueError::new(codes::PRACTICE_FILE, detail)
}

/// Extracts plain text from a file's bytes, choosing the method by extension.
pub fn extract_text(file_name: &str, bytes: &[u8]) -> Result<String, MeetCueError> {
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let text = match ext.as_str() {
        "txt" | "md" | "markdown" | "csv" => decode(bytes),
        "docx" => docx_text(bytes)?,
        "pdf" => pdf_extract::extract_text_from_mem(bytes).map_err(|e| err(e.to_string()))?,
        _ => return Err(err(format!("unsupported file type .{ext}"))),
    };
    Ok(tidy(&text))
}

fn docx_text(bytes: &[u8]) -> Result<String, MeetCueError> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| err(e.to_string()))?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|e| err(e.to_string()))?
        .read_to_string(&mut xml)
        .map_err(|e| err(e.to_string()))?;
    Ok(xml_to_text(&xml))
}

/// Converts WordprocessingML to text: paragraphs become lines, tags are dropped.
fn xml_to_text(xml: &str) -> String {
    let xml = xml
        .replace("</w:p>", "\n")
        .replace("<w:tab/>", "\t")
        .replace("<w:br/>", "\n");
    let mut out = String::with_capacity(xml.len() / 3);
    let mut in_tag = false;
    for c in xml.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn tidy(text: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

/// The material excerpt for the system prompt: each document gets a share of
/// the budget, so one long résumé does not crowd out the agenda.
pub fn summary(materials: &[Material], max_chars: usize) -> String {
    if materials.is_empty() {
        return String::new();
    }
    let share = max_chars / materials.len();
    let mut out = String::new();
    for m in materials {
        out.push_str("## ");
        out.push_str(&m.name);
        out.push('\n');
        let text: String = m.text.chars().take(share).collect();
        out.push_str(text.trim());
        if m.text.chars().count() > share {
            out.push('…');
        }
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
}

/// Domain terms for recognition boosting: acronyms, CamelCase and
/// alphanumeric product names, and quoted or bracketed Chinese terms,
/// most frequent first.
pub fn hotwords(texts: &[&str], max: usize) -> Vec<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut add = |w: &str| {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric());
        if w.chars().count() < 2 || w.chars().count() > 24 {
            return;
        }
        let e = counts.entry(w.to_string()).or_insert(0);
        if *e == 0 {
            order.push(w.to_string());
        }
        *e += 1;
    };
    for text in texts {
        for word in text.split(|c: char| c.is_whitespace() || ",;:()（）,，。、；：\"'".contains(c))
        {
            let letters = word.chars().filter(|c| c.is_ascii_alphabetic()).count();
            if letters == 0 || !word.is_ascii() {
                continue;
            }
            let upper = word.chars().filter(|c| c.is_ascii_uppercase()).count();
            let has_digit = word.chars().any(|c| c.is_ascii_digit());
            let camel = word.chars().skip(1).any(|c| c.is_ascii_uppercase()) && upper < word.len();
            if (upper >= 2 && upper == letters) || camel || (has_digit && letters >= 2) {
                add(word);
            }
        }
        for (open, close) in [('《', '》'), ('“', '”'), ('「', '」')] {
            let mut rest = *text;
            while let Some(start) = rest.find(open) {
                let after = &rest[start + open.len_utf8()..];
                let Some(end) = after.find(close) else { break };
                add(&after[..end]);
                rest = &after[end + close.len_utf8()..];
            }
        }
    }
    let mut words = order;
    words.sort_by(|a, b| counts[b].cmp(&counts[a]));
    words.truncate(max);
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn extracts_plain_text_and_docx() {
        assert_eq!(
            extract_text("a.md", b"# Agenda\r\n\r\n\r\n- SDN").unwrap(),
            "# Agenda\n\n- SDN"
        );
        let mut buf = Vec::new();
        {
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            z.start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            z.write_all(br#"<w:document><w:body><w:p><w:r><w:t>Project &amp; goals</w:t></w:r></w:p><w:p><w:r><w:t>BGP</w:t></w:r><w:tab/><w:r><w:t>EVPN</w:t></w:r></w:p></w:body></w:document>"#).unwrap();
            z.finish().unwrap();
        }
        assert_eq!(
            extract_text("resume.docx", &buf).unwrap(),
            "Project & goals\nBGP\tEVPN"
        );
        assert!(extract_text("x.exe", b"").is_err());
        assert!(extract_text("broken.docx", b"not a zip").is_err());
    }

    #[test]
    fn summary_shares_budget() {
        let m = |name: &str, text: &str| Material {
            name: name.into(),
            path: name.into(),
            text: text.into(),
        };
        let s = summary(
            &[m("agenda.md", &"a".repeat(100)), m("cv.txt", "short")],
            40,
        );
        assert!(s.starts_with("## agenda.md\n"));
        assert!(s.contains(&format!("{}…", "a".repeat(20))));
        assert!(s.contains("## cv.txt\nshort"));
        assert_eq!(summary(&[], 100), "");
    }

    #[test]
    fn hotword_extraction() {
        let words = hotwords(
            &["我们用 OpenFlow 1.3 和 NETCONF，BGP EVPN 做 overlay。项目《星环网络》上线，BGP 收敛 <1s。iPhone 15 测试。"],
            50,
        );
        for w in ["OpenFlow", "NETCONF", "BGP", "EVPN", "星环网络", "iPhone"] {
            assert!(words.contains(&w.to_string()), "{w} in {words:?}");
        }
        assert_eq!(words[0], "BGP", "most frequent first");
        assert!(!words.contains(&"overlay".to_string()));
        assert!(hotwords(&["x".repeat(10).as_str()], 5).is_empty());
    }
}
