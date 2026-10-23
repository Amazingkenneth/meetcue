//! `--self-test`: checks the things that cannot be verified in CI and
//! prints a plain-text report the user can paste into an issue.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Manager};

use meetcue_audio::{CaptureConfig, SourceEvent};
use meetcue_core::Channel;
use meetcue_pipeline::providers;

use crate::state::AppState;
use crate::windows::OVERLAY;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: &'static str,
    pub outcome: Outcome,
    pub detail: String,
}

pub fn render(checks: &[Check]) -> String {
    let mut out = format!(
        "MeetCue self-test · version {} · {} {}\n",
        meetcue_core::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    for c in checks {
        let tag = match c.outcome {
            Outcome::Pass => "PASS",
            Outcome::Fail => "FAIL",
            Outcome::Skip => "SKIP",
        };
        let _ = writeln!(out, "[{tag}] {:<28} {}", c.name, c.detail);
    }
    let failed = checks.iter().filter(|c| c.outcome == Outcome::Fail).count();
    let _ = writeln!(
        out,
        "Result: {} passed, {} failed, {} skipped",
        checks.iter().filter(|c| c.outcome == Outcome::Pass).count(),
        failed,
        checks.iter().filter(|c| c.outcome == Outcome::Skip).count()
    );
    out
}

/// Plays a tone and checks that the loopback channel itself receives signal.
/// Speakers should be muted (or headphones used) so the microphone cannot
/// fake a pass by picking up the tone.
fn audio_checks() -> Vec<Check> {
    let levels = Arc::new(Mutex::new([f32::MIN; 2]));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut source = meetcue_audio::platform_source();
    let (l, e) = (levels.clone(), errors.clone());
    let started = source.start(
        CaptureConfig::default(),
        Box::new(move |ev| match ev {
            SourceEvent::Frame(f) => {
                let i = usize::from(f.channel == Channel::Remote);
                let mut lv = l.lock().unwrap();
                lv[i] = lv[i].max(f.level_db);
            }
            SourceEvent::Error(ch, err) => {
                e.lock()
                    .unwrap()
                    .push(format!("{}: {}", ch.as_str(), err.code))
            }
            SourceEvent::DeviceChanged(_) => {}
        }),
    );
    if let Err(err) = started {
        return vec![Check {
            name: "audio capture",
            outcome: Outcome::Fail,
            detail: err.to_string(),
        }];
    }
    std::thread::sleep(Duration::from_millis(500));
    let tone = source.play_test_tone(440.0, 1500);
    std::thread::sleep(Duration::from_millis(300));
    source.stop();
    let [me, remote] = *levels.lock().unwrap();
    let errs = errors.lock().unwrap().join("; ");
    let mut checks = vec![Check {
        name: "system sound (loopback)",
        outcome: if tone.is_ok() && remote > -50.0 {
            Outcome::Pass
        } else {
            Outcome::Fail
        },
        detail: format!(
            "peak {remote:.0} dBFS while playing a test tone{}{}",
            tone.err()
                .map(|e| format!(" · tone error {e}"))
                .unwrap_or_default(),
            if errs.is_empty() {
                String::new()
            } else {
                format!(" · {errs}")
            }
        ),
    }];
    checks.push(Check {
        name: "microphone",
        outcome: if me > -95.0 {
            Outcome::Pass
        } else {
            Outcome::Fail
        },
        detail: format!("peak {me:.0} dBFS (speak or tap the microphone during the test)"),
    });
    checks
}

/// Shows the overlay filled with a marker color and checks a screen capture
/// does not contain it.
async fn share_hiding_check(app: &AppHandle) -> Check {
    let Some(win) = app.get_webview_window(OVERLAY) else {
        return Check {
            name: "hidden in screen share",
            outcome: Outcome::Skip,
            detail: "no panel".into(),
        };
    };
    let _ = win.eval("document.body.style.background='#ff00ff';document.getElementById('root').style.display='none';document.documentElement.style.background='#ff00ff'");
    let _ = win.show();
    tokio::time::sleep(Duration::from_millis(800)).await;
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.inner_size()) else {
        return Check {
            name: "hidden in screen share",
            outcome: Outcome::Skip,
            detail: "no geometry".into(),
        };
    };
    let w = (size.width as i32).min(300);
    let h = (size.height as i32).min(300);
    let result = meetcue_platform::window::capture_screen(pos.x, pos.y, w, h);
    let _ = win.hide();
    match result {
        Ok(px) => {
            let share = meetcue_platform::window::color_share(&px, 0xFF00FF, 8);
            Check {
                name: "hidden in screen share",
                outcome: if share < 0.05 {
                    Outcome::Pass
                } else {
                    Outcome::Fail
                },
                detail: format!(
                    "{:.0}% of the panel area visible in a screen capture",
                    share * 100.0
                ),
            }
        }
        Err(e) => Check {
            name: "hidden in screen share",
            outcome: Outcome::Skip,
            detail: e.to_string(),
        },
    }
}

