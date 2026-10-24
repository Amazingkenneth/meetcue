//! The price list: no prices are built in. The last downloaded list is
//! cached in the configuration folder and refreshed from the published copy
//! at start-up and on request.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use meetcue_core::ipc::AppEvent;
use meetcue_core::pricing::PRICING_URLS;
use meetcue_core::{codes, MeetCueError, PriceTable};
use meetcue_pipeline::net;

use crate::state::AppState;
use crate::{config, APP_EVENT};

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

/// Downloads the published list, keeps it if it is valid and not older than
/// the one in use, and tells every window.
pub async fn refresh(app: &AppHandle) -> Result<PriceTable, MeetCueError> {
    let proxy = app.state::<AppState>().settings().providers.proxy;
    let client = net::http_client(proxy.as_deref())?;
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
            let table = PriceTable::parse(&text)
                .map_err(|e| MeetCueError::new(codes::NET_TLS, format!("{url}: {e}")))?;
            Ok((table, text))
        }
        .await;
        match fetched {
            Ok((table, text)) => {
                // ISO dates compare as strings; never go back to an older list.
                if table.updated < PriceTable::current().updated {
                    return Ok((*PriceTable::current()).clone());
                }
                let state = app.state::<AppState>();
                if let Err(e) = config::write_atomic(&state.paths.pricing_cache(), text.as_bytes())
                {
                    tracing::warn!("could not cache the price list: {e}");
                }
                PriceTable::set_current(table.clone());
                let _ = app.emit(
                    APP_EVENT,
                    AppEvent::PricingChanged {
                        pricing: Box::new(table.clone()),
                    },
                );
                return Ok(table);
            }
            Err(e) => {
                tracing::warn!("price list from {url}: {e}");
                last = e;
            }
        }
    }
    Err(last)
}
