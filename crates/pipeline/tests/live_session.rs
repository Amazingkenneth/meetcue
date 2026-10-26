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
    /// The next this many opens fail as an unreachable provider would.
    fail_opens: u32,
    /// Opens never complete, as with a provider that accepts the connection
    /// and then says nothing.
    hang: bool,
    /// Streams end right after opening, as when the provider rejects a setting.
    end_at_once: bool,
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
        if self.log().hang {
            std::future::pending::<()>().await;
        }
        let (tx, rx) = mpsc::unbounded();
        let mut log = self.log();
        if log.fail_opens > 0 {
            log.fail_opens -= 1;
            return Err(MeetCueError::new(
                meetcue_core::codes::ASR_CONNECT,
                "connection timed out",
            ));
        }
        log.opens += 1;
        if !log.end_at_once {
            log.senders.push(tx);
        }
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
        // Each stream runs a while before the provider ends it.
        tokio::time::sleep(Duration::from_secs(15)).await;
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

#[tokio::test(start_paused = true)]
async fn keeps_the_stream_alive_when_no_audio_arrives() {
    // System sound capture delivers nothing while nothing plays; the
    // provider must still hear from us, or it ends the stream (Soniox after
    // 20 s with a 408, shown as MC-ASR-002).
    let asr = RecordingAsr::default();
    let backend = started(&asr).await;
    tokio::time::sleep(Duration::from_secs(30)).await;
    {
        let log = asr.log();
        assert!(log.keepalives >= 5, "keep-alives: {}", log.keepalives);
        assert_eq!(log.opens, 1);
    }
    backend.stop().await;
}

#[tokio::test(start_paused = true)]
async fn notices_a_stream_the_provider_ended_without_audio() {
    let asr = RecordingAsr::default();
    let backend = started(&asr).await;
    asr.close_streams();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(asr.log().opens, 2, "reopened without waiting for audio");
    backend.stop().await;
}

#[tokio::test(start_paused = true)]
async fn an_unreachable_provider_does_not_hold_up_stopping() {
    let asr = RecordingAsr::default();
    asr.log().hang = true;
    let backend = started(&asr).await;
    push(&backend, 1, 10, true).await;
    let stopped = tokio::time::timeout(Duration::from_secs(3), backend.stop()).await;
    assert!(stopped.is_ok(), "stop waited for the connection attempt");
}

#[tokio::test(start_paused = true)]
async fn reports_an_outage_once_and_when_recognition_is_back() {
    let asr = RecordingAsr::default();
    asr.log().fail_opens = 6;
    let backend = LocalHintBackend::new();
    let mut events = backend.events();
    backend
        .start(config(&asr, "a", model("anthropic", "claude-haiku-5-5")))
        .await
        .unwrap();
    // Speech during the outage is kept flowing in, then sent once connected.
    for _ in 0..30 {
        push(&backend, 1, 50, false).await;
    }
    push(&backend, 100, 3, true).await;
    let mut errors = 0;
    let mut status = Vec::new();
    while let Ok(e) = events.try_recv() {
        match e {
            MeetCueEvent::Error { code, .. } if code == meetcue_core::codes::ASR_CONNECT => {
                errors += 1
            }
            MeetCueEvent::Recognition { channel, connected } => {
                assert_eq!(channel, Channel::Remote);
                status.push(connected);
            }
            _ => {}
        }
    }
    assert_eq!(errors, 1, "one message for the whole outage");
    assert_eq!(status, [false, true]);
    assert_eq!(asr.log().opens, 1);
    assert!(
        asr.log().writes.contains(&102),
        "speech after the outage is recognised"
    );
    backend.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_stream_that_keeps_ending_at_once_is_retried_with_a_growing_wait() {
    let asr = RecordingAsr::default();
    asr.log().end_at_once = true;
    let backend = started(&asr).await;
    // Twenty seconds of audio: one frame every 20 ms.
    for _ in 0..20 {
        push(&backend, 1, 50, true).await;
    }
    let opens = asr.log().opens;
    assert!((3..=7).contains(&opens), "{opens} opens in 20 s");
    backend.stop().await;
}

#[tokio::test(start_paused = true)]
async fn input_levels_reach_the_meters() {
    let asr = RecordingAsr::default();
    let backend = LocalHintBackend::new();
    let mut events = backend.events();
    backend
        .start(config(&asr, "a", model("anthropic", "claude-haiku-5-5")))
        .await
        .unwrap();
    for i in 0..50 {
        for (channel, level_db) in [(Channel::Me, -30.0), (Channel::Remote, -100.0)] {
            let frame = AudioFrame {
                channel,
                samples: vec![i; 320],
                capture_ts_ms: 0,
                voiced: level_db > -45.0,
                level_db,
            };
            backend.push_audio(channel, frame).await;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut loudest = f32::MIN;
    while let Ok(e) = events.try_recv() {
        if let MeetCueEvent::Levels { me, .. } = e {
            loudest = loudest.max(me);
        }
    }
    assert_eq!(loudest, -30.0, "the microphone level reaches the panel");
    backend.stop().await;
}