async fn provider_checks(app: &AppHandle) -> Vec<Check> {
    let state = app.state::<AppState>();
    let s = state.settings();
    let mut out = Vec::new();
    for (slot, choice) in [
        ("speech recognition", &s.providers.asr),
        ("hint model", &s.providers.hints),
    ] {
        let (Some(p), Some(m)) = (&choice.provider, &choice.model) else {
            out.push(Check {
                name: if slot.starts_with("speech") {
                    "speech recognition"
                } else {
                    "hint model"
                },
                outcome: Outcome::Skip,
                detail: "not configured".into(),
            });
            continue;
        };
        let name = if slot.starts_with("speech") {
            "speech recognition"
        } else {
            "hint model"
        };
        let cfg = match crate::session::provider_config(&state, &s, p, m, &choice.endpoint) {
            Ok(c) if c.key.is_some() || p == "openai-compatible" => c,
            Ok(_) => {
                out.push(Check {
                    name,
                    outcome: Outcome::Skip,
                    detail: format!("{p}: no key"),
                });
                continue;
            }
            Err(e) => {
                out.push(Check {
                    name,
                    outcome: Outcome::Fail,
                    detail: e.to_string(),
                });
                continue;
            }
        };
        let r = if slot.starts_with("speech") {
            match providers::asr(&cfg) {
                Ok(a) => a.verify().await,
                Err(e) => Err(e),
            }
        } else {
            match providers::llm(&cfg) {
                Ok(l) => l.list_models().await.map(|_| ()),
                Err(e) => Err(e),
            }
        };
        out.push(match r {
            Ok(()) => Check {
                name,
                outcome: Outcome::Pass,
                detail: format!("{p} {m} via {}", cfg.host.unwrap_or_default()),
            },
            Err(e) => Check {
                name,
                outcome: Outcome::Fail,
                detail: format!("{p}: {e}"),
            },
        });
    }
    out
}

pub async fn run(app: &AppHandle) -> String {
    let mut checks = Vec::new();
    let webview = tauri::webview_version();
    checks.push(Check {
        name: "webview runtime",
        outcome: if webview.is_ok() {
            Outcome::Pass
        } else {
            Outcome::Fail
        },
        detail: webview.unwrap_or_else(|e| e.to_string()),
    });
    let audio = tauri::async_runtime::spawn_blocking(audio_checks)
        .await
        .unwrap_or_default();
    checks.extend(audio);
    let hotkeys = app.state::<AppState>().hotkeys.lock().unwrap().clone();
    let failed: Vec<_> = hotkeys
        .iter()
        .filter(|h| !h.ok)
        .map(|h| h.accelerator.clone())
        .collect();
    checks.push(Check {
        name: "global shortcuts",
        outcome: if failed.is_empty() && !hotkeys.is_empty() {
            Outcome::Pass
        } else {
            Outcome::Fail
        },
        detail: if failed.is_empty() {
            format!("{} registered", hotkeys.len())
        } else {
            format!("taken: {}", failed.join(", "))
        },
    });
    checks.push(share_hiding_check(app).await);
    checks.extend(provider_checks(app).await);
    render(&checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_format() {
        let r = render(&[
            Check {
                name: "a",
                outcome: Outcome::Pass,
                detail: "ok".into(),
            },
            Check {
                name: "b",
                outcome: Outcome::Fail,
                detail: "bad".into(),
            },
        ]);
        assert!(r.contains("[PASS] a"));
        assert!(r.contains("[FAIL] b"));
        assert!(r.ends_with("Result: 1 passed, 1 failed, 0 skipped\n"));
    }
}
