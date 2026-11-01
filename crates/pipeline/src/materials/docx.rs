//! Word documents: body text with tables kept row by row, text boxes,
//! charts, SmartArt, picture descriptions and footnotes.
//!
//! Only the text of `w:t` runs is taken, so layout numbers in drawings
//! (positions, sizes) and field codes never end up in the knowledge base.

use meetcue_core::MeetCueError;

use super::{attr, entry_name, err, open_zip, tokens, unescape, zip_entry, Tok, Zip};

const REL_DOCUMENT: &str = "/officeDocument";

/// Text of a .docx or .docm package.
pub(super) fn text(bytes: &[u8]) -> Result<String, MeetCueError> {
    let mut archive = open_zip(bytes)?;
    let main = main_part(&mut archive).ok_or_else(|| {
        err("there is no Word document inside this file. It may be damaged or another kind of file renamed to .docx; open it in Word and save it again as .docx")
    })?;
    let xml = zip_entry(&mut archive, &main)?;
    let rels = relationships(&mut archive, &main);
    let mut out = {
        let mut embed = |kind: Embed, id: &str| {
            let path = rels.get(id)?;
            let xml = zip_entry(&mut archive, path).ok()?;
            Some(match kind {
                Embed::Chart => chart_text(&xml),
                Embed::Diagram => diagram_text(&xml),
            })
        };
        body_text(&xml, &mut embed)
    };
    for (kind, heading) in [("footnotes", "Footnotes"), ("endnotes", "Endnotes")] {
        let Some(path) = rels.of_type(kind) else {
            continue;
        };
        let Ok(xml) = zip_entry(&mut archive, &path) else {
            continue;
        };
        let notes = body_text(&xml, &mut |_, _| None);
        if !notes.trim().is_empty() {
            out.push_str(&format!("\n{heading}:\n{}\n", notes.trim()));
        }
    }
    Ok(out)
}

/// Text of Word's single-file XML formats, which hold every part inline.
pub(super) fn flat_text(xml: &str) -> String {
    body_text(xml, &mut |_, _| None)
}

/// Embedded parts a document refers to by relationship id.
#[derive(Clone, Copy)]
enum Embed {
    Chart,
    Diagram,
}

/// The main document part. Usually `word/document.xml`, but some tools name
/// it differently (`word/document2.xml`), so the package root says.
fn main_part(archive: &mut Zip<'_>) -> Option<String> {
    let from_rels = zip_entry(archive, "_rels/.rels").ok().and_then(|xml| {
        parse_rels(&xml, "")
            .into_iter()
            .find(|r| r.kind.ends_with(REL_DOCUMENT))
            .map(|r| r.target)
    });
    from_rels
        .and_then(|p| entry_name(archive, &p))
        .or_else(|| entry_name(archive, "word/document.xml"))
}

struct Rel {
    id: String,
    kind: String,
    target: String,
}

#[derive(Default)]
struct Rels(Vec<Rel>);

impl Rels {
    fn get(&self, id: &str) -> Option<&String> {
        self.0.iter().find(|r| r.id == id).map(|r| &r.target)
    }

    fn of_type(&self, kind: &str) -> Option<String> {
        let suffix = format!("/{kind}");
        self.0
            .iter()
            .find(|r| r.kind.ends_with(&suffix))
            .map(|r| r.target.clone())
    }
}

/// Relationships of a part, with targets resolved to package paths.
fn relationships(archive: &mut Zip<'_>, part: &str) -> Rels {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let rels_path = if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    };
    zip_entry(archive, &rels_path)
        .map(|xml| Rels(parse_rels(&xml, dir)))
        .unwrap_or_default()
}

fn parse_rels(xml: &str, dir: &str) -> Vec<Rel> {
    tokens(xml)
        .filter_map(|t| match t {
            Tok::Tag(t) if t.name == "Relationship" && !t.closing => {
                if attr(t.attrs, "TargetMode").is_some_and(|m| m == "External") {
                    return None;
                }
                Some(Rel {
                    id: attr(t.attrs, "Id")?,
                    kind: attr(t.attrs, "Type")?,
                    target: resolve(dir, &attr(t.attrs, "Target")?),
                })
            }
            _ => None,
        })
        .collect()
}

