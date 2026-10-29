//! Knowledge base files: text extraction from common office formats, the
//! condensed overview placed in the hint prompt, and domain terms for
//! recognition. Retrieval of relevant passages is in [`crate::knowledge`].
//!
//! Everything runs locally; only excerpts are sent to the model.

use std::collections::HashMap;
use std::io::Read;

use meetcue_core::settings::Material;
use meetcue_core::{codes, MeetCueError};

use crate::textio::decode;

/// Characters of material placed in the system prompt.
pub const SUMMARY_CHARS: usize = 3_000;
/// Most recognizers accept a limited vocabulary list.
pub const MAX_HOTWORDS: usize = 50;
/// Text kept per file; the rest is cut so settings stay a manageable size.
pub const MAX_TEXT_CHARS: usize = 300_000;

/// File extensions [`extract_text`] reads, for the file picker.
pub const EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "csv", "tsv", "json", "yaml", "yml", "log", "html", "htm", "rtf",
    "pdf", "docx", "docm", "xlsx", "xlsm", "pptx", "pptm", "odt", "ods", "odp",
];

fn err(detail: impl Into<String>) -> MeetCueError {
    MeetCueError::new(codes::KNOWLEDGE_FILE, detail)
}

/// Extracts plain text from a file's bytes, choosing the method by extension.
pub fn extract_text(file_name: &str, bytes: &[u8]) -> Result<String, MeetCueError> {
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let text = match ext.as_str() {
        "txt" | "md" | "markdown" | "csv" | "tsv" | "json" | "yaml" | "yml" | "log" => {
            decode(bytes)
        }
        "html" | "htm" => html_text(&decode(bytes)),
        "rtf" => rtf_text(bytes),
        "pdf" => pdf_extract::extract_text_from_mem(bytes).map_err(|e| err(e.to_string()))?,
        "docx" | "docm" => docx_text(bytes)?,
        "xlsx" | "xlsm" => xlsx_text(bytes)?,
        "pptx" | "pptm" => pptx_text(bytes)?,
        "odt" | "ods" | "odp" => odf_text(bytes)?,
        "doc" | "xls" | "ppt" => {
            return Err(err(format!(
                "the old binary format .{ext} is not supported; save the file as .{ext}x and add it again"
            )))
        }
        _ => return Err(err(format!("unsupported file type .{ext}"))),
    };
    let text = tidy(&text);
    Ok(match text.char_indices().nth(MAX_TEXT_CHARS) {
        Some((cut, _)) => text[..cut].to_string(),
        None => text,
    })
}

type Zip<'a> = zip::ZipArchive<std::io::Cursor<&'a [u8]>>;

fn open_zip(bytes: &[u8]) -> Result<Zip<'_>, MeetCueError> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| err(e.to_string()))
}

fn zip_entry(archive: &mut Zip<'_>, name: &str) -> Result<String, MeetCueError> {
    let mut xml = String::new();
    archive
        .by_name(name)
        .map_err(|e| err(format!("{name}: {e}")))?
        .read_to_string(&mut xml)
        .map_err(|e| err(e.to_string()))?;
    Ok(xml)
}

/// Entries named `{prefix}{n}.xml`, in numeric order (slide2 before slide10).
fn numbered_entries(archive: &Zip<'_>, prefix: &str) -> Vec<(u32, String)> {
    let mut found: Vec<(u32, String)> = archive
        .file_names()
        .filter_map(|n| {
            let num = n.strip_prefix(prefix)?.strip_suffix(".xml")?.parse().ok()?;
            Some((num, n.to_string()))
        })
        .collect();
    found.sort();
    found
}

fn docx_text(bytes: &[u8]) -> Result<String, MeetCueError> {
    let xml = zip_entry(&mut open_zip(bytes)?, "word/document.xml")?;
    Ok(xml_to_text(&xml))
}

/// Converts WordprocessingML to text: paragraphs become lines, tags are dropped.
fn xml_to_text(xml: &str) -> String {
    markup_text(
        xml,
        &[
            ("/w:p", "\n"),
            ("w:tab", "\t"),
            ("w:br", "\n"),
            ("/w:tc", "\t"),
        ],
    )
}

