//! The price list: no prices are built in. Prices are read live from each
//! provider's official pricing page where possible, on top of the list
//! published with the documentation (which covers anything a page could
//! not be read for). The result is cached in the configuration folder for
//! offline use and refreshed at start-up and when the settings open.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use meetcue_core::ipc::AppEvent;
use meetcue_core::pricing::PRICING_URLS;
use meetcue_core::{codes, MeetCueError, PriceEntry, PriceTable};
use meetcue_pipeline::{live_prices, net};

use crate::state::AppState;
use crate::{config, APP_EVENT};

/// A refresh that is not asked for explicitly is skipped this soon after the last one.
const MIN_INTERVAL: Duration = Duration::from_secs(600);

static LAST_REFRESH: Mutex<Option<Instant>> = Mutex::new(None);

/// Loads the cached list, if any, so prices are known before the network answers.
pub fn load_cached(state: &AppState) {
    let path = state.paths.pricing_cache();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    match PriceTable::parse(&text) {
        Ok(t) => PriceTable::set_current(t),
        Err(e) => tracing::warn!("ignoring cached price list: {e}"),
    }
}

fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

async fn published(proxy: Option<&str>) -> Result<PriceTable, MeetCueError> {
    let client = net::http_client(proxy)?;
    let mut last = MeetCueError::new(codes::NET_TLS, "no price list source");
    for url in PRICING_URLS {
        let fetched = async {
            let resp = client
                .get(*url)
                .timeout(Duration::from_secs(10))
                .send()
                .await
                .map_err(|e| net::reqwest_error(&e))?;
            if !resp.status().is_success() {
                return Err(MeetCueError::new(
                    codes::NET_TLS,
                    format!("HTTP {} from {url}", resp.status()),
                ));
            }
            let text = resp.text().await.map_err(|e| net::reqwest_error(&e))?;
            PriceTable::parse(&text)
                .map_err(|e| MeetCueError::new(codes::NET_TLS, format!("{url}: {e}")))
        }
        .await;
        match fetched {
            Ok(t) => return Ok(t),
            Err(e) => {
                tracing::warn!("price list from {url}: {e}");
                last = e;
            }
        }
    }
    Err(last)
}

/// Uses `table` from now on: checks it, caches it and tells every window.
fn install(app: &AppHandle, table: PriceTable) -> Result<PriceTable, MeetCueError> {
    let json = serde_json::to_string_pretty(&table).unwrap_or_default();
    // The same checks as a downloaded list, so live figures can't slip past them.
    let table = PriceTable::parse(&json)
        .map_err(|e| MeetCueError::new(codes::NET_TLS, format!("price list rejected: {e}")))?;
    let state = app.state::<AppState>();
    if let Err(e) = config::write_atomic(&state.paths.pricing_cache(), json.as_bytes()) {
        tracing::warn!("could not cache the price list: {e}");
    }
    PriceTable::set_current(table.clone());
    let _ = app.emit(
        APP_EVENT,
        AppEvent::PricingChanged {
            pricing: Box::new(table.clone()),
        },
    );
    Ok(table)
}

/// Reads the providers' pricing pages and the published list. With `force`
/// false, does nothing if prices were refreshed in the last few minutes.
pub async fn refresh(app: &AppHandle, force: bool) -> Result<PriceTable, MeetCueError> {
    {
        let mut last = LAST_REFRESH.lock().unwrap();
        let recent = last.is_some_and(|t| t.elapsed() < MIN_INTERVAL);
        if recent && !force && !PriceTable::current().entries.is_empty() {
            return Ok((*PriceTable::current()).clone());
        }
        *last = Some(Instant::now());
    }
    let proxy = app.state::<AppState>().settings().providers.proxy;
    let today = today();
    let (base, (live, errors)) = tokio::join!(
        published(proxy.as_deref()),
        live_prices::fetch_all(proxy.as_deref(), &today)
    );
    for e in &errors {
        tracing::warn!("live prices: {e}");
    }
    tracing::info!(
        "read {} prices live from the providers' pricing pages",
        live.len()
    );
    let base = match base {
        Ok(t) => t,
        // Keep what we had (the cache) and still apply the live figures.
        Err(e) if !live.is_empty() || !PriceTable::current().entries.is_empty() => {
            tracing::warn!("published price list unavailable: {e}");
            (*PriceTable::current()).clone()
        }
        Err(e) => return Err(e),
    };
    if live.is_empty() && base.updated < PriceTable::current().updated {
        // Never fall back to an older list.
        return Ok((*PriceTable::current()).clone());
    }
    install(app, base.with_live(live, &today))
}

/// Adds prices a provider listed with its models (OpenAI-compatible servers).
pub fn add_listed(app: &AppHandle, prices: Vec<PriceEntry>) {
    if prices.is_empty() {
        return;
    }
    let current = PriceTable::current();
    let changed = prices
        .iter()
        .any(|p| current.find(&[], &p.provider, &p.model) != Some(p));
    if !changed {
        return;
    }
    let base = if current.entries.is_empty() {
        PriceTable {
            updated: today(),
            ..(*current).clone()
        }
    } else {
        (*current).clone()
    };
    if let Err(e) = install(app, base.with_live(prices, &today())) {
        tracing::warn!("{e}");
    }
}