/// A relationship target relative to `dir`, as a path inside the package.
fn resolve(dir: &str, target: &str) -> String {
    let target = target.replace('\\', "/");
    let mut parts: Vec<&str> = match target.strip_prefix('/') {
        Some(_) => Vec::new(),
        None => dir.split('/').filter(|s| !s.is_empty()).collect(),
    };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Word's own placeholder alt text says nothing about the picture.
fn is_generated_alt(text: &str) -> bool {
    text.contains("automatically generated") || text.contains("自动生成")
}

/// Text of WordprocessingML: one line per paragraph, one line per table row
/// with cells separated by " | ", and embedded charts and diagrams in place.
fn body_text(xml: &str, embed: &mut dyn FnMut(Embed, &str) -> Option<String>) -> String {
    // bufs[0] is the output; each open table cell adds a buffer.
    let mut bufs = vec![String::new()];
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut in_text = false;
    let mut in_props = 0usize;
    // Depth inside content that repeats or removes text: the fallback copy
    // of text boxes and drawings, and text moved elsewhere.
    let mut skip = 0usize;
    for tok in tokens(xml) {
        let Tok::Tag(t) = tok else {
            if let Tok::Text(s) = tok {
                if in_text && skip == 0 {
                    push(&mut bufs, &unescape(s));
                }
            }
            continue;
        };
        if matches!(t.name, "mc:Fallback" | "w:moveFrom") && !t.empty {
            if t.closing {
                skip = skip.saturating_sub(1);
            } else {
                skip += 1;
            }
            continue;
        }
        if skip > 0 {
            continue;
        }
        match (t.name, t.closing) {
            ("w:t", _) => in_text = !t.closing && !t.empty,
            ("w:pPr" | "w:rPr" | "w:sectPr" | "w:tblPr" | "w:trPr" | "w:tcPr", _) if !t.empty => {
                if t.closing {
                    in_props = in_props.saturating_sub(1);
                } else {
                    in_props += 1;
                }
            }
            // Inside properties, w:tab defines a tab stop rather than a tab.
            _ if in_props > 0 => {}
            ("w:tab" | "w:ptab", false) => push(&mut bufs, "\t"),
            ("w:br" | "w:cr", false) => push(&mut bufs, "\n"),
            ("w:noBreakHyphen", false) => push(&mut bufs, "-"),
            ("w:p", true) => push(&mut bufs, "\n"),
            ("w:tr", false) if !t.empty => rows.push(Vec::new()),
            ("w:tc", false) if !t.empty => bufs.push(String::new()),
            ("w:tc", true) if bufs.len() > 1 => {
                let cell = bufs.pop().unwrap_or_default();
                let cell = cell.split_whitespace().collect::<Vec<_>>().join(" ");
                if let Some(row) = rows.last_mut() {
                    row.push(cell);
                }
            }
            ("w:tr", true) => {
                let mut row = rows.pop().unwrap_or_default();
                while row.last().is_some_and(String::is_empty) {
                    row.pop();
                }
                if row.iter().any(|c| !c.is_empty()) {
                    push(&mut bufs, &format!("{}\n", row.join(" | ")));
                }
            }
            ("w:tbl", true) => push(&mut bufs, "\n"),
            ("c:chart", false) => {
                let text = attr(t.attrs, "r:id").and_then(|id| embed(Embed::Chart, &id));
                push_block(&mut bufs, text);
            }
            ("dgm:relIds", false) => {
                let text = attr(t.attrs, "r:dm").and_then(|id| embed(Embed::Diagram, &id));
                push_block(&mut bufs, text);
            }
            ("wp:docPr", false) => {
                if let Some(alt) = attr(t.attrs, "descr") {
                    let alt = alt.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !alt.is_empty() && !is_generated_alt(&alt) {
                        push(&mut bufs, &format!("\n[Picture: {alt}]\n"));
                    }
                }
            }
            _ => {}
        }
    }
    // Unclosed cells in a truncated file: keep their text.
    let rest: Vec<String> = bufs.drain(1..).collect();
    let mut out = bufs.pop().unwrap_or_default();
    for cell in rest {
        out.push_str(&cell);
    }
    out
}

/// Puts an embedded part's text on lines of its own.
fn push_block(bufs: &mut [String], text: Option<String>) {
    if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
        push(bufs, &format!("\n{text}\n"));
    }
}

