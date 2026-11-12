//! Live checks against real provider APIs. Ignored by default and never run
//! in CI: they cost money and need keys. Run manually with, for example,
//!
//! ```sh
//! MEETCUE_ANTHROPIC_KEY=... cargo test -p meetcue-pipeline --test live_providers -- --ignored
//! ```

use futures::StreamExt;
use meetcue_core::UsagePurpose;
use meetcue_pipeline::provider::{AsrOptions, AsrResult, LlmChunk, LlmRequest};
use meetcue_pipeline::providers::{self, ProviderConfig};

fn cfg(provider: &str, env: &str, model: &str) -> Option<ProviderConfig> {
    let key = std::env::var(env).ok().filter(|k| !k.is_empty())?;
    Some(ProviderConfig {
        provider: provider.into(),
        model: model.into(),
        key: Some(key),
        ..Default::default()
    })
}

async fn llm_roundtrip(c: ProviderConfig) {
    let llm = providers::llm(&c).unwrap();
    let models = llm.list_models().await.unwrap();
    assert!(!models.is_empty());
    let req = LlmRequest::new(&c.model, UsagePurpose::Probe)
        .user("Reply with the single word: ready")
        .max_tokens(20);
    let mut s = llm.stream(req).await.unwrap();
    let mut text = String::new();
    let mut usage = false;
    while let Some(c) = s.next().await {
        match c.unwrap() {
            LlmChunk::Text(t) => text.push_str(&t),
            LlmChunk::Usage { .. } => usage = true,
        }
    }
    assert!(text.to_lowercase().contains("ready"), "{text}");
    assert!(usage);
}

#[tokio::test]
#[ignore]
async fn anthropic_live() {
    if let Some(c) = cfg("anthropic", "MEETCUE_ANTHROPIC_KEY", "claude-haiku-5-5") {
        llm_roundtrip(c).await;
    }
}

async fn asr_roundtrip(c: ProviderConfig) {
    let asr = providers::asr(&c).unwrap();
    asr.verify().await.unwrap();
    let audio = meetcue_pipeline::textio::wav_pcm16(include_bytes!(
        "../../../samples/audio/probe-zh-en.wav"
    ))
    .unwrap();
    let mut stream = asr
        .open(AsrOptions {
            model: c.model.clone(),
            languages: vec!["zh".into(), "en".into()],
            endpoint_silence_ms: 700,
            ..Default::default()
        })
        .await
        .unwrap();
    let mut results = stream.results();
    for chunk in audio.chunks(320) {
        stream.write(chunk, 0).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    stream.finalize().await.unwrap();
    let mut text = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while let Ok(Some(r)) = tokio::time::timeout_at(deadline, results.next()).await {
        match r {
            AsrResult::Final { text: t, .. } => text.push_str(&t),
            AsrResult::Endpoint { .. } if !text.is_empty() => break,
            AsrResult::Error(e) => panic!("{e}"),
            _ => {}
        }
    }
    let _ = stream.close().await;
    assert!(!text.is_empty());
    println!("recognised: {text}");
}

#[tokio::test]
#[ignore]
async fn soniox_live() {
    if let Some(c) = cfg("soniox", "MEETCUE_SONIOX_KEY", "stt-rt-v5") {
        asr_roundtrip(c).await;
    }
}

#[tokio::test]
#[ignore]
async fn deepgram_live() {
    if let Some(c) = cfg("deepgram", "MEETCUE_DEEPGRAM_KEY", "nova-3") {
        asr_roundtrip(c).await;
    }
}

async fn complete(c: &ProviderConfig, req: LlmRequest) -> String {
    let llm = providers::llm(c).unwrap();
    let mut s = llm.stream(req).await.unwrap();
    let mut text = String::new();
    while let Some(c) = s.next().await {
        if let LlmChunk::Text(t) = c.unwrap() {
            text.push_str(&t);
        }
    }
    text
}

/// A question cut by a pause right after its first words: the detection
/// model counts the piece before it as its first half and titles the whole
/// question, and the title model titles the joined question as a whole.
#[tokio::test]
#[ignore]
async fn anthropic_keeps_the_start_of_a_cut_question() {
    use meetcue_pipeline::detect;
    let Some(c) = cfg("anthropic", "MEETCUE_ANTHROPIC_KEY", "claude-haiku-5-5") else {
        return;
    };
    let transcript = "[Them] 你好，我们开始吧。\n[Them, 0.6 s pause] 上任后。\n[Them, 0.4 s pause] 一年的计划是什么？\n";
    let reply = complete(
        &c,
        detect::request(&c.model, transcript, "Simplified Chinese"),
    )
    .await;
    let d = detect::parse(&reply).unwrap_or_else(|| panic!("{reply}"));
    assert!(d.is_question, "{reply}");
    assert_eq!(d.lead_in, 1, "{reply}");
    assert!(d.short.contains("上任"), "{reply}");

    let reply = complete(
        &c,
        detect::title_request(
            &c.model,
            "如何制定数据中心IP。V6的迁移方案，同时保证。存量用户不受影响？",
            "Simplified Chinese",
        ),
    )
    .await;
    let d = detect::parse(&reply).unwrap_or_else(|| panic!("{reply}"));
    assert!(
        d.short.contains("IPv6") || d.short.contains("IPV6"),
        "{reply}"
    );
}