fn pptx_text(bytes: &[u8]) -> Result<String, MeetCueError> {
    let mut archive = open_zip(bytes)?;
    let slides = numbered_entries(&archive, "ppt/slides/slide");
    let notes: HashMap<u32, String> = numbered_entries(&archive, "ppt/notesSlides/notesSlide")
        .into_iter()
        .collect();
    let rules = [("/a:p", "\n"), ("a:br", "\n"), ("/a:tc", "\t")];
    let mut out = String::new();
    for (n, name) in slides {
        out.push_str(&format!("# Slide {n}\n"));
        out.push_str(&markup_text(&zip_entry(&mut archive, &name)?, &rules));
        // Notes slides are numbered on their own, but almost always match.
        if let Some(note) = notes.get(&n) {
            let text = markup_text(&zip_entry(&mut archive, note)?, &rules);
            // Notes pages repeat the slide number as their only other text.
            let text = text.trim();
            if !text.is_empty() && text.parse::<u32>().is_err() {
                out.push_str("Notes: ");
                out.push_str(text);
                out.push('\n');
            }
        }
        out.push('\n');
    }
    Ok(out)
}

fn xlsx_text(bytes: &[u8]) -> Result<String, MeetCueError> {
    let mut archive = open_zip(bytes)?;
    let shared: Vec<String> = match zip_entry(&mut archive, "xl/sharedStrings.xml") {
        Ok(xml) => {
            let mut items = Vec::new();
            let mut cur: Option<String> = None;
            let mut in_text = false;
            let mut phonetic = false;
            for tok in tokens(&xml) {
                match tok {
                    Tok::Tag(t) if t.name == "si" && !t.closing => cur = Some(String::new()),
                    Tok::Tag(t) if t.name == "si" => items.push(cur.take().unwrap_or_default()),
                    // Phonetic guides repeat the reading of East Asian text.
                    Tok::Tag(t) if t.name == "rPh" => phonetic = !t.closing && !t.empty,
                    Tok::Tag(t) if t.name == "t" => in_text = !t.closing && !t.empty && !phonetic,
                    Tok::Text(s) if in_text => {
                        if let Some(c) = cur.as_mut() {
                            c.push_str(&unescape(s));
                        }
                    }
                    _ => {}
                }
            }
            items
        }
        Err(_) => Vec::new(),
    };
    let names: Vec<String> = zip_entry(&mut archive, "xl/workbook.xml")
        .map(|xml| {
            tokens(&xml)
                .filter_map(|t| match t {
                    Tok::Tag(t) if t.name == "sheet" && !t.closing => attr(t.attrs, "name"),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let mut out = String::new();
    for (i, (n, entry)) in numbered_entries(&archive, "xl/worksheets/sheet")
        .into_iter()
        .enumerate()
    {
        let title = names
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("Sheet {n}"));
        out.push_str(&format!("# {title}\n"));
        let xml = zip_entry(&mut archive, &entry)?;
        let mut kind = String::new();
        let mut value = String::new();
        let mut in_value = false;
        let mut row = Vec::new();
        for tok in tokens(&xml) {
            match tok {
                Tok::Tag(t) if t.name == "c" && !t.closing => {
                    kind = attr(t.attrs, "t").unwrap_or_default();
                    value.clear();
                }
                Tok::Tag(t) if t.name == "v" || t.name == "t" => in_value = !t.closing && !t.empty,
                Tok::Text(s) if in_value => value.push_str(&unescape(s)),
                Tok::Tag(t) if t.name == "c" && t.closing => {
                    let cell = match kind.as_str() {
                        "s" => value
                            .trim()
                            .parse::<usize>()
                            .ok()
                            .and_then(|i| shared.get(i).cloned())
                            .unwrap_or_default(),
                        _ => value.clone(),
                    };
                    row.push(cell.replace(['\t', '\n'], " "));
                }
                Tok::Tag(t) if t.name == "row" && t.closing => {
                    while row.last().is_some_and(|c: &String| c.trim().is_empty()) {
                        row.pop();
                    }
                    if !row.is_empty() {
                        out.push_str(&row.join("\t"));
                        out.push('\n');
                    }
                    row.clear();
                }
                _ => {}
            }
        }
        out.push('\n');
    }
    Ok(out)
}

/// OpenDocument text, spreadsheets and presentations share `content.xml`.
fn odf_text(bytes: &[u8]) -> Result<String, MeetCueError> {
    let xml = zip_entry(&mut open_zip(bytes)?, "content.xml")?;
    Ok(markup_text(
        &xml,
        &[
            ("/text:p", "\n"),
            ("/text:h", "\n"),
            ("text:tab", "\t"),
            ("text:line-break", "\n"),
            ("text:s", " "),
            ("/table:table-cell", "\t"),
            ("/table:table-row", "\n"),
            ("/draw:page", "\n"),
        ],
    ))
}

fn html_text(html: &str) -> String {
    let mut out = String::new();
    let mut skip: Option<String> = None;
    for tok in tokens(html) {
        match tok {
            Tok::Tag(t) => {
                let name = t.name.to_ascii_lowercase();
                if let Some(s) = &skip {
                    if t.closing && name == *s {
                        skip = None;
                    }
                    continue;
                }
                match name.as_str() {
                    "script" | "style" | "head" | "noscript" if !t.closing && !t.empty => {
                        skip = Some(name)
                    }
                    "td" | "th" if t.closing => out.push('\t'),
                    "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                    | "section" | "article" | "table" | "ul" | "ol" | "pre" | "blockquote" => {
                        out.push('\n')
                    }
                    _ => {}
                }
            }
            Tok::Text(s) if skip.is_none() => {
                // Source line breaks are layout, not content.
                out.push_str(&unescape(&s.replace('\n', " ")))
            }
            Tok::Text(_) => {}
        }
    }
    out.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Plain text of an RTF document: control words dropped, `\par` as line
/// breaks, `\uN` and `\'hh` escapes decoded with the document's code page.
fn rtf_text(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut encoding = encoding_rs::WINDOWS_1252;
    // Group depth at which an ignored destination (font table, pictures) began.
    let mut skip_from: Option<usize> = None;
    let mut depth = 0usize;
    let mut uc = 1usize;
    let mut fallback = 0usize;
    let mut i = 0;
    let flush = |pending: &mut Vec<u8>, out: &mut String, enc: &'static encoding_rs::Encoding| {
        if !pending.is_empty() {
            out.push_str(&enc.decode_without_bom_handling(pending).0);
            pending.clear();
        }
    };
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'{' => {
                depth += 1;
                i += 1;
                if bytes[i..].starts_with(b"\\*") && skip_from.is_none() {
                    skip_from = Some(depth);
                }
            }
            b'}' => {
                if skip_from == Some(depth) {
                    skip_from = None;
                }
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'\\' => {
                i += 1;
                let Some(&c) = bytes.get(i) else { break };
                if c == b'\'' {
                    let hex = bytes
                        .get(i + 1..i + 3)
                        .and_then(|h| std::str::from_utf8(h).ok());
                    i += 3;
                    if fallback > 0 {
                        fallback -= 1;
                    } else if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                        if skip_from.is_none() {
                            pending.push(v);
                        }
                    }
                    continue;
                }
                if !c.is_ascii_alphabetic() {
                    // Escaped symbol: \\, \{, \}, \~ (non-breaking space).
                    i += 1;
                    if skip_from.is_none() {
                        flush(&mut pending, &mut out, encoding);
                        match c {
                            b'~' => out.push(' '),
                            b'\\' | b'{' | b'}' => out.push(c as char),
                            _ => {}
                        }
                    }
                    continue;
                }
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let word = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
                let num_start = i;
                if i < bytes.len() && (bytes[i] == b'-' || bytes[i].is_ascii_digit()) {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let num: Option<i32> = std::str::from_utf8(&bytes[num_start..i])
                    .ok()
                    .and_then(|n| n.parse().ok());
                if bytes.get(i) == Some(&b' ') {
                    i += 1;
                }
                match word {
                    "fonttbl" | "colortbl" | "stylesheet" | "info" | "pict" | "header"
                    | "footer" | "listtable" | "listoverridetable" => {
                        if skip_from.is_none() {
                            skip_from = Some(depth);
                        }
                    }
                    "ansicpg" => {
                        encoding = match num {
                            Some(936) => encoding_rs::GBK,
                            Some(950) => encoding_rs::BIG5,
                            Some(932) => encoding_rs::SHIFT_JIS,
                            Some(949) => encoding_rs::EUC_KR,
                            Some(n) => {
                                encoding_rs::Encoding::for_label(format!("windows-{n}").as_bytes())
                                    .unwrap_or(encoding_rs::WINDOWS_1252)
                            }
                            None => encoding,
                        }
                    }
                    "uc" => uc = num.unwrap_or(1).max(0) as usize,
                    _ if skip_from.is_some() => {}
                    "u" => {
                        flush(&mut pending, &mut out, encoding);
                        let n = num.unwrap_or(0);
                        let n = if n < 0 { n + 65_536 } else { n } as u32;
                        out.extend(char::from_u32(n));
                        fallback = uc;
                    }
                    "par" | "line" | "row" | "sect" | "page" => {
                        flush(&mut pending, &mut out, encoding);
                        out.push('\n');
                    }
                    "tab" | "cell" => {
                        flush(&mut pending, &mut out, encoding);
                        out.push('\t');
                    }
                    _ => {}
                }
            }
            b'\r' | b'\n' => i += 1,
            _ => {
                i += 1;
                if fallback > 0 {
                    fallback -= 1;
                } else if skip_from.is_none() {
                    pending.push(b);
                }
            }
        }
    }
    flush(&mut pending, &mut out, encoding);
    out
}

/// A piece of markup: a tag or the text between tags.
enum Tok<'a> {
    Tag(TagTok<'a>),
    Text(&'a str),
}

struct TagTok<'a> {
    name: &'a str,
    attrs: &'a str,
    closing: bool,
    empty: bool,
}

/// Splits XML or HTML into tags and text. Comments, declarations and
/// processing instructions are dropped.
fn tokens(xml: &str) -> impl Iterator<Item = Tok<'_>> {
    let mut rest = xml;
    std::iter::from_fn(move || loop {
        if rest.is_empty() {
            return None;
        }
        let Some(lt) = rest.find('<') else {
            let text = rest;
            rest = "";
            return Some(Tok::Text(text));
        };
        if lt > 0 {
            let text = &rest[..lt];
            rest = &rest[lt..];
            return Some(Tok::Text(text));
        }
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |e| &after[e + 3..]);
            continue;
        }
        let Some(gt) = rest.find('>') else {
            rest = "";
            return None;
        };
        let inner = &rest[1..gt];
        rest = &rest[gt + 1..];
        if inner.starts_with('?') || inner.starts_with('!') {
            continue;
        }
        let closing = inner.starts_with('/');
        let inner = inner.trim_start_matches('/');
        let empty = inner.ends_with('/');
        let inner = inner.trim_end_matches('/');
        let (name, attrs) = inner
            .split_once(|c: char| c.is_whitespace())
            .unwrap_or((inner, ""));
        return Some(Tok::Tag(TagTok {
            name,
            attrs,
            closing,
            empty,
        }));
    })
}