fn push(bufs: &mut [String], s: &str) {
    if let Some(b) = bufs.last_mut() {
        b.push_str(s);
    }
}

/// A chart's title, series names, categories and values, so questions about
/// the numbers in a graph can be answered from the knowledge base.
fn chart_text(xml: &str) -> String {
    struct Series {
        name: String,
        cats: Vec<String>,
        vals: Vec<String>,
    }
    let mut stack: Vec<&str> = Vec::new();
    let mut titles: Vec<String> = Vec::new();
    let mut series: Vec<Series> = Vec::new();
    let mut cur: Option<Series> = None;
    for tok in tokens(xml) {
        match tok {
            Tok::Tag(t) if t.closing => {
                if let Some(pos) = stack.iter().rposition(|n| *n == t.name) {
                    stack.truncate(pos);
                }
                if t.name == "c:ser" {
                    series.extend(cur.take());
                }
            }
            Tok::Tag(t) if !t.empty => {
                if t.name == "c:ser" {
                    cur = Some(Series {
                        name: String::new(),
                        cats: Vec::new(),
                        vals: Vec::new(),
                    });
                }
                stack.push(t.name);
            }
            Tok::Tag(_) => {}
            Tok::Text(s) => {
                let text = unescape(s);
                let text = text.trim();
                let leaf = stack.last().copied().unwrap_or("");
                if text.is_empty() || !matches!(leaf, "c:v" | "a:t") {
                    continue;
                }
                match (stack.iter().position(|n| *n == "c:ser"), cur.as_mut()) {
                    (Some(at), Some(ser)) => match stack.get(at + 1).copied() {
                        Some("c:tx") => {
                            if !ser.name.is_empty() {
                                ser.name.push(' ');
                            }
                            ser.name.push_str(text);
                        }
                        Some("c:cat" | "c:xVal") => ser.cats.push(text.to_string()),
                        Some("c:val" | "c:yVal") => ser.vals.push(text.to_string()),
                        _ => {}
                    },
                    _ if stack.contains(&"c:title") && leaf == "a:t" => {
                        titles.push(text.to_string())
                    }
                    _ => {}
                }
            }
        }
    }
    if titles.is_empty() && series.is_empty() {
        return String::new();
    }
    let mut out = match titles.is_empty() {
        true => "Chart".to_string(),
        false => format!("Chart: {}", titles.join(" / ")),
    };
    for (i, s) in series.iter().enumerate() {
        let name = match s.name.is_empty() {
            true => format!("Series {}", i + 1),
            false => s.name.clone(),
        };
        let points: Vec<String> = if s.cats.len() == s.vals.len() {
            s.cats
                .iter()
                .zip(&s.vals)
                .map(|(c, v)| format!("{c} {v}"))
                .collect()
        } else {
            s.vals.clone()
        };
        out.push_str(&format!("\n{name}: {}", points.join(", ")));
    }
    out
}

