//! Deepgram live transcription (`wss://<host>/v1/listen`).
//!
//! Options go in the query string; the key in `Authorization: Token …`.
//! Interim results carry the whole current segment, `is_final` fixes it,
//! `speech_final` / `UtteranceEnd` mark the end of a sentence, and a
//! `Finalize` message flushes pending audio.

use async_trait::async_trait;
use futures::stream::{BoxStream, SplitSink};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::tungstenite::Message;

use meetcue_core::{codes, MeetCueError};

use crate::net::{self, Ws};
use crate::provider::{AsrOptions, AsrProvider, AsrResult, AsrStream, AudioUsage, ModelInfo};
use crate::providers::soniox::{asr_status_code, pcm_bytes};

pub const DEFAULT_HOST: &str = "api.deepgram.com";

pub struct Deepgram {
    key: String,
    host: String,
    proxy: Option<String>,
}

impl Deepgram {
    pub fn new(key: String, host: Option<String>, proxy: Option<String>) -> Self {
        Self {
            key,
            host: host.unwrap_or_else(|| DEFAULT_HOST.into()),
            proxy,
        }
    }

    async fn get(&self, path: &str) -> Result<reqwest::Response, MeetCueError> {
        let client = net::http_client(self.proxy.as_deref())?;
        let resp = client
            .get(format!("https://{}{path}", self.host))
            .header("Authorization", format!("Token {}", self.key))
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
        Ok(resp)
    }
}

/// Deepgram language parameter for the configured speech languages, main
/// language first. Deepgram's code-switching mode (`multi`) doesn't cover
/// Chinese, so a list with Chinese uses one model for the main language:
/// Mandarin when Chinese comes first, English when English does.
pub fn language_param(languages: &[String]) -> &'static str {
    let is = |x: &String, l: &str| {
        x.eq_ignore_ascii_case(l) || x.to_lowercase().starts_with(&format!("{l}-"))
    };
    let has_zh = languages.iter().any(|x| is(x, "zh"));
    match languages.first() {
        None => "en",
        Some(first) if is(first, "zh") => "zh-CN",
        Some(first) if is(first, "en") && (languages.len() == 1 || has_zh) => "en",
        Some(_) if has_zh => "zh-CN",
        Some(_) => "multi",
    }
}

/// Whether Deepgram can recognise every configured language in one stream.
pub fn covers_all(languages: &[String]) -> bool {
    let has_zh = languages.iter().any(|x| x.to_lowercase().starts_with("zh"));
    !(has_zh && languages.len() > 1)
}

/// Deepgram sends each segment of a sentence as its own trimmed text. The
/// engine concatenates segments, so a segment that continues a sentence in a
/// spaced language starts with a space ("what is your" + " experience").
/// Chinese and Japanese text is joined without one.
fn segment(text: &str) -> String {
    let t = text.trim();
    match t.chars().next() {
        Some(c) if !crate::text::is_cjk(c) && !",.?!;:，。？！；：、".contains(c) => {
            format!(" {t}")
        }
        _ => t.to_string(),
    }
}

/// Builds the `/v1/listen` URL with all options.
pub fn listen_url(host: &str, opts: &AsrOptions) -> String {
    let model = if opts.model.is_empty() {
        "nova-3"
    } else {
        opts.model.as_str()
    };
    let mut q: Vec<(String, String)> = vec![
        ("model".into(), model.into()),
        ("language".into(), language_param(&opts.languages).into()),
        ("encoding".into(), "linear16".into()),
        ("sample_rate".into(), "16000".into()),
        ("channels".into(), "1".into()),
        ("interim_results".into(), "true".into()),
        ("punctuate".into(), "true".into()),
        ("smart_format".into(), "true".into()),
        (
            "endpointing".into(),
            opts.endpoint_silence_ms.clamp(10, 3000).to_string(),
        ),
        ("utterance_end_ms".into(), "1000".into()),
        ("vad_events".into(), "true".into()),
    ];
    for term in opts.hotwords.iter().take(50) {
        q.push(("keyterm".into(), term.clone()));
    }
    let query: Vec<String> = q
        .iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect();
    format!("wss://{host}/v1/listen?{}", query.join("&"))
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct Alternative {
    #[serde(default)]
    transcript: String,
}

#[derive(Debug, Deserialize)]
struct ChannelResult {
    #[serde(default)]
    alternatives: Vec<Alternative>,
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    start: f64,
    #[serde(default)]
    duration: f64,
    #[serde(default)]
    is_final: bool,
    #[serde(default)]
    speech_final: bool,
    #[serde(default)]
    from_finalize: bool,
    #[serde(default)]
    channel: Option<serde_json::Value>,
    #[serde(default)]
    last_word_end: Option<f64>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    variant: Option<String>,
}

fn ms(s: f64) -> i64 {
    (s * 1000.0).round() as i64
}

/// Joins words without spaces between Chinese characters.
fn tidy(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' '
            && i > 0
            && i + 1 < chars.len()
            && crate::text::is_cjk(chars[i - 1])
            && crate::text::is_cjk(chars[i + 1])
        {
            continue;
        }
        out.push(c);
    }
    out
}

