//! Records the SDN demo session as timed events for the browser preview.
//!
//! `cargo run -p meetcue-pipeline --example record_demo` writes
//! `app/ui/src/mock/sdn-demo.json`. The mock backend of the UI replays this
//! file, so the preview shows exactly what the real pipeline emits.

use std::sync::Arc;
use std::time::Duration;

use meetcue_core::TimedEvent;
use meetcue_pipeline::backend::now_ms;
use meetcue_pipeline::engine::{EngineConfig, ModelRef};
use meetcue_pipeline::providers::mock::{MockAsr, MockLlm};
use meetcue_pipeline::{HintBackend, LocalHintBackend, SessionConfig};

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .expect("runtime");
    let events = rt.block_on(async {
        let backend = LocalHintBackend::new();
        let mut rx = backend.events();
        let llm = Arc::new(MockLlm::sdn());
        let model = ModelRef {
            provider: "anthropic".into(),
            model: "claude-haiku-5-5".into(),
        };
        let engine = EngineConfig {
            detect: model.clone(),
            hints: model,
            ..Default::default()
        };
        let mut cfg = SessionConfig::new(engine, Some(Arc::new(MockAsr::sdn())), llm.clone(), llm);
        cfg.asr_model = ModelRef {
            provider: "soniox".into(),
            model: "stt-rt-v5".into(),
        };
        let start = now_ms();
        let collector = tokio::spawn(async move {
            let mut out = Vec::new();
            while let Ok(event) = rx.recv().await {
                out.push(TimedEvent {
                    t_ms: now_ms() - start,
                    event,
                });
            }
            out
        });
        backend.start(cfg).await.expect("start");
        tokio::time::sleep(Duration::from_secs(70)).await;
        backend.stop().await;
        drop(backend);
        collector.await.expect("collector")
    });
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../app/ui/src/mock/sdn-demo.json");
    let json = serde_json::to_string(&events).expect("serialize");
    std::fs::write(path, json + "\n").expect("write demo file");
    println!("wrote {} events to {path}", events.len());
}
