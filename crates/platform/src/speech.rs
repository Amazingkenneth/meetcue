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
        /// Id of the system voice to use; the first voice of `lang` when
        /// empty or no longer installed.
        voice: String,
        done: mpsc::Sender<Result<(), MeetCueError>>,
    },
    Voices {
        done: mpsc::Sender<Vec<VoiceInfo>>,
    },
}

/// A system text-to-speech voice, as offered in the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceInfo {
    pub id: String,
    pub name: String,
    /// Primary language, such as `en` or `zh`.
    pub lang: String,
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
                        voice,
                        done,
                    } => {
                        let result = match engine.as_mut() {
                            None => Err(MeetCueError::new(codes::PRACTICE_TTS, "no speech engine")),
                            Some(t) => speak_blocking(t, &text, &lang, rate, &voice),
                        };
                        let _ = done.send(result);
                    }
                    Command::Voices { done } => {
                        let _ = done.send(engine.as_ref().map(list_voices).unwrap_or_default());
                    }
                }
            }
        });
        Self { tx, stop }
    }

    /// The system voices available for speech synthesis.
    pub fn voices(&self) -> Vec<VoiceInfo> {
        let (done, wait) = mpsc::channel();
        if self.tx.send(Command::Voices { done }).is_err() {
            return Vec::new();
        }
        wait.recv().unwrap_or_default()
    }

    /// Speaks and blocks until finished (or stopped). `lang` is `zh` or `en`;
    /// `voice` is a voice id from [`Speaker::voices`], or empty for the first
    /// voice of that language.
    pub fn speak(
        &self,
        text: &str,
        lang: &str,
        rate: f32,
        voice: &str,
    ) -> Result<(), MeetCueError> {
        let (done, wait) = mpsc::channel();
        self.tx
            .send(Command::Speak {
                text: text.into(),
                lang: lang.into(),
                rate,
                voice: voice.into(),
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

fn list_voices(t: &tts::Tts) -> Vec<VoiceInfo> {
    t.voices()
        .unwrap_or_default()
        .iter()
        .map(|v| VoiceInfo {
            id: v.id(),
            name: v.name(),
            lang: v.language().primary_language().to_string(),
        })
        .collect()
}

fn speak_blocking(
    t: &mut tts::Tts,
    text: &str,
    lang: &str,
    rate: f32,
    voice: &str,
) -> Result<(), MeetCueError> {
    if let Ok(voices) = t.voices() {
        // The chosen voice, else the first one of the text's language.
        let chosen = voices
            .iter()
            .find(|v| !voice.is_empty() && v.id() == voice)
            .or_else(|| {
                voices
                    .iter()
                    .find(|v| v.language().primary_language().eq_ignore_ascii_case(lang))
            });
        if let Some(v) = chosen {
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