pub fn parse_message(text: &str) -> Vec<AsrResult> {
    let env: Envelope = match serde_json::from_str(text) {
        Ok(e) => e,
        Err(e) => {
            return vec![AsrResult::Error(MeetCueError::new(
                codes::ASR_PROTOCOL,
                e.to_string(),
            ))]
        }
    };
    match env.kind.as_str() {
        "Results" => {
            let transcript = env
                .channel
                .and_then(|c| serde_json::from_value::<ChannelResult>(c).ok())
                .and_then(|c| c.alternatives.into_iter().next())
                .map(|a| segment(&tidy(a.transcript.trim())))
                .unwrap_or_default();
            let (t0, t1) = (ms(env.start), ms(env.start + env.duration));
            let mut out = Vec::new();
            if env.is_final {
                if !transcript.is_empty() {
                    out.push(AsrResult::Final {
                        text: transcript,
                        t0_ms: t0,
                        t1_ms: t1,
                    });
                }
                if env.speech_final || env.from_finalize {
                    out.push(AsrResult::Endpoint { t_ms: t1 });
                }
            } else if !transcript.is_empty() {
                out.push(AsrResult::Partial {
                    text: transcript,
                    t0_ms: t0,
                    t1_ms: t1,
                });
            }
            out
        }
        "UtteranceEnd" => vec![AsrResult::Endpoint {
            t_ms: ms(env.last_word_end.unwrap_or(0.0)),
        }],
        "Error" => vec![AsrResult::Error(MeetCueError::new(
            codes::ASR_PROTOCOL,
            format!(
                "{} {}",
                env.variant.unwrap_or_default(),
                env.description.unwrap_or_default()
            ),
        ))],
        _ => Vec::new(),
    }
}

#[async_trait]
impl AsrProvider for Deepgram {
    fn id(&self) -> &'static str {
        "deepgram"
    }

    async fn open(&self, opts: AsrOptions) -> Result<Box<dyn AsrStream>, MeetCueError> {
        let host = opts.host.clone().unwrap_or_else(|| self.host.clone());
        let proxy = opts.proxy.clone().or_else(|| self.proxy.clone());
        let (ws, _) = net::websocket(
            &listen_url(&host, &opts),
            &[("Authorization", format!("Token {}", self.key))],
            proxy.as_deref(),
        )
        .await?;
        let (sink, mut stream) = ws.split();
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
        Ok(Box::new(DeepgramStream {
            sink,
            rx: Some(rx),
            reader,
            sent_ms: 0,
        }))
    }

    async fn verify(&self) -> Result<(), MeetCueError> {
        self.get("/v1/projects").await.map(|_| ())
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, MeetCueError> {
        #[derive(Deserialize)]
        struct Model {
            name: Option<String>,
            canonical_name: Option<String>,
            #[serde(default)]
            streaming: bool,
        }
        #[derive(Deserialize)]
        struct Models {
            #[serde(default)]
            stt: Vec<Model>,
        }
        let list: Models = self
            .get("/v1/models")
            .await?
            .json()
            .await
            .map_err(|e| MeetCueError::new(codes::ASR_PROTOCOL, e.to_string()))?;
        let mut out: Vec<ModelInfo> = Vec::new();
        for m in list.stt.into_iter().filter(|m| m.streaming) {
            let Some(id) = m.canonical_name.clone().or(m.name.clone()) else {
                continue;
            };
            if !out.iter().any(|x| x.id == id) {
                out.push(ModelInfo {
                    display_name: m.name.unwrap_or_else(|| id.clone()),
                    id,
                    price: None,
                });
            }
        }
        Ok(out)
    }
}

struct DeepgramStream {
    sink: SplitSink<Ws, Message>,
    rx: Option<futures::channel::mpsc::UnboundedReceiver<AsrResult>>,
    reader: tokio::task::JoinHandle<()>,
    sent_ms: u64,
}

