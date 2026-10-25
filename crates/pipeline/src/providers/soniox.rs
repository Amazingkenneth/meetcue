//! Soniox real-time speech recognition.
//!
//! WebSocket `wss://<host>/transcribe-websocket`: authenticate with a bearer
//! header, send one JSON configuration message, then binary PCM. Results are
//! token lists; final tokens never change, `<end>` marks an endpoint and
//! `<fin>` answers a `finalize` request. REST: `https://<api host>/v1/models`.

use async_trait::async_trait;
use futures::stream::{BoxStream, SplitSink};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

use meetcue_core::{codes, MeetCueError};

use crate::endpoints::EndpointCatalog;
use crate::net::{self, Ws};
use crate::provider::{AsrOptions, AsrProvider, AsrResult, AsrStream, AudioUsage, ModelInfo};

pub const DEFAULT_HOST: &str = "stt-rt.soniox.com";
const END: &str = "<end>";
const FIN: &str = "<fin>";

pub struct Soniox {
    key: String,
    host: String,
    proxy: Option<String>,
}

impl Soniox {
    pub fn new(key: String, host: Option<String>, proxy: Option<String>) -> Self {
        Self {
            key,
            host: host.unwrap_or_else(|| DEFAULT_HOST.into()),
            proxy,
        }
    }

    fn api_base(&self) -> String {
        format!(
            "https://{}/v1",
            EndpointCatalog::builtin().api_host("soniox", &self.host)
        )
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, MeetCueError> {
        let client = net::http_client(self.proxy.as_deref())?;
        let resp = client
            .get(format!("{}/models", self.api_base()))
            .bearer_auth(&self.key)
            .send()
            .await
            .map_err(|e| MeetCueError::new(codes::ASR_CONNECT, e.to_string()))?;
        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(MeetCueError::new(
                asr_status_code(status),
                format!("HTTP {status}: {body}"),
            ));
        }
        #[derive(Deserialize)]
        struct Model {
            id: String,
            name: Option<String>,
            transcription_mode: Option<String>,
        }
        #[derive(Deserialize)]
        struct Models {
            models: Vec<Model>,
        }
        let list: Models = resp
            .json()
            .await
            .map_err(|e| MeetCueError::new(codes::ASR_PROTOCOL, e.to_string()))?;
        Ok(list
            .models
            .into_iter()
            .filter(|m| m.transcription_mode.as_deref() != Some("async"))
            .map(|m| ModelInfo {
                display_name: m.name.unwrap_or_else(|| m.id.clone()),
                id: m.id,
                price: None,
            })
            .collect())
    }
}

pub fn asr_status_code(status: u16) -> &'static str {
    match status {
        // A parameter the service doesn't accept (model, language, option):
        // retrying or checking the network won't help.
        400 => codes::ASR_PROTOCOL,
        401 | 403 => codes::ASR_AUTH,
        402 | 429 => codes::ASR_RATE_LIMIT,
        _ => codes::ASR_CONNECT,
    }
}

/// The configuration message sent right after connecting.
pub fn config_message(opts: &AsrOptions) -> serde_json::Value {
    let mut cfg = json!({
        "model": if opts.model.is_empty() { "stt-rt-v5" } else { opts.model.as_str() },
        "audio_format": "pcm_s16le",
        "sample_rate": 16_000,
        "num_channels": 1,
        "enable_endpoint_detection": true,
        "language_hints": opts.languages,
    });
    if opts.endpoint_silence_ms > 0 {
        // Soniox accepts 500–3000 ms.
        cfg["max_endpoint_delay_ms"] = json!(opts.endpoint_silence_ms.clamp(500, 3000));
    }
    if !opts.hotwords.is_empty() {
        cfg["context"] = json!({ "terms": opts.hotwords });
    }
    cfg
}

#[derive(Debug, Deserialize)]
struct Token {
    text: String,
    #[serde(default)]
    start_ms: Option<i64>,
    #[serde(default)]
    end_ms: Option<i64>,
    #[serde(default)]
    is_final: bool,
}

#[derive(Debug, Deserialize)]
struct Response {
    #[serde(default)]
    tokens: Vec<Token>,
    #[serde(default)]
    error_code: Option<serde_json::Value>,
    #[serde(default)]
    error_message: Option<String>,
}

