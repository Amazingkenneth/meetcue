//! Usage ledger (`usage.jsonl`): one line per billed call, used for the
//! monthly summary, the monthly budget and the CSV export.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use meetcue_core::ipc::{PurposeRow, UsageRow, UsageSummary};
use meetcue_core::{UsageKind, UsagePurpose};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerLine {
    /// RFC 3339 timestamp.
    pub at: String,
    pub provider: String,
    pub model: String,
    pub purpose: UsagePurpose,
    pub usage: UsageKind,
    pub micros: Option<u64>,
}

pub fn append(path: &Path, line: &LedgerLine) -> std::io::Result<()> {
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(line).expect("serialize"))
}

pub fn read(path: &Path) -> Vec<LedgerLine> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Summary of one month (`YYYY-MM`).
pub fn summary(lines: &[LedgerLine], month: &str) -> UsageSummary {
    let mut rows: BTreeMap<(String, String), UsageRow> = BTreeMap::new();
    let mut purposes: Vec<PurposeRow> = Vec::new();
    for l in lines.iter().filter(|l| l.at.starts_with(month)) {
        let p = match purposes.iter_mut().position(|p| p.purpose == l.purpose) {
            Some(i) => &mut purposes[i],
            None => {
                purposes.push(PurposeRow {
                    purpose: l.purpose,
                    calls: 0,
                    micros: 0,
                    unpriced: false,
                });
                purposes.last_mut().unwrap()
            }
        };
        p.calls += 1;
        match l.micros {
            Some(m) => p.micros += m,
            None => p.unpriced = true,
        }
        let row = rows
            .entry((l.provider.clone(), l.model.clone()))
            .or_insert_with(|| UsageRow {
                provider: l.provider.clone(),
                model: l.model.clone(),
                calls: 0,
                input_tokens: 0,
                output_tokens: 0,
                cached_tokens: 0,
                audio_ms: 0,
                micros: Some(0),
            });
        row.calls += 1;
        match &l.usage {
            UsageKind::Llm {
                input,
                output,
                cache_read,
                cache_write,
            } => {
                row.input_tokens += input + cache_read + cache_write;
                row.output_tokens += output;
                row.cached_tokens += cache_read;
            }
            UsageKind::Audio { ms } => row.audio_ms += ms,
        }
        row.micros = match (row.micros, l.micros) {
            (Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
    }
    let rows: Vec<UsageRow> = rows.into_values().collect();
    UsageSummary {
        month: month.to_string(),
        total_micros: rows.iter().filter_map(|r| r.micros).sum(),
        rows,
        by_purpose: {
            purposes.sort_by_key(|p| std::cmp::Reverse(p.micros));
            purposes
        },
    }
}

pub fn month_total(lines: &[LedgerLine], month: &str) -> u64 {
    lines
        .iter()
        .filter(|l| l.at.starts_with(month))
        .filter_map(|l| l.micros)
        .sum()
}

pub fn csv(lines: &[LedgerLine]) -> String {
    let mut out = String::from("time,provider,model,purpose,input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,audio_ms,cost_usd\n");
    for l in lines {
        let (i, o, cr, cw, a) = match &l.usage {
            UsageKind::Llm {
                input,
                output,
                cache_read,
                cache_write,
            } => (*input, *output, *cache_read, *cache_write, 0),
            UsageKind::Audio { ms } => (0, 0, 0, 0, *ms),
        };
        let purpose = serde_json::to_value(l.purpose)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let cost = l
            .micros
            .map(|m| format!("{:.6}", m as f64 / 1_000_000.0))
            .unwrap_or_default();
        out.push_str(&format!(
            "{},{},{},{},{i},{o},{cr},{cw},{a},{cost}\n",
            l.at,
            csv_field(&l.provider),
            csv_field(&l.model),
            purpose
        ));
    }
    out
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(at: &str, provider: &str, usage: UsageKind, micros: Option<u64>) -> LedgerLine {
        LedgerLine {
            at: at.into(),
            provider: provider.into(),
            model: "m".into(),
            purpose: UsagePurpose::Hints,
            usage,
            micros,
        }
    }

    #[test]
    fn ledger_summary_and_csv() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.jsonl");
        let llm = UsageKind::Llm {
            input: 100,
            output: 50,
            cache_read: 10,
            cache_write: 0,
        };
        append(
            &path,
            &line("2026-10-01T10:00:00Z", "anthropic", llm.clone(), Some(1000)),
        )
        .unwrap();
        append(
            &path,
            &line("2026-10-02T10:00:00Z", "anthropic", llm, Some(500)),
        )
        .unwrap();
        append(
            &path,
            &line(
                "2026-10-02T10:00:00Z",
                "soniox",
                UsageKind::Audio { ms: 60_000 },
                Some(2000),
            ),
        )
        .unwrap();
        append(
            &path,
            &line(
                "2026-09-30T10:00:00Z",
                "soniox",
                UsageKind::Audio { ms: 1 },
                Some(7),
            ),
        )
        .unwrap();
        append(
            &path,
            &line(
                "2026-10-03T10:00:00Z",
                "openai-compatible",
                UsageKind::Audio { ms: 0 },
                None,
            ),
        )
        .unwrap();
        let lines = read(&path);
        assert_eq!(lines.len(), 5);
        let s = summary(&lines, "2026-10");
        assert_eq!(s.total_micros, 3500);
        let a = s.rows.iter().find(|r| r.provider == "anthropic").unwrap();
        assert_eq!(
            (a.calls, a.input_tokens, a.output_tokens, a.micros),
            (2, 220, 100, Some(1500))
        );
        assert!(s
            .rows
            .iter()
            .any(|r| r.provider == "openai-compatible" && r.micros.is_none()));
        assert_eq!(month_total(&lines, "2026-09"), 7);
        let c = csv(&lines);
        assert!(c.starts_with("time,provider"));
        assert!(c.contains("2026-10-01T10:00:00Z,anthropic,m,hints,100,50,10,0,0,0.001000"));
    }

    #[test]
    fn summary_splits_cost_by_purpose() {
        let audio = UsageKind::Audio { ms: 1 };
        let mut asr = line("2026-10-01T00:00:00Z", "soniox", audio.clone(), Some(900));
        asr.purpose = UsagePurpose::Asr;
        let mut detect = line("2026-10-01T00:00:00Z", "x", audio.clone(), None);
        detect.purpose = UsagePurpose::Detect;
        let lines = vec![
            asr,
            detect.clone(),
            LedgerLine {
                micros: Some(40),
                ..detect
            },
            line("2026-10-01T00:00:00Z", "x", audio, Some(300)),
        ];
        let s = summary(&lines, "2026-10");
        let by: Vec<_> = s
            .by_purpose
            .iter()
            .map(|p| (p.purpose, p.calls, p.micros, p.unpriced))
            .collect();
        assert_eq!(
            by,
            [
                (UsagePurpose::Asr, 1, 900, false),
                (UsagePurpose::Hints, 1, 300, false),
                (UsagePurpose::Detect, 2, 40, true),
            ]
        );
    }
}
