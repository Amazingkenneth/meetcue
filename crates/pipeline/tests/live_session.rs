//! Recognition and model handling in a running session, through the real
//! async backend on tokio's paused clock: pre-roll, keep-alives while paused,
//! reconnecting when the provider closes the stream, and switching models
//! during a session.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures::channel::mpsc;
use futures::stream::BoxStream;
use futures::StreamExt;

use meetcue_core::{AudioFrame, Channel, MeetCueError, MeetCueEvent, UsagePurpose, UserAction};
use meetcue_pipeline::engine::{EngineConfig, ModelRef};
use meetcue_pipeline::provider::{AsrOptions, AsrProvider, AsrResult, AsrStream, AudioUsage};
use meetcue_pipeline::providers::mock::MockLlm;
use meetcue_pipeline::{HintBackend, LocalHintBackend, SessionConfig};

#[derive(Default)]
struct Log {
    opens: u32,
    /// The tag (first sample) of every frame written, in order.
    writes: Vec<i16>,
    keepalives: u32,
    finalizes: u32,
    closes: u32,
    /// Result senders of the open streams; dropping them ends the stream as
    /// a provider closing the connection would.
    senders: Vec<mpsc::UnboundedSender<AsrResult>>,
}

#[derive(Clone, Default)]
struct RecordingAsr {
    log: Arc<Mutex<Log>>,
}

impl RecordingAsr {
    fn log(&self) -> std::sync::MutexGuard<'_, Log> {
        self.log.lock().unwrap()
    }

    /// The provider closes every open stream.
    fn close_streams(&self) {
        self.log().senders.clear();
    }
}

#[async_trait]
impl AsrProvider for RecordingAsr {
    fn id(&self) -> &'static str {
        "mock"
    }

    async fn open(&self, _opts: AsrOptions) -> Result<Box<dyn AsrStream>, MeetCueError> {
        let (tx, rx) = mpsc::unbounded();
        let mut log = self.log();
        log.opens += 1;
        log.senders.push(tx);
        Ok(Box::new(RecordingStream {
            log: self.log.clone(),
            rx: Some(rx),
        }))
    }
}

struct RecordingStream {
    log: Arc<Mutex<Log>>,
    rx: Option<mpsc::UnboundedReceiver<AsrResult>>,
}

#[async_trait]
impl AsrStream for RecordingStream {
    async fn write(&mut self, pcm16k: &[i16], _ts: i64) -> Result<(), MeetCueError> {
        self.log.lock().unwrap().writes.push(pcm16k[0]);
        Ok(())
    }

    fn results(&mut self) -> BoxStream<'static, AsrResult> {
        self.rx
            .take()
            .map(|r| r.boxed())
            .unwrap_or_else(|| futures::stream::empty().boxed())
    }

    async fn keepalive(&mut self) -> Result<(), MeetCueError> {
        self.log.lock().unwrap().keepalives += 1;
        Ok(())
    }

    async fn finalize(&mut self) -> Result<(), MeetCueError> {
        self.log.lock().unwrap().finalizes += 1;
        Ok(())
    }

    async fn close(self: Box<Self>) -> Result<AudioUsage, MeetCueError> {
        self.log.lock().unwrap().closes += 1;
        Ok(AudioUsage::default())
    }
}

fn model(provider: &str, model: &str) -> ModelRef {
    ModelRef {
        provider: provider.into(),
        model: model.into(),
    }
}

fn config(asr: &RecordingAsr, signature: &str, hints: ModelRef) -> SessionConfig {
    let llm = Arc::new(MockLlm::sdn());
    let engine = EngineConfig {
        detect: hints.clone(),
        hints,
        ..Default::default()
    };
    let mut cfg = SessionConfig::new(engine, Some(Arc::new(asr.clone())), llm.clone(), llm);
    cfg.asr_model = model("mock", signature);
    cfg.asr_signature = signature.into();
    // Only the other side, to keep the recordings simple.
    cfg.recognize_me = false;
    cfg
}