/// The text of a SmartArt diagram, one line per shape.
fn diagram_text(xml: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut in_text = false;
    for tok in tokens(xml) {
        match tok {
            Tok::Tag(t) if t.name == "a:t" => in_text = !t.closing && !t.empty,
            Tok::Tag(t) if t.name == "a:p" && t.closing => {
                let done = std::mem::take(&mut line);
                let done = done.trim();
                if !done.is_empty() {
                    lines.push(done.to_string());
                }
            }
            Tok::Text(s) if in_text => line.push_str(&unescape(s)),
            _ => {}
        }
    }
    match lines.is_empty() {
        true => String::new(),
        false => format!("Diagram:\n{}", lines.join("\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::extract_text;
    use std::io::Write;

    fn package(files: &[(&str, &str)]) -> Vec<u8> {
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

    const RELS: &str = r#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="/word/document2.xml"/></Relationships>"#;

    /// Shaped like Word's output: a field code, a tracked deletion, a
    /// floating text box with its fallback copy, a table, a chart, SmartArt,
    /// pictures with real and generated alt text, and a footnote.
    const BODY: &str = r#"<?xml version="1.0" encoding="UTF-8"?><w:document><w:body>
<w:p><w:pPr><w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs></w:pPr><w:r><w:instrText> TOC \o "1-3" </w:instrText></w:r><w:r><w:t>Quarterly review</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">We grew </w:t></w:r><w:del><w:r><w:delText>slowly</w:delText></w:r></w:del><w:r><w:t>fast.</w:t></w:r></w:p>
<w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wp:anchor><wp:positionH><wp:posOffset>1905000</wp:posOffset></wp:positionH><wp:docPr id="3" name="Text Box 3"/><wps:txbx><w:txbxContent><w:p><w:r><w:t>Key risk: churn</w:t></w:r></w:p></w:txbxContent></wps:txbx></wp:anchor></w:drawing></mc:Choice><mc:Fallback><w:pict><v:textbox><w:txbxContent><w:p><w:r><w:t>Key risk: churn</w:t></w:r></w:p></w:txbxContent></v:textbox></w:pict></mc:Fallback></mc:AlternateContent></w:r></w:p>
<w:tbl><w:tblPr><w:tblW w:w="0"/></w:tblPr><w:tr><w:tc><w:tcPr><w:tcW w:w="4675"/></w:tcPr><w:p><w:r><w:t>Region</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Revenue</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>EMEA</w:t></w:r></w:p><w:p><w:r><w:t>(incl. UK)</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>$1.2M</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>
<w:p><w:r><w:drawing><wp:inline><wp:docPr id="1" name="Chart 1"/><c:chart r:id="rId5"/></wp:inline></w:drawing></w:r></w:p>
<w:p><w:r><w:drawing><wp:inline><wp:docPr id="2" descr="Org chart: Ana leads &amp; Bo reports > Ana"/></wp:inline></w:drawing></w:r></w:p>
<w:p><w:r><w:drawing><wp:inline><wp:docPr id="4" descr="A screenshot of a computer&#xA;&#xA;Description automatically generated"/></wp:inline></w:drawing></w:r></w:p>
<w:p><w:r><w:drawing><wp:inline><dgm:relIds r:dm="rId6" r:lo="rId7"/></wp:inline></w:drawing></w:r></w:p>
<w:sectPr><w:pgSz w:w="12240"/></w:sectPr></w:body></w:document>"#;

    const DOC_RELS: &str = r#"<Relationships><Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="charts/chart1.xml"/><Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/diagramData" Target="../word/diagrams/data1.xml"/><Relationship Id="rId8" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.test" TargetMode="External"/></Relationships>"#;

    const CHART: &str = r#"<c:chartSpace><c:chart><c:title><c:tx><c:rich><a:p><a:r><a:t>Revenue by quarter</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:barChart><c:ser><c:tx><c:strRef><c:f>Sheet1!$B$1</c:f><c:strCache><c:pt idx="0"><c:v>2025</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:f>Sheet1!$A$2:$A$3</c:f><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt><c:pt idx="1"><c:v>Q2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:formatCode>General</c:formatCode><c:pt idx="0"><c:v>4.3</c:v></c:pt><c:pt idx="1"><c:v>2.5</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;

    const DIAGRAM: &str = r#"<dgm:dataModel><dgm:ptLst><dgm:pt type="doc"><dgm:t><a:p><a:endParaRPr/></a:p></dgm:t></dgm:pt><dgm:pt><dgm:t><a:p><a:r><a:t>Plan</a:t></a:r></a:p></dgm:t></dgm:pt><dgm:pt><dgm:t><a:p><a:r><a:t>Build</a:t></a:r></a:p></dgm:t></dgm:pt></dgm:ptLst></dgm:dataModel>"#;

    const NOTES: &str = r#"<w:footnotes><w:footnote w:type="separator"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:t>Unaudited figures.</w:t></w:r></w:p></w:footnote></w:footnotes>"#;

    #[test]
    fn reads_word_documents_with_tables_charts_and_drawings() {
        let docx = package(&[
            ("_rels/.rels", RELS),
            ("word/document2.xml", BODY),
            ("word/_rels/document2.xml.rels", DOC_RELS),
            ("word/charts/chart1.xml", CHART),
            ("word/diagrams/data1.xml", DIAGRAM),
            ("word/footnotes.xml", NOTES),
            ("word/media/image1.png", "png"),
        ]);
        assert_eq!(
            extract_text("report.docx", &docx).unwrap(),
            "Quarterly review\nWe grew fast.\nKey risk: churn\n\n\
             Region | Revenue\nEMEA (incl. UK) | $1.2M\n\n\
             Chart: Revenue by quarter\n2025: Q1 4.3, Q2 2.5\n\n\
             [Picture: Org chart: Ana leads & Bo reports > Ana]\n\n\
             Diagram:\nPlan\nBuild\n\n\
             Footnotes:\nUnaudited figures."
        );
    }

    #[test]
    fn explains_word_files_it_cannot_read() {
        let mut encrypted = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0];
        encrypted.extend("EncryptedPackage".encode_utf16().flat_map(u16::to_le_bytes));
        let e = extract_text("secret.docx", &encrypted).unwrap_err();
        assert!(e.detail.contains("password-protected"), "{}", e.detail);

        let old = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0];
        let e = extract_text("renamed.docx", &old).unwrap_err();
        assert!(e.detail.contains("older .doc file"), "{}", e.detail);

        let e = extract_text("half.docx", b"PK\x03\x04 cut short").unwrap_err();
        assert!(e.detail.contains("damaged or incomplete"), "{}", e.detail);

        let e = extract_text("notes.docx", b"hello").unwrap_err();
        assert!(e.detail.contains("not a valid .docx"), "{}", e.detail);

        let pictures = package(&[(
            "word/document.xml",
            r#"<w:document><w:body><w:p><w:r><w:drawing><wp:anchor><wp:posOffset>1905000</wp:posOffset><wp:docPr id="1" name="Picture 1"/></wp:anchor></w:drawing></w:r></w:p></w:body></w:document>"#,
        )]);
        let e = extract_text("scan.docx", &pictures).unwrap_err();
        assert!(e.detail.contains("no text was found"), "{}", e.detail);

        let e = extract_text("other.docx", &package(&[("mimetype", "x")])).unwrap_err();
        assert!(e.detail.contains("no Word document inside"), "{}", e.detail);
    }

    #[test]
    fn reads_word_files_saved_in_related_formats() {
        assert_eq!(
            extract_text("rtf.docx", br"{\rtf1\ansi Hello\par}").unwrap(),
            "Hello"
        );
        let flat = r#"<?xml version="1.0"?><pkg:package><pkg:part><pkg:xmlData><w:document><w:body><w:p><w:r><w:t>Flat</w:t></w:r></w:p></w:body></w:document></pkg:xmlData></pkg:part></pkg:package>"#;
        assert_eq!(extract_text("flat.docx", flat.as_bytes()).unwrap(), "Flat");
        // Backslashes and odd case in part names, as some tools write them.
        let odd = package(&[(
            "Word\\Document.xml",
            "<w:document><w:body><w:p><w:r><w:t>Odd</w:t></w:r></w:p></w:body></w:document>",
        )]);
        assert_eq!(extract_text("odd.docx", &odd).unwrap(), "Odd");
    }
}