/// Turns one server message into recognition results.
pub fn parse_message(text: &str) -> Vec<AsrResult> {
    let resp: Response = match serde_json::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            return vec![AsrResult::Error(MeetCueError::new(
                codes::ASR_PROTOCOL,
                e.to_string(),
            ))];
        }
    };
    let mut out = Vec::new();
    if resp.error_code.is_some() || resp.error_message.is_some() {
        let status = resp
            .error_code
            .as_ref()
            .and_then(|c| c.as_u64())
            .unwrap_or(0) as u16;
        out.push(AsrResult::Error(MeetCueError::new(
            asr_status_code(status),
            format!("{} {}", status, resp.error_message.unwrap_or_default()),
        )));
        return out;
    }
    let mut final_text = String::new();
    let (mut f0, mut f1) = (None, 0i64);
    let mut partial = String::new();
    let (mut p0, mut p1) = (None, 0i64);
    for t in &resp.tokens {
        if t.is_final && (t.text == END || t.text == FIN) {
            if !final_text.is_empty() {
                out.push(AsrResult::Final {
                    text: std::mem::take(&mut final_text),
                    t0_ms: f0.take().unwrap_or(f1),
                    t1_ms: f1,
                });
            }
            out.push(AsrResult::Endpoint {
                t_ms: t.end_ms.or(t.start_ms).unwrap_or(f1),
            });
        } else if t.is_final {
            final_text.push_str(&t.text);
            f0.get_or_insert(t.start_ms.unwrap_or(f1));
            f1 = t.end_ms.unwrap_or(f1);
        } else {
            partial.push_str(&t.text);
            p0.get_or_insert(t.start_ms.unwrap_or(p1));
            p1 = t.end_ms.unwrap_or(p1);
        }
    }
    if !final_text.is_empty() {
        out.push(AsrResult::Final {
            text: final_text,
            t0_ms: f0.unwrap_or(f1),
            t1_ms: f1,
        });
    }
    if !partial.is_empty() {
        out.push(AsrResult::Partial {
            text: partial,
            t0_ms: p0.unwrap_or(p1),
            t1_ms: p1,
        });
    }
    out
}

#[async_trait]
impl AsrProvider for Soniox {
    fn id(&self) -> &'static str {
        "soniox"
    }

    async fn open(&self, opts: AsrOptions) -> Result<Box<dyn AsrStream>, MeetCueError> {
        let host = opts.host.clone().unwrap_or_else(|| self.host.clone());
        let url = format!("wss://{host}/transcribe-websocket");
        let proxy = opts.proxy.clone().or_else(|| self.proxy.clone());
        let (ws, _) = net::websocket(
            &url,
            &[("Authorization", format!("Bearer {}", self.key))],
            proxy.as_deref(),
        )
        .await?;
        let (mut sink, mut stream) = ws.split();
        sink.send(Message::text(config_message(&opts).to_string()))
            .await
            .map_err(|e| MeetCueError::new(codes::ASR_CONNECT, e.to_string()))?;
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let reader = tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(Message::Text(t)) => {
                        for r in parse_message(t.as_str()) {
                            if tx.unbounded_send(r).is_err() {
                                return;
                            }
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        if let Some(f) = frame.filter(|f| u16::from(f.code) != 1000) {
                            let _ = tx.unbounded_send(AsrResult::Error(MeetCueError::new(
                                codes::ASR_CONNECT,
                                f.reason.to_string(),
                            )));
                        }
                        return;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        let _ = tx.unbounded_send(AsrResult::Error(MeetCueError::new(
                            codes::ASR_CONNECT,
                            e.to_string(),
                        )));
                        return;
                    }
                }
            }
        });
        Ok(Box::new(SonioxStream {
            sink,
            rx: Some(rx),
            reader,
            sent_ms: 0,
        }))
    }

    async fn verify(&self) -> Result<(), MeetCueError> {
        self.models().await.map(|_| ())
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, MeetCueError> {
        self.models().await
    }
}

struct SonioxStream {
    sink: SplitSink<Ws, Message>,
    rx: Option<futures::channel::mpsc::UnboundedReceiver<AsrResult>>,
    reader: tokio::task::JoinHandle<()>,
    sent_ms: u64,
}

// A stream dropped without `close` (an abandoned connection attempt, a
// session that was cut short) must not leave its socket open.
impl Drop for SonioxStream {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

fn send_err(e: impl std::fmt::Display) -> MeetCueError {
    MeetCueError::new(codes::ASR_CONNECT, e.to_string())
}

pub fn pcm_bytes(pcm: &[i16]) -> Vec<u8> {
    pcm.iter().flat_map(|s| s.to_le_bytes()).collect()
}

#[async_trait]
impl AsrStream for SonioxStream {
    async fn write(&mut self, pcm16k: &[i16], _capture_ts_ms: i64) -> Result<(), MeetCueError> {
        self.sink
            .send(Message::binary(pcm_bytes(pcm16k)))
            .await
            .map_err(send_err)?;
        self.sent_ms += pcm16k.len() as u64 / 16;
        Ok(())
    }

    fn results(&mut self) -> BoxStream<'static, AsrResult> {
        match self.rx.take() {
            Some(rx) => rx.boxed(),
            None => futures::stream::empty().boxed(),
        }
    }