/// Pushes `n` frames of 20 ms tagged `tag, tag + 1, …`, in real (paused) time.
async fn push(backend: &LocalHintBackend, tag: i16, n: i16, voiced: bool) {
    for i in 0..n {
        let frame = AudioFrame {
            channel: Channel::Remote,
            samples: vec![tag + i; 320],
            capture_ts_ms: 0,
            voiced,
            level_db: if voiced { -20.0 } else { -70.0 },
        };
        backend.push_audio(Channel::Remote, frame).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn started(asr: &RecordingAsr) -> LocalHintBackend {
    let backend = LocalHintBackend::new();
    backend
        .start(config(asr, "a", model("anthropic", "claude-haiku-5-5")))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    backend
}

#[tokio::test(start_paused = true)]
async fn speech_onset_is_sent_with_the_preceding_300_ms() {
    let asr = RecordingAsr::default();
    let backend = started(&asr).await;
    push(&backend, 1, 30, false).await;
    push(&backend, 100, 3, true).await;
    let writes = asr.log().writes.clone();
    let expected: Vec<i16> = (16..=30).chain(100..=102).collect();
    assert_eq!(
        writes, expected,
        "15 silent frames (300 ms) lead the speech"
    );
    backend.stop().await;
}

#[tokio::test(start_paused = true)]
async fn pausing_keeps_the_connection_alive_without_sending_audio() {
    let asr = RecordingAsr::default();
    let backend = started(&asr).await;
    push(&backend, 1, 2, true).await;
    // Silence before pausing fills the pre-roll buffer.
    push(&backend, 500, 80, false).await;
    backend.send(UserAction::Pause).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    // Ten seconds of speech while paused: nothing is sent, the stream is kept alive.
    push(&backend, 1000, 500, true).await;
    {
        let log = asr.log();
        assert!(
            log.writes.iter().all(|t| *t < 1000),
            "no audio while paused"
        );
        assert!(log.keepalives >= 9, "keep-alives: {}", log.keepalives);
        assert_eq!(log.opens, 1, "the connection stayed open");
    }
    backend.send(UserAction::Resume).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    push(&backend, 2000, 3, true).await;
    assert!(
        asr.log().writes.contains(&2000),
        "audio flows again after resuming"
    );
    backend.stop().await;
}

#[tokio::test(start_paused = true)]
async fn reconnects_every_time_the_provider_closes_the_stream() {
    let asr = RecordingAsr::default();
    let backend = started(&asr).await;
    for round in 0..4i16 {
        push(&backend, round * 100, 3, true).await;
        assert!(
            asr.log().writes.contains(&(round * 100 + 2)),
            "round {round} recognised"
        );
        asr.close_streams();
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(asr.log().opens, 4, "one reconnect per close, without limit");
    backend.stop().await;
}

#[tokio::test(start_paused = true)]
async fn switching_recognition_during_a_session_reconnects_live() {
    let a = RecordingAsr::default();
    let b = RecordingAsr::default();
    let backend = started(&a).await;
    push(&backend, 1, 3, true).await;

    backend
        .reconfigure(config(&b, "b", model("anthropic", "claude-haiku-5-5")))
        .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    push(&backend, 100, 3, true).await;
    {
        let (a, b) = (a.log(), b.log());
        assert!(
            a.finalizes >= 1,
            "pending text was flushed before switching"
        );
        assert!(a.closes >= 1);
        assert!(!a.writes.contains(&100), "nothing goes to the old provider");
        assert_eq!(b.opens, 1);
        assert!(b.writes.contains(&100));
    }

    // Saving unrelated settings doesn't reconnect.
    backend
        .reconfigure(config(&b, "b", model("anthropic", "claude-sonnet-5-5")))
        .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    push(&backend, 200, 3, true).await;
    assert_eq!(b.log().opens, 1);
    backend.stop().await;
}

#[tokio::test(start_paused = true)]
async fn switching_the_hint_model_applies_to_the_next_question() {
    let asr = RecordingAsr::default();
    let backend = started(&asr).await;
    let mut events = backend.events();
    backend
        .reconfigure(config(&asr, "a", model("anthropic", "claude-sonnet-5-5")))
        .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    backend
        .inject_question("你们的 SDN 控制器是怎么做高可用的？".into(), 0)
        .await;
    tokio::time::sleep(Duration::from_secs(10)).await;
    let mut hint_models = Vec::new();
    while let Ok(e) = events.try_recv() {
        if let MeetCueEvent::Usage {
            model,
            purpose: UsagePurpose::Hints,
            ..
        } = e
        {
            hint_models.push(model);
        }
    }
    assert!(!hint_models.is_empty(), "hints were generated");
    assert!(
        hint_models.iter().all(|m| m == "claude-sonnet-5-5"),
        "{hint_models:?}"
    );
    backend.stop().await;
}
