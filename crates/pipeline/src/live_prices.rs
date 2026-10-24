//! Prices read live from each provider's official pricing page.
//!
//! Anthropic publishes its pricing page as Markdown; Soniox and Deepgram
//! render their prices into the page HTML. Each parser returns only figures
//! it could read unambiguously and in a plausible range, so a page redesign
//! makes a provider fall back to the published list instead of showing a
//! wrong number.

use futures::future::join_all;
use meetcue_core::{codes, MeetCueError, PriceEntry, PriceUnit};

use crate::net;

pub const ANTHROPIC_PAGE: &str = "https://platform.claude.com/docs/en/about-claude/pricing";
const ANTHROPIC_MARKDOWN: &str = "https://platform.claude.com/docs/en/about-claude/pricing.md";
pub const SONIOX_PAGE: &str = "https://soniox.com/pricing";
pub const DEEPGRAM_PAGE: &str = "https://deepgram.com/pricing";

/// Reads one provider's page; `(page, today)` → prices found.
type Parser = fn(&str, &str) -> Vec<PriceEntry>;

/// Fetches every provider's page and returns the prices that could be read,
/// plus one message per provider that failed.
pub async fn fetch_all(proxy: Option<&str>, today: &str) -> (Vec<PriceEntry>, Vec<String>) {
    let client = match net::http_client(proxy) {
        Ok(c) => c,
        Err(e) => return (Vec::new(), vec![e.to_string()]),
    };
    let get = |url: &'static str| {
        let client = client.clone();
        async move {
            let resp = client
                .get(url)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
                .map_err(|e| net::reqwest_error(&e))?;
            if !resp.status().is_success() {
                return Err(MeetCueError::new(
                    codes::NET_TLS,
                    format!("HTTP {} from {url}", resp.status()),
                ));
            }
            resp.text().await.map_err(|e| net::reqwest_error(&e))
        }
    };
    let pages = join_all([
        get(ANTHROPIC_MARKDOWN),
        get(SONIOX_PAGE),
        get(DEEPGRAM_PAGE),
    ])
    .await;
    let parsers: [(&str, Parser); 3] = [
        ("anthropic", parse_anthropic),
        ("soniox", parse_soniox),
        ("deepgram", parse_deepgram),
    ];
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for ((provider, parse), page) in parsers.into_iter().zip(pages) {
        match page {
            Ok(text) => {
                let found = parse(&text, today);
                if found.is_empty() {
                    errors.push(format!("{provider}: no prices found on the page"));
                }
                entries.extend(found);
            }
            Err(e) => errors.push(format!("{provider}: {e}")),
        }
    }
    (entries, errors)
}

