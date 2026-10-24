//! Price table, cost conversion and hourly estimates.
//!
//! Prices are not built into the app. The list is kept in `data/pricing.json`
//! in the repository, published with the documentation, and downloaded at
//! run time (see [`PRICING_URLS`]); until a list is loaded the table is empty
//! and costs are unknown.
//!
//! Money is stored and summed as integer micro-dollars ([`Usd`]) to avoid
//! floating-point drift over long sessions; it is converted to a decimal
//! amount only for display.

use std::sync::{Arc, OnceLock, RwLock};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::event::UsageKind;

/// An amount in micro-dollars (1 USD = 1 000 000).
pub type Usd = u64;

pub const MICROS_PER_USD: f64 = 1_000_000.0;

pub fn usd_to_micros(usd: f64) -> Usd {
    (usd * MICROS_PER_USD).round().max(0.0) as Usd
}

pub fn micros_to_usd(micros: Usd) -> f64 {
    micros as f64 / MICROS_PER_USD
}

/// The unit a price is quoted in. Every entry must name one explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PriceUnit {
    PerMillionTokens,
    PerThousandTokens,
    PerAudioHour,
    PerAudioMinute,
    PerAudioSecond,
}

impl PriceUnit {
    pub fn is_audio(self) -> bool {
        matches!(
            self,
            PriceUnit::PerAudioHour | PriceUnit::PerAudioMinute | PriceUnit::PerAudioSecond
        )
    }

    /// Multiplier that turns a price in this unit into USD per token.
    fn per_token(self) -> f64 {
        match self {
            PriceUnit::PerMillionTokens => 1e-6,
            PriceUnit::PerThousandTokens => 1e-3,
            _ => 0.0,
        }
    }