    async fn keepalive(&mut self) -> Result<(), MeetCueError> {
        self.sink
            .send(Message::text(r#"{"type":"keepalive"}"#))
            .await
            .map_err(send_err)
    }

    async fn finalize(&mut self) -> Result<(), MeetCueError> {
        self.sink
            .send(Message::text(r#"{"type":"finalize"}"#))
            .await
            .map_err(send_err)
    }

    async fn close(mut self: Box<Self>) -> Result<AudioUsage, MeetCueError> {
        // An empty message ends the audio; then close the socket.
        let _ = self.sink.send(Message::text("")).await;
        let _ = self.sink.close().await;
        self.reader.abort();
        Ok(AudioUsage {
            audio_ms: self.sent_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_has_format_languages_and_terms() {
        let opts = AsrOptions {
            model: "stt-rt-v5".into(),
            languages: vec!["zh".into(), "en".into()],
            hotwords: vec!["OpenFlow".into()],
            endpoint_silence_ms: 200,
            ..Default::default()
        };
        let c = config_message(&opts);
        assert_eq!(c["audio_format"], "pcm_s16le");
        assert_eq!(c["sample_rate"], 16000);
        assert_eq!(c["language_hints"], json!(["zh", "en"]));
        assert_eq!(c["context"]["terms"], json!(["OpenFlow"]));
        assert_eq!(c["max_endpoint_delay_ms"], 500);
        assert_eq!(c["enable_endpoint_detection"], true);
        assert!(
            c.get("api_key").is_none(),
            "the key goes in the header only"
        );
    }

    #[test]
    fn parses_partial_final_and_endpoint() {
        let msg = r#"{"tokens":[
            {"text":"你们","start_ms":100,"end_ms":300,"is_final":true},
            {"text":"怎么","start_ms":300,"end_ms":500,"is_final":true},
            {"text":"做","start_ms":500,"end_ms":600,"is_final":false}
        ],"final_audio_proc_ms":500,"total_audio_proc_ms":700}"#;
        assert_eq!(
            parse_message(msg),
            vec![
                AsrResult::Final {
                    text: "你们怎么".into(),
                    t0_ms: 100,
                    t1_ms: 500
                },
                AsrResult::Partial {
                    text: "做".into(),
                    t0_ms: 500,
                    t1_ms: 600
                },
            ]
        );
        let msg = r#"{"tokens":[{"text":"？","start_ms":900,"end_ms":950,"is_final":true},{"text":"<end>","is_final":true},{"text":"And","start_ms":1500,"end_ms":1600,"is_final":false}]}"#;
        assert_eq!(
            parse_message(msg),
            vec![
                AsrResult::Final {
                    text: "？".into(),
                    t0_ms: 900,
                    t1_ms: 950
                },
                AsrResult::Endpoint { t_ms: 950 },
                AsrResult::Partial {
                    text: "And".into(),
                    t0_ms: 1500,
                    t1_ms: 1600
                },
            ]
        );
        assert_eq!(
            parse_message(r#"{"tokens":[{"text":"<fin>","is_final":true}]}"#),
            vec![AsrResult::Endpoint { t_ms: 0 }]
        );
        assert!(parse_message(r#"{"tokens":[]}"#).is_empty());
    }

    #[test]
    fn maps_errors() {
        match &parse_message(r#"{"tokens":[],"error_code":401,"error_message":"Invalid API key"}"#)
            [0]
        {
            AsrResult::Error(e) => assert_eq!(e.code, codes::ASR_AUTH),
            other => panic!("{other:?}"),
        }
        match &parse_message(r#"{"tokens":[],"error_code":402,"error_message":"No credit"}"#)[0] {
            AsrResult::Error(e) => assert_eq!(e.code, codes::ASR_RATE_LIMIT),
            other => panic!("{other:?}"),
        }
        assert!(matches!(parse_message("not json")[0], AsrResult::Error(_)));
        // An idle stream ends with a timeout: a connection problem. A bad
        // parameter isn't one.
        match &parse_message(r#"{"tokens":[],"error_code":408,"error_message":"Request timeout."}"#)
            [0]
        {
            AsrResult::Error(e) => assert_eq!(e.code, codes::ASR_CONNECT),
            other => panic!("{other:?}"),
        }
        match &parse_message(r#"{"tokens":[],"error_code":400,"error_message":"Invalid model."}"#)
            [0]
        {
            AsrResult::Error(e) => assert_eq!(e.code, codes::ASR_PROTOCOL),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn pcm_is_little_endian() {
        assert_eq!(pcm_bytes(&[1, -2]), vec![1, 0, 0xFE, 0xFF]);
    }
}