// A stream dropped without `close` (an abandoned connection attempt, a
// session that was cut short) must not leave its socket open.
impl Drop for DeepgramStream {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

fn send_err(e: impl std::fmt::Display) -> MeetCueError {
    MeetCueError::new(codes::ASR_CONNECT, e.to_string())
}

#[async_trait]
impl AsrStream for DeepgramStream {
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
            .send(Message::text(r#"{"type":"KeepAlive"}"#))
            .await
            .map_err(send_err)
    }

    async fn finalize(&mut self) -> Result<(), MeetCueError> {
        self.sink
            .send(Message::text(r#"{"type":"Finalize"}"#))
            .await
            .map_err(send_err)
    }

    async fn close(mut self: Box<Self>) -> Result<AudioUsage, MeetCueError> {
        let _ = self
            .sink
            .send(Message::text(r#"{"type":"CloseStream"}"#))
            .await;
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
    fn url_has_options_and_keyterms() {
        let opts = AsrOptions {
            model: "nova-3".into(),
            languages: vec!["en".into()],
            hotwords: vec!["OpenFlow 1.3".into(), "BGP".into()],
            endpoint_silence_ms: 300,
            ..Default::default()
        };
        let url = listen_url("api.eu.deepgram.com", &opts);
        assert!(url.starts_with("wss://api.eu.deepgram.com/v1/listen?model=nova-3&language=en&encoding=linear16&sample_rate=16000"));
        assert!(url.contains("&endpointing=300&"));
        assert!(url.contains("&keyterm=OpenFlow%201.3&keyterm=BGP"));
        assert!(!url.to_lowercase().contains("token"));
    }

    #[test]
    fn language_choice() {
        let l = |v: &[&str]| language_param(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(l(&["zh", "en"]), "zh-CN");
        // English first: the meeting is mainly English, so don't send it to
        // the Mandarin model.
        assert_eq!(l(&["en", "zh"]), "en");
        assert_eq!(l(&["en"]), "en");
        assert_eq!(l(&["zh"]), "zh-CN");
        assert_eq!(l(&["en", "de"]), "multi");
        assert_eq!(l(&[]), "en");
        let all = |v: &[&str]| covers_all(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert!(!all(&["zh", "en"]));
        assert!(all(&["zh"]) && all(&["en", "de"]));
    }

    #[test]
    fn parses_results() {
        let interim = r#"{"type":"Results","channel_index":[0,1],"duration":1.2,"start":0.5,"is_final":false,"speech_final":false,"channel":{"alternatives":[{"transcript":"how do you","confidence":0.9,"words":[]}]},"metadata":{}}"#;
        assert_eq!(
            parse_message(interim),
            vec![AsrResult::Partial {
                text: " how do you".into(),
                t0_ms: 500,
                t1_ms: 1700
            }]
        );
        let fin = r#"{"type":"Results","duration":2.0,"start":0.5,"is_final":true,"speech_final":true,"channel":{"alternatives":[{"transcript":"how do you scale it?"}]}}"#;
        assert_eq!(
            parse_message(fin),
            vec![
                AsrResult::Final {
                    text: " how do you scale it?".into(),
                    t0_ms: 500,
                    t1_ms: 2500
                },
                AsrResult::Endpoint { t_ms: 2500 },
            ]
        );
        let zh = r#"{"type":"Results","duration":1.0,"start":0.0,"is_final":true,"channel":{"alternatives":[{"transcript":"控 制 器 用 BGP 吗"}]}}"#;
        assert_eq!(
            parse_message(zh),
            vec![AsrResult::Final {
                text: "控制器用 BGP 吗".into(),
                t0_ms: 0,
                t1_ms: 1000
            }]
        );
        assert_eq!(
            parse_message(r#"{"type":"UtteranceEnd","channel":[0,1],"last_word_end":3.1}"#),
            vec![AsrResult::Endpoint { t_ms: 3100 }]
        );
        let flushed = r#"{"type":"Results","duration":0.1,"start":3.0,"is_final":true,"from_finalize":true,"channel":{"alternatives":[{"transcript":""}]}}"#;
        assert_eq!(
            parse_message(flushed),
            vec![AsrResult::Endpoint { t_ms: 3100 }]
        );
        assert!(parse_message(r#"{"type":"Metadata","request_id":"x"}"#).is_empty());
        assert!(matches!(
            parse_message(r#"{"type":"Error","variant":"SchemaError","description":"bad"}"#)[0],
            AsrResult::Error(_)
        ));
    }

    #[test]
    fn segments_of_one_sentence_join_with_spaces() {
        // Two final segments and a partial, as the engine concatenates them.
        let seg = |t: &str| {
            match parse_message(&format!(
            r#"{{"type":"Results","duration":1.0,"start":0.0,"is_final":true,"channel":{{"alternatives":[{{"transcript":"{t}"}}]}}}}"#
        ))
        .remove(0)
        {
            AsrResult::Final { text, .. } => text,
            other => panic!("{other:?}"),
        }
        };
        let joined = format!(
            "{}{}{}",
            seg("so what is your"),
            seg("experience with"),
            seg("Rust?")
        );
        assert_eq!(joined.trim(), "so what is your experience with Rust?");
        let zh = format!("{}{}", seg("你 在 项 目"), seg("中 用 过 吗"));
        assert_eq!(zh.trim(), "你在项目中用过吗");
        assert_eq!(seg(", right?"), ", right?");
    }
}