/// An attribute value from a tag's attribute text.
fn attr(attrs: &str, name: &str) -> Option<String> {
    let mut rest = attrs;
    while let Some(pos) = rest.find(name) {
        let before_ok = pos == 0 || rest[..pos].ends_with(char::is_whitespace);
        let after = &rest[pos + name.len()..];
        if before_ok {
            if let Some(v) = after.trim_start().strip_prefix('=') {
                let v = v.trim_start();
                let q = v.chars().next()?;
                if q == '"' || q == '\'' {
                    let v = &v[1..];
                    return v.find(q).map(|e| unescape(&v[..e]));
                }
            }
        }
        rest = after;
    }
    None
}

/// Text of markup with the given tags replaced by separators (`/name` for
/// a closing tag, `name` for an opening or empty one); other tags dropped.
fn markup_text(xml: &str, breaks: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(xml.len() / 3);
    for tok in tokens(xml) {
        match tok {
            Tok::Text(s) => out.push_str(&unescape(s)),
            Tok::Tag(t) => {
                let found = breaks.iter().find(|(tag, _)| match tag.strip_prefix('/') {
                    Some(name) => t.closing && name == t.name,
                    None => !t.closing && *tag == t.name,
                });
                if let Some((_, sep)) = found {
                    out.push_str(sep);
                }
            }
        }
    }
    out
}