/// The first `$` amount in `s`, e.g. `$6.25 / MTok` → 6.25.
fn dollars(s: &str) -> Option<f64> {
    let rest = &s[s.find('$')? + 1..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
        .unwrap_or(rest.len());
    rest[..end].replace(',', "").parse().ok()
}

fn plausible(v: f64, max: f64) -> bool {
    v.is_finite() && v > 0.0 && v <= max
}

/// Removes `<…>` tags, scripts and styles, and collapses whitespace.
fn plain_text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut i = 0;
    while i < html.len() {
        let rest = &lower[i..];
        let skip = [("<script", "</script>"), ("<style", "</style>")]
            .into_iter()
            .find(|(open, _)| rest.starts_with(open));
        if let Some((_, close)) = skip {
            match rest.find(close) {
                Some(p) => i += p + close.len(),
                None => break,
            }
            out.push(' ');
            continue;
        }
        // Step by whole characters so slicing never splits one.
        let ch = html[i..].chars().next().unwrap_or(' ');
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
        i += ch.len_utf8();
    }
    let out = out
        .replace("&amp;", "&")
        .replace("&nbsp;", " ")
        .replace("&#36;", "$");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Model id from a display name: "Claude Opus 4.8" → `claude-opus-4-8`.
fn claude_id(name: &str) -> Option<String> {
    let id = name.trim().to_lowercase().replace(['.', ' '], "-");
    let ok = id.starts_with("claude-")
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    ok.then_some(id)
}

/// Removes `<sup>…</sup>` and turns `[text](url)` into `text`.
fn clean_cell(cell: &str) -> String {
    let mut s = cell.to_string();
    while let (Some(a), Some(b)) = (s.find("<sup>"), s.find("</sup>")) {
        if b < a {
            break;
        }
        s.replace_range(a..b + "</sup>".len(), "");
    }
    while let Some(a) = s.find("](") {
        let Some(b) = s[a..].find(')') else { break };
        s.replace_range(a..a + b + 1, "");
    }
    s.replace(['[', ']'], "").trim().to_string()
}

/// Reads the "Model pricing" table of Anthropic's pricing page (Markdown).
pub fn parse_anthropic(md: &str, today: &str) -> Vec<PriceEntry> {
    let Some(start) = md.find("## Model pricing") else {
        return Vec::new();
    };
    let rows: Vec<Vec<String>> = md[start..]
        .lines()
        .skip_while(|l| !l.trim_start().starts_with('|'))
        .take_while(|l| l.trim_start().starts_with('|'))
        .map(|l| {
            l.trim()
                .trim_matches('|')
                .split('|')
                .map(clean_cell)
                .collect()
        })
        .collect();
    let Some(header) = rows.first() else {
        return Vec::new();
    };
    let col = |name: &str| {
        header
            .iter()
            .position(|h| h.to_lowercase().starts_with(name))
    };
    let (Some(input), Some(write), Some(read), Some(output)) = (
        col("base input"),
        col("5m cache write"),
        col("cache hits"),
        col("output"),
    ) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in rows.iter().skip(2) {
        let Some(name_cell) = row.first() else {
            continue;
        };
        let lower = name_cell.to_lowercase();
        // Retired models can't be called; the long-prompt tier of a model is
        // a second row whose base price is the one MeetCue's short prompts pay.
        if lower.contains("retired") || lower.contains("over 100") || lower.contains("deprecated") {
            continue;
        }
        let (name, note) = match name_cell.split_once('(') {
            Some((n, rest)) => (n.trim(), Some(rest.trim_end_matches(')').trim())),
            None => (name_cell.as_str(), None),
        };
        let Some(model) = claude_id(name) else {
            continue;
        };
        let price = |i: usize| row.get(i).and_then(|c| dollars(c));
        let (Some(i), Some(o)) = (price(input), price(output)) else {
            continue;
        };
        if !plausible(i, 1000.0) || !plausible(o, 1000.0) {
            continue;
        }
        let note = note
            .filter(|n| n.contains("prompts"))
            .map(|n| format!("Price {n}."));
        out.push(PriceEntry {
            provider: "anthropic".into(),
            model,
            label: name.to_string(),
            unit: PriceUnit::PerMillionTokens,
            input: Some(i),
            output: Some(o),
            cache_read: price(read).filter(|v| plausible(*v, 1000.0)),
            cache_write: price(write).filter(|v| plausible(*v, 1000.0)),
            audio: None,
            note,
            source: ANTHROPIC_PAGE.into(),
            verified: today.into(),
        });
    }
    out
}

/// Reads the streaming model rows of Soniox's pricing page, e.g.
/// "Soniox stt-rt-v5 $120 Updated June 2026 $0.12/hr".
pub fn parse_soniox(html: &str, today: &str) -> Vec<PriceEntry> {
    let text = plain_text(html);
    let mut out: Vec<PriceEntry> = Vec::new();
    let mut from = 0;
    while let Some(p) = text[from..].find("stt-rt-") {
        let at = from + p;
        from = at + 7;
        let model: String = text[at..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        // The hourly price follows the model name within the same row.
        let row = &text[at..text.len().min(at + 120)];
        let Some(h) = row.find("/hr") else { continue };
        let Some(dollar) = row[..h].rfind('$') else {
            continue;
        };
        let Some(v) = dollars(&row[dollar..h]) else {
            continue;
        };
        if !plausible(v, 10.0) || out.iter().any(|e| e.model == model) {
            continue;
        }
        out.push(PriceEntry {
            provider: "soniox".into(),
            label: format!("Soniox {model}"),
            model,
            unit: PriceUnit::PerAudioHour,
            input: None,
            output: None,
            cache_read: None,
            cache_write: None,
            audio: Some(v),
            note: Some("Real-time (streaming) price.".into()),
            source: SONIOX_PAGE.into(),
            verified: today.into(),
        });
    }
    out
}

/// Reads Nova-3 from the streaming table of Deepgram's pricing page. The
/// first price after the model name is the pay-as-you-go rate billed today
/// (a promotional "current price" when one runs).
pub fn parse_deepgram(html: &str, today: &str) -> Vec<PriceEntry> {
    let text = plain_text(html);
    let Some(section) = text.find("Streaming pricing") else {
        return Vec::new();
    };
    let text = &text[section..];
    // MeetCue asks for one language with Chinese or English (see
    // `deepgram::language_param`), which bills at the monolingual rate.
    let Some(at) = text.find("Nova-3 Monolingual") else {
        return Vec::new();
    };
    let row = &text[at..text.len().min(at + 600)];
    let Some(m) = row.find("/min") else {
        return Vec::new();
    };
    let Some(dollar) = row[..m].rfind('$') else {
        return Vec::new();
    };
    let Some(current) = dollars(&row[dollar..m]) else {
        return Vec::new();
    };
    if !plausible(current, 1.0) {
        return Vec::new();
    }
    let regular = row[m..]
        .split_once("Regular price")
        .filter(|(before, _)| !before.contains("Current price"))
        .and_then(|(_, after)| dollars(after))
        .filter(|r| *r > current);
    let promo = row[..m].contains("Current price");
    let note = match (promo, regular) {
        (true, Some(r)) => format!("Limited-time promotional rate; the regular rate is ${r}/min."),
        _ => "Streaming, one language.".into(),
    };
    vec![PriceEntry {
        provider: "deepgram".into(),
        model: "nova-3".into(),
        label: "Deepgram Nova-3".into(),
        unit: PriceUnit::PerAudioMinute,
        input: None,
        output: None,
        cache_read: None,
        cache_write: None,
        audio: Some(current),
        note: Some(note),
        source: DEEPGRAM_PAGE.into(),
        verified: today.into(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANTHROPIC: &str = r#"
## Model pricing

The following table shows pricing for all Claude models:

| Model                                                       | Base input tokens     | 5m cache writes | 1h cache writes | Cache hits and refreshes | Output tokens          |
| :---------------------------------------------------------- | :-------------------- | :-------------- | :-------------- | :----------------------- | :--------------------- |
| Claude Opus 5.5                                             | $4 / MTok             | $5 / MTok       | $8 / MTok       | $0.20 / MTok<sup>2</sup> | $20 / MTok             |
| Claude Opus 4.8                                             | $5 / MTok             | $6.25 / MTok    | $10 / MTok      | $0.50 / MTok             | $25 / MTok             |
| Claude Opus 4.1 ([retired, except on Bedrock](https://x))   | $15 / MTok            | $18.75 / MTok   | $30 / MTok      | $1.50 / MTok             | $75 / MTok             |
| Claude Mythos 5.1 ([limited availability](https://x))       | $10 / MTok            | $12.50 / MTok   | $20 / MTok      | $0.25 / MTok<sup>1</sup> | $50 / MTok             |
| Claude Sonnet 5                                             | $2 / MTok<sup>3</sup> | $2.50 / MTok    | $4 / MTok       | $0.20 / MTok             | $10 / MTok<sup>3</sup> |
| Claude Haiku 5.5 (for prompts up to 100,000 tokens)         | $0.10 / MTok          | $0.125 / MTok   | $0.20 / MTok    | $0.01 / MTok             | $0.50 / MTok           |
| Claude Haiku 5.5 (for prompts over 100,000 tokens)          | $0.50 / MTok          | $0.625 / MTok   | $1 / MTok       | $0.05 / MTok             | $2.50 / MTok           |

## Batch processing

| Model | Batch input | Batch output |
| Claude Opus 5.5 | $2 / MTok | $10 / MTok |
"#;

    #[test]
    fn reads_anthropic_model_table() {
        let e = parse_anthropic(ANTHROPIC, "2026-10-24");
        let ids: Vec<&str> = e.iter().map(|e| e.model.as_str()).collect();
        assert_eq!(
            ids,
            [
                "claude-opus-5-5",
                "claude-opus-4-8",
                "claude-mythos-5-1",
                "claude-sonnet-5",
                "claude-haiku-5-5"
            ]
        );
        let opus = &e[1];
        assert_eq!(
            (opus.input, opus.output, opus.cache_read, opus.cache_write),
            (Some(5.0), Some(25.0), Some(0.5), Some(6.25))
        );
        assert_eq!(e[0].cache_read, Some(0.2), "footnote markers are ignored");
        let haiku = &e[4];
        assert_eq!((haiku.input, haiku.output), (Some(0.1), Some(0.5)));
        assert!(haiku.note.as_deref().unwrap().contains("up to 100,000"));
        assert!(e
            .iter()
            .all(|e| e.source == ANTHROPIC_PAGE && e.verified == "2026-10-24"));
    }

    #[test]
    fn unreadable_anthropic_page_gives_nothing() {
        assert!(parse_anthropic("# Pricing\n\nSee claude.com.", "d").is_empty());
        let wrong_columns =
            "## Model pricing\n\n| Model | Price |\n| - | - |\n| Claude X 1 | $1 |\n";
        assert!(parse_anthropic(wrong_columns, "d").is_empty());
    }

    const SONIOX: &str = r#"<div><span>Soniox</span> <span>stt-rt-v5</span><span>$120</span>
        <div class="x"><span>Updated June 2026</span><span class="y"><span>$0.12/hr</span></span></div></div>
        <div><span>OpenAI</span> <span>gpt-realtime-whisper</span><span>$1,020</span><span>$1.02/hr</span></div>
        <script>var stt-rt-v9 = "$99/hr";</script>"#;

    #[test]
    fn reads_soniox_streaming_rows() {
        let e = parse_soniox(SONIOX, "2026-10-24");
        assert_eq!(e.len(), 1, "scripts are ignored");
        assert_eq!(e[0].model, "stt-rt-v5");
        assert_eq!(e[0].audio, Some(0.12));
        assert_eq!(e[0].unit, PriceUnit::PerAudioHour);
        assert!(parse_soniox("<p>Contact sales</p>", "d").is_empty());
    }

    const DEEPGRAM: &str = r#"<h3>Streaming pricing</h3><table><tr><td>Model</td><td>Pay As You Go</td><td>Growth</td></tr>
        <tr><td>Flux English <i>i</i> Conversational speech recognition.</td><td>Current price $0.0065/min Regular price $0.0077/min</td></tr>
        <tr><td>Nova-3 Monolingual <i>i</i> Our highest performing model.</td>
        <td><span>Current price</span> <span>$0.0048/min</span> <span>Regular price</span> <s>$0.0077/min</s></td>
        <td>Current price $0.0042/min Regular price $0.0065/min</td></tr></table>
        <h3>Pre-recorded</h3><p>Nova-3 Monolingual $0.0043/min</p>"#;

    #[test]
    fn reads_deepgram_current_streaming_rate() {
        let e = parse_deepgram(DEEPGRAM, "2026-10-24");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].model, "nova-3");
        assert_eq!(e[0].audio, Some(0.0048));
        assert!(e[0].note.as_deref().unwrap().contains("$0.0077/min"));
        // Without a promotion, the single price is the regular one.
        let plain =
            "<h3>Streaming pricing</h3><p>Nova-3 Monolingual desc $0.0077/min $0.0065/min</p>";
        let e = parse_deepgram(plain, "d");
        assert_eq!(e[0].audio, Some(0.0077));
        assert_eq!(e[0].note.as_deref(), Some("Streaming, one language."));
        assert!(parse_deepgram("<p>Nova-3 Monolingual $0.0043/min</p>", "d").is_empty());
    }

    #[test]
    fn helpers() {
        assert_eq!(dollars("$6.25 / MTok"), Some(6.25));
        assert_eq!(dollars("$1,020"), Some(1020.0));
        assert_eq!(dollars("free"), None);
        assert_eq!(
            claude_id("Claude Opus 4.8").as_deref(),
            Some("claude-opus-4-8")
        );
        assert_eq!(claude_id("GPT 5"), None);
        assert_eq!(plain_text("<b>a</b>&amp;<i>b</i>"), "a & b");
        assert_eq!(
            plain_text("<p title=\"价格\">价 $1</p><style>p{}</style>x"),
            "价 $1 x"
        );
    }
}
