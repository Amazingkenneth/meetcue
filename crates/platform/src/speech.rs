//! Offline system text-to-speech (SAPI/WinRT, AVSpeechSynthesizer,
//! speech-dispatcher) on a dedicated worker thread.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use meetcue_core::{codes, MeetCueError};

enum Command {
    Speak {
        text: String,
        lang: String,
        rate: f32,
        done: mpsc::Sender<Result<(), MeetCueError>>,
    },
}

/// Speaks text in the voice matching its language. Cloneable handle.
#[derive(Clone)]
pub struct Speaker {
    tx: mpsc::Sender<Command>,
    stop: Arc<Mutex<Option<tts::Tts>>>,
}

fn tts_err(e: tts::Error) -> MeetCueError {
    MeetCueError::new(codes::PRACTICE_TTS, e.to_string())
}

impl Speaker {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Command>();
        let stop: Arc<Mutex<Option<tts::Tts>>> = Arc::new(Mutex::new(None));
        let shared = stop.clone();
        thread::spawn(move || {
            let mut engine = match tts::Tts::default() {
                Ok(t) => Some(t),
                Err(e) => {
                    tracing::warn!("speech synthesis unavailable: {e}");
                    None
                }
            };
            if let Some(t) = &engine {
                *shared.lock().unwrap() = Some(t.clone());
            }
            for cmd in rx {
                match cmd {
                    Command::Speak {
                        text,
                        lang,
                        rate,
                        done,
                    } => {
                        let result = match engine.as_mut() {
                            None => Err(MeetCueError::new(codes::PRACTICE_TTS, "no speech engine")),
                            Some(t) => speak_blocking(t, &text, &lang, rate),
                        };
                        let _ = done.send(result);
                    }
                }
            }
        });
        Self { tx, stop }
    }

    /// Speaks and blocks until finished (or stopped). `lang` is `zh` or `en`.
    pub fn speak(&self, text: &str, lang: &str, rate: f32) -> Result<(), MeetCueError> {
        let (done, wait) = mpsc::channel();
        self.tx
            .send(Command::Speak {
                text: text.into(),
                lang: lang.into(),
                rate,
                done,
            })
            .map_err(|_| MeetCueError::new(codes::PRACTICE_TTS, "speech worker stopped"))?;
        wait.recv().unwrap_or_else(|_| {
            Err(MeetCueError::new(
                codes::PRACTICE_TTS,
                "speech worker stopped",
            ))
        })
    }

    /// Interrupts speech in progress (e.g. the user started answering).
    pub fn stop(&self) {
        if let Some(t) = self.stop.lock().unwrap().as_mut() {
            let _ = t.stop();
        }
    }
}

impl Default for Speaker {
    fn default() -> Self {
        Self::new()
    }
}

fn speak_blocking(t: &mut tts::Tts, text: &str, lang: &str, rate: f32) -> Result<(), MeetCueError> {
    if let Ok(voices) = t.voices() {
        if let Some(v) = voices
            .iter()
            .find(|v| v.language().primary_language().eq_ignore_ascii_case(lang))
        {
            let _ = t.set_voice(v);
        }
    }
    let normal = t.normal_rate();
    let target = (normal * rate.clamp(0.5, 2.0)).clamp(t.min_rate(), t.max_rate());
    let _ = t.set_rate(target);
    t.speak(text, true).map_err(tts_err)?;
    // Wait for the utterance to finish.
    thread::sleep(Duration::from_millis(150));
    let mut waited = 0;
    while t.is_speaking().unwrap_or(false) && waited < 120_000 {
        thread::sleep(Duration::from_millis(50));
        waited += 50;
    }
    Ok(())
}