/// Decodes the XML and common HTML character references.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "amp" => Some('&'),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
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

    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            for (name, body) in files {
                z.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        buf
    }

    #[test]
    fn extracts_spreadsheets() {
        let xlsx = zip_of(&[
            (
                "xl/workbook.xml",
                r#"<workbook><sheets><sheet name="Projects" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/sharedStrings.xml",
                r#"<sst><si><t>Project</t></si><si><r><t>Kube</t></r><r><t xml:space="preserve">rnetes &amp; CI</t></r></si><si><t>星环</t><rPh><t>ほし</t></rPh></si></sst>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="inlineStr"><is><t>Year</t></is></c></row><row r="2"><c r="A2" t="s"><v>1</v></c><c r="B2"><v>2024</v></c></row><row r="3"><c r="A3" t="s"><v>2</v></c><c r="B3"/></row></sheetData></worksheet>"#,
            ),
        ]);
        assert_eq!(
            extract_text("cv.xlsx", &xlsx).unwrap(),
            "# Projects\nProject\tYear\nKubernetes & CI\t2024\n星环"
        );
        let ods = zip_of(&[(
            "content.xml",
            r#"<office:document-content><table:table><table:table-row><table:table-cell><text:p>BGP</text:p></table:table-cell><table:table-cell><text:p>EVPN</text:p></table:table-cell></table:table-row></table:table></office:document-content>"#,
        )]);
        assert_eq!(extract_text("a.ods", &ods).unwrap(), "BGP\n\tEVPN");
    }

    #[test]
    fn extracts_slides_with_notes() {
        let slide = |t: &str| {
            format!(
                r#"<p:sld><p:txBody><a:p><a:r><a:t>{t}</a:t></a:r></a:p><a:p><a:r><a:t>second line</a:t></a:r></a:p></p:txBody></p:sld>"#
            )
        };
        let (s1, s2, s10) = (slide("Intro"), slide("Results"), slide("End"));
        let pptx = zip_of(&[
            ("ppt/slides/slide10.xml", &s10),
            ("ppt/slides/slide1.xml", &s1),
            ("ppt/slides/slide2.xml", &s2),
            (
                "ppt/notesSlides/notesSlide2.xml",
                "<p:notes><a:p><a:r><a:t>Mention the 40% gain</a:t></a:r></a:p><a:p><a:r><a:t>2</a:t></a:r></a:p></p:notes>",
            ),
            (
                "ppt/notesSlides/notesSlide1.xml",
                "<p:notes><a:p><a:r><a:t>1</a:t></a:r></a:p></p:notes>",
            ),
        ]);
        let text = extract_text("talk.pptx", &pptx).unwrap();
        assert!(text.starts_with("# Slide 1\nIntro\nsecond line\n\n# Slide 2\nResults"));
        assert!(text.contains("Notes: Mention the 40% gain\n2"));
        assert!(text.ends_with("# Slide 10\nEnd\nsecond line"));
        assert!(!text.contains("Notes: 1"));
    }

    #[test]
    fn extracts_documents() {
        let odt = zip_of(&[(
            "content.xml",
            r#"<office:text><text:h>Experience</text:h><text:p>Led<text:s/>the<text:tab/>rollout</text:p></office:text>"#,
        )]);
        assert_eq!(
            extract_text("cv.odt", &odt).unwrap(),
            "Experience\nLed the\trollout"
        );
        let html = "<html><head><title>x</title><style>p{}</style></head><body><h1>Me</h1><p>Five\n years &amp; <b>BGP</b>&nbsp;work</p><script>alert(1)</script><ul><li>One</li><li>Two</li></ul></body></html>";
        assert_eq!(
            extract_text("cv.html", html.as_bytes()).unwrap(),
            "Me\n\nFive years & BGP work\n\nOne\n\nTwo"
        );
        let rtf = br"{\rtf1\ansi\ansicpg936{\fonttbl{\f0 SimSun;}}{\*\generator Word;}\f0 Hello\par \'d6\'d0\'ce\'c4 and \u26143?\u29615?\tab done\}\par}";
        assert_eq!(
            extract_text("cv.rtf", rtf).unwrap(),
            "Hello\n中文 and 星环\tdone}"
        );
        let err = extract_text("old.doc", b"").unwrap_err();
        assert_eq!(err.code, codes::KNOWLEDGE_FILE);
        assert!(err.detail.contains(".docx"));
        let long = "a".repeat(MAX_TEXT_CHARS + 10);
        assert_eq!(
            extract_text("long.txt", long.as_bytes()).unwrap().len(),
            MAX_TEXT_CHARS
        );
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