    /// Multiplier that turns a price in this unit into USD per audio millisecond.
    fn per_audio_ms(self) -> f64 {
        match self {
            PriceUnit::PerAudioHour => 1.0 / 3_600_000.0,
            PriceUnit::PerAudioMinute => 1.0 / 60_000.0,
            PriceUnit::PerAudioSecond => 1.0 / 1_000.0,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PriceEntry {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub label: String,
    pub unit: PriceUnit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub output: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cache_read: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cache_write: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub audio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
    #[serde(default)]
    pub source: String,
    /// Date the price was last checked against the provider's page.
    #[serde(default)]
    pub verified: String,
}

impl PriceEntry {
    /// Cost of one call in micro-dollars.
    pub fn cost(&self, usage: &UsageKind) -> Usd {
        let usd = match usage {
            UsageKind::Llm {
                input,
                output,
                cache_read,
                cache_write,
            } => {
                let k = self.unit.per_token();
                let input_price = self.input.unwrap_or(0.0);
                *input as f64 * input_price * k
                    + *output as f64 * self.output.unwrap_or(0.0) * k
                    + *cache_read as f64 * self.cache_read.unwrap_or(input_price) * k
                    + *cache_write as f64 * self.cache_write.unwrap_or(input_price) * k
            }
            UsageKind::Audio { ms } => {
                *ms as f64 * self.audio.unwrap_or(0.0) * self.unit.per_audio_ms()
            }
        };
        usd_to_micros(usd)
    }

    /// Hourly cost of audio in USD, if this is an audio price.
    pub fn audio_per_hour(&self) -> Option<f64> {
        self.audio
            .map(|p| p * self.unit.per_audio_ms() * 3_600_000.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PriceTable {
    pub updated: String,
    pub currency: String,
    pub entries: Vec<PriceEntry>,
}

/// Where the published price list is downloaded from, in order.
pub const PRICING_URLS: &[&str] = &[
    "https://amazingkenneth.github.io/meetcue/pricing.json",
    "https://raw.githubusercontent.com/Amazingkenneth/meetcue/main/crates/core/data/pricing.json",
];

fn current_slot() -> &'static RwLock<Arc<PriceTable>> {
    static CURRENT: OnceLock<RwLock<Arc<PriceTable>>> = OnceLock::new();
    CURRENT.get_or_init(|| RwLock::new(Arc::new(PriceTable::empty())))
}

impl PriceTable {
    /// A table with no prices: every cost is unknown.
    pub fn empty() -> PriceTable {
        PriceTable {
            updated: String::new(),
            currency: "USD".into(),
            entries: Vec::new(),
        }
    }

    /// The price list in use (empty until one is loaded).
    pub fn current() -> Arc<PriceTable> {
        current_slot().read().unwrap().clone()
    }

    pub fn set_current(table: PriceTable) {
        *current_slot().write().unwrap() = Arc::new(table);
    }

    /// Parses a published price list, rejecting anything that could produce
    /// wrong figures: other currencies, entries without a source or check
    /// date, missing or negative prices.
    pub fn parse(json: &str) -> Result<PriceTable, String> {
        let t: PriceTable = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if t.currency != "USD" {
            return Err(format!("unsupported currency {}", t.currency));
        }
        if t.updated.is_empty() || t.entries.is_empty() {
            return Err("empty price list".into());
        }
        for e in &t.entries {
            let name = format!("{}/{}", e.provider, e.model);
            if e.source.is_empty() || e.verified.is_empty() {
                return Err(format!("{name}: no source or check date"));
            }
            let prices = if e.unit.is_audio() {
                vec![e.audio]
            } else {
                vec![e.input, e.output]
            };
            if prices
                .iter()
                .any(|p| !p.is_some_and(|v| v.is_finite() && v >= 0.0))
            {
                return Err(format!("{name}: missing or invalid price"));
            }
        }
        Ok(t)
    }

    /// Looks up a price, letting user overrides win over the published list.
    pub fn find<'a>(
        &'a self,
        overrides: &'a [PriceEntry],
        provider: &str,
        model: &str,
    ) -> Option<&'a PriceEntry> {
        overrides
            .iter()
            .chain(self.entries.iter())
            .find(|e| e.provider == provider && e.model == model)
    }
}

/// Assumptions behind the "estimated cost per hour" figure. Users can edit
/// them in the settings and the estimate updates immediately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct CostAssumptions {
    /// Share of time that contains speech after voice activity detection.
    pub speech_ratio: f64,
    pub questions_per_hour: f64,
    /// Static system prompt (role, materials) billed at the cache-read price.
    pub system_tokens: f64,
    pub detect_input_tokens: f64,
    pub detect_output_tokens: f64,
    pub hint_input_tokens: f64,
    pub hint_output_tokens: f64,
    /// Extra share for early-trigger requests that are cancelled.
    pub cancel_overhead: f64,
}

impl Default for CostAssumptions {
    fn default() -> Self {
        Self {
            speech_ratio: 0.6,
            questions_per_hour: 20.0,
            system_tokens: 1500.0,
            detect_input_tokens: 900.0,
            detect_output_tokens: 80.0,
            hint_input_tokens: 1200.0,
            hint_output_tokens: 500.0,
            cancel_overhead: 0.10,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HourlyEstimate {
    #[ts(type = "number | null")]
    pub asr: Option<Usd>,
    #[ts(type = "number | null")]
    pub detect: Option<Usd>,
    #[ts(type = "number | null")]
    pub hints: Option<Usd>,
    /// Sum of the known parts; `complete` is false if any part is unknown.
    #[ts(type = "number")]
    pub total: Usd,
    pub complete: bool,
}

/// Estimated cost of one hour of meeting with both channels recognised.
pub fn estimate_hourly(
    asr: Option<&PriceEntry>,
    detect: Option<&PriceEntry>,
    hints: Option<&PriceEntry>,
    a: &CostAssumptions,
) -> HourlyEstimate {
    let asr_cost = asr.map(|p| {
        let ms = (3_600_000.0 * 2.0 * a.speech_ratio) as u64;
        p.cost(&UsageKind::Audio { ms })
    });
    let llm = |p: &PriceEntry, input: f64, output: f64| {
        let per_call = p.cost(&UsageKind::Llm {
            input: input as u64,
            output: output as u64,
            cache_read: a.system_tokens as u64,
            cache_write: 0,
        });
        (per_call as f64 * a.questions_per_hour * (1.0 + a.cancel_overhead)).round() as Usd
    };
    let detect_cost = detect.map(|p| llm(p, a.detect_input_tokens, a.detect_output_tokens));
    let hints_cost = hints.map(|p| llm(p, a.hint_input_tokens, a.hint_output_tokens));
    let parts = [asr_cost, detect_cost, hints_cost];
    HourlyEstimate {
        asr: asr_cost,
        detect: detect_cost,
        hints: hints_cost,
        total: parts.iter().flatten().sum(),
        complete: parts.iter().all(Option::is_some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(unit: PriceUnit) -> PriceEntry {
        PriceEntry {
            provider: "p".into(),
            model: "m".into(),
            label: String::new(),
            unit,
            input: Some(1.0),
            output: Some(5.0),
            cache_read: Some(0.1),
            cache_write: Some(1.25),
            audio: Some(0.12),
            note: None,
            source: String::new(),
            verified: String::new(),
        }
    }

    /// The list published from this repository.
    const PUBLISHED: &str = include_str!("../data/pricing.json");

    #[test]
    fn published_list_is_valid() {
        let t = PriceTable::parse(PUBLISHED).unwrap();
        let mut seen = std::collections::HashSet::new();
        for e in &t.entries {
            assert!(
                seen.insert((&e.provider, &e.model)),
                "duplicate {}",
                e.model
            );
            assert!(e.source.starts_with("https://"), "{}", e.model);
        }
    }

    #[test]
    fn rejects_unreliable_lists() {
        let ok = PriceTable::parse(PUBLISHED).unwrap();
        let bad = |f: &dyn Fn(&mut PriceTable)| {
            let mut t = ok.clone();
            f(&mut t);
            PriceTable::parse(&serde_json::to_string(&t).unwrap()).is_err()
        };
        assert!(bad(&|t| t.currency = "EUR".into()));
        assert!(bad(&|t| t.entries.clear()));
        assert!(bad(&|t| t.entries[0].source.clear()));
        assert!(bad(&|t| t.entries[0].input = None));
        assert!(bad(&|t| t.entries[0].input = Some(-1.0)));
        assert!(PriceTable::parse("not json").is_err());
    }

    #[test]
    fn starts_empty_until_loaded() {
        // Other tests may have loaded a table already; only check the empty table.
        let e = PriceTable::empty();
        assert!(e.find(&[], "anthropic", "claude-haiku-5-5").is_none());
    }

    #[test]
    fn token_units_convert() {
        let usage = UsageKind::Llm {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 0,
            cache_write: 0,
        };
        // $1 + $5 per million tokens.
        assert_eq!(entry(PriceUnit::PerMillionTokens).cost(&usage), 6_000_000);
        // Same numbers per thousand tokens are 1000x more expensive.
        assert_eq!(
            entry(PriceUnit::PerThousandTokens).cost(&usage),
            6_000_000_000
        );
        let cached = UsageKind::Llm {
            input: 0,
            output: 0,
            cache_read: 1_000_000,
            cache_write: 1_000_000,
        };
        assert_eq!(entry(PriceUnit::PerMillionTokens).cost(&cached), 1_350_000);
    }

    #[test]
    fn audio_units_convert() {
        let hour = UsageKind::Audio { ms: 3_600_000 };
        assert_eq!(entry(PriceUnit::PerAudioHour).cost(&hour), 120_000);
        assert_eq!(entry(PriceUnit::PerAudioMinute).cost(&hour), 7_200_000);
        assert_eq!(entry(PriceUnit::PerAudioSecond).cost(&hour), 432_000_000);
        let e = entry(PriceUnit::PerAudioMinute);
        assert!((e.audio_per_hour().unwrap() - 7.2).abs() < 1e-9);
    }

    #[test]
    fn micro_dollar_sums_do_not_drift() {
        let e = entry(PriceUnit::PerMillionTokens);
        let small = UsageKind::Llm {
            input: 100,
            output: 0,
            cache_read: 0,
            cache_write: 0,
        };
        let total: Usd = (0..10_000).map(|_| e.cost(&small)).sum();
        assert_eq!(total, 1_000_000); // 10k calls x 100 tokens x $1/MTok = $1
    }

    #[test]
    fn overrides_win() {
        let t = PriceTable::parse(PUBLISHED).unwrap();
        let mut o = t.entries[0].clone();
        o.input = Some(0.0);
        let overrides = vec![o.clone()];
        let found = t.find(&overrides, &o.provider, &o.model).unwrap();
        assert_eq!(found.input, Some(0.0));
    }

    #[test]
    fn hourly_estimate() {
        let a = CostAssumptions::default();
        let asr = entry(PriceUnit::PerAudioHour);
        let llm = entry(PriceUnit::PerMillionTokens);
        let est = estimate_hourly(Some(&asr), Some(&llm), None, &a);
        // 2 channels x 0.6 x $0.12 = $0.144
        assert_eq!(est.asr, Some(144_000));
        assert!(!est.complete);
        assert_eq!(est.total, est.asr.unwrap() + est.detect.unwrap());
    }
}
