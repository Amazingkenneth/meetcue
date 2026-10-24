//! Cost metering and budget checks.

use meetcue_core::pricing::usd_to_micros;
use meetcue_core::settings::{BudgetAction, BudgetSettings};
use meetcue_core::{
    BudgetLevel, BudgetScope, MeetCueEvent, PriceEntry, PriceTable, UsageKind, UsagePurpose, Usd,
};

#[derive(Debug, Clone, PartialEq)]
pub struct UsageRecord {
    pub provider: String,
    pub model: String,
    pub purpose: UsagePurpose,
    pub usage: UsageKind,
    pub cost_micros: Option<Usd>,
}

/// What the session should do after a budget limit was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetOutcome {
    Continue,
    PauseHints,
    EndSession,
}

#[derive(Debug)]
pub struct CostMeter {
    overrides: Vec<PriceEntry>,
    budget: BudgetSettings,
    session: Usd,
    /// Spending earlier this month, before this session.
    month_before: Usd,
    warned: [bool; 2],
    reached: [bool; 2],
    records: Vec<UsageRecord>,
}

impl CostMeter {
    pub fn new(overrides: Vec<PriceEntry>, budget: BudgetSettings, month_before: Usd) -> Self {
        Self {
            overrides,
            budget,
            session: 0,
            month_before,
            warned: [false; 2],
            reached: [false; 2],
            records: Vec::new(),
        }
    }

    /// Changes prices and budget during a session. Limits are checked
    /// again from scratch, so a raised limit stops warning.
    pub fn set_rules(&mut self, overrides: Vec<PriceEntry>, budget: BudgetSettings) {
        if budget != self.budget {
            self.warned = [false; 2];
            self.reached = [false; 2];
        }
        self.overrides = overrides;
        self.budget = budget;
    }

    pub fn session_total(&self) -> Usd {
        self.session
    }

    pub fn records(&self) -> &[UsageRecord] {
        &self.records
    }

    /// Records one call and returns the events to emit plus the action to take.
    pub fn record(
        &mut self,
        provider: &str,
        model: &str,
        purpose: UsagePurpose,
        usage: UsageKind,
    ) -> (Vec<MeetCueEvent>, BudgetOutcome) {
        let cost = PriceTable::current()
            .find(&self.overrides, provider, model)
            .map(|p| p.cost(&usage));
        self.session += cost.unwrap_or(0);
        self.records.push(UsageRecord {
            provider: provider.into(),
            model: model.into(),
            purpose,
            usage: usage.clone(),
            cost_micros: cost,
        });
        let mut events = vec![MeetCueEvent::Usage {
            provider: provider.into(),
            model: model.into(),
            purpose,
            usage,
            cost_micros: cost,
            session_micros: self.session,
        }];
        let mut outcome = BudgetOutcome::Continue;
        let checks = [
            (
                BudgetScope::Session,
                self.budget.session_limit_usd,
                self.session,
            ),
            (
                BudgetScope::Month,
                self.budget.monthly_limit_usd,
                self.month_before + self.session,
            ),
        ];
        for (i, (scope, limit, spent)) in checks.into_iter().enumerate() {
            let Some(limit) = limit.filter(|l| *l > 0.0).map(usd_to_micros) else {
                continue;
            };
            if spent >= limit && !self.reached[i] {
                self.reached[i] = true;
                self.warned[i] = true;
                events.push(MeetCueEvent::Budget {
                    scope,
                    level: BudgetLevel::Reached,
                    spent_micros: spent,
                    limit_micros: limit,
                });
                outcome = match self.budget.on_limit {
                    BudgetAction::NotifyOnly => outcome,
                    BudgetAction::PauseHints => BudgetOutcome::PauseHints,
                    BudgetAction::EndSession => BudgetOutcome::EndSession,
                };
            } else if spent * 5 >= limit * 4 && !self.warned[i] {
                self.warned[i] = true;
                events.push(MeetCueEvent::Budget {
                    scope,
                    level: BudgetLevel::Warning,
                    spent_micros: spent,
                    limit_micros: limit,
                });
            }
        }
        (events, outcome)
    }
}

/// Loads the price list published from this repository (tests).
#[cfg(test)]
pub(crate) fn load_published_prices() {
    let json = include_str!("../../core/data/pricing.json");
    PriceTable::set_current(PriceTable::parse(json).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn llm(input: u64, output: u64) -> UsageKind {
        UsageKind::Llm {
            input,
            output,
            cache_read: 0,
            cache_write: 0,
        }
    }

    #[test]
    fn known_and_unknown_prices() {
        load_published_prices();
        let mut m = CostMeter::new(vec![], BudgetSettings::default(), 0);
        let (ev, _) = m.record(
            "anthropic",
            "claude-haiku-5-5",
            UsagePurpose::Hints,
            llm(1_000_000, 0),
        );
        assert!(matches!(
            ev[0],
            MeetCueEvent::Usage {
                cost_micros: Some(100_000),
                ..
            }
        ));
        let (ev, _) = m.record(
            "openai-compatible",
            "local",
            UsagePurpose::Hints,
            llm(1000, 1000),
        );
        assert!(matches!(
            ev[0],
            MeetCueEvent::Usage {
                cost_micros: None,
                ..
            }
        ));
        assert_eq!(m.session_total(), 100_000);
    }

    #[test]
    fn budget_warns_once_then_applies_action() {
        load_published_prices();
        let budget = BudgetSettings {
            session_limit_usd: Some(1.0),
            monthly_limit_usd: None,
            on_limit: BudgetAction::PauseHints,
        };
        let mut m = CostMeter::new(vec![], budget, 0);
        let haiku = |m: &mut CostMeter, tokens| {
            m.record(
                "anthropic",
                "claude-haiku-5-5",
                UsagePurpose::Hints,
                llm(tokens, 0),
            )
        };
        // $0.70: no warning.
        let (ev, out) = haiku(&mut m, 7_000_000);
        assert_eq!(ev.len(), 1);
        assert_eq!(out, BudgetOutcome::Continue);
        // $0.80: warning.
        let (ev, _) = haiku(&mut m, 1_000_000);
        assert!(matches!(
            ev[1],
            MeetCueEvent::Budget {
                level: BudgetLevel::Warning,
                ..
            }
        ));
        // $0.90: nothing new.
        let (ev, _) = haiku(&mut m, 1_000_000);
        assert_eq!(ev.len(), 1);
        // $1.00: reached, pause hints.
        let (ev, out) = haiku(&mut m, 1_000_000);
        assert!(matches!(
            ev[1],
            MeetCueEvent::Budget {
                level: BudgetLevel::Reached,
                ..
            }
        ));
        assert_eq!(out, BudgetOutcome::PauseHints);
        let (_, out) = haiku(&mut m, 1_000_000);
        assert_eq!(out, BudgetOutcome::Continue, "action applied once");
    }

    #[test]
    fn monthly_budget_counts_earlier_spending() {
        load_published_prices();
        let budget = BudgetSettings {
            session_limit_usd: None,
            monthly_limit_usd: Some(10.0),
            on_limit: BudgetAction::EndSession,
        };
        let mut m = CostMeter::new(vec![], budget, 9_950_000);
        let (ev, out) = m.record(
            "anthropic",
            "claude-haiku-5-5",
            UsagePurpose::Hints,
            llm(1_000_000, 0),
        );
        assert!(matches!(
            ev[1],
            MeetCueEvent::Budget {
                scope: BudgetScope::Month,
                level: BudgetLevel::Reached,
                ..
            }
        ));
        assert_eq!(out, BudgetOutcome::EndSession);
    }
}
