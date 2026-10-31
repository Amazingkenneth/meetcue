//! Alibaba Cloud Model Studio (DashScope) real-time speech recognition:
//! Fun-ASR and Paraformer.
//!
//! WebSocket `wss://<host>/api-ws/v1/inference` with a bearer header. The
//! client sends a JSON `run-task`, waits for `task-started`, streams binary
//! PCM and ends with `finish-task`. Results are `result-generated` events
//! holding the current sentence; `sentence_end` marks it final.
//!
//! Keys belong to one region (Singapore or Beijing) and are rejected by the
//! other, so a stream whose key is refused tries the other region's host and
//! remembers the one that worked.

use std::sync::Mutex;

use async_trait::async_trait;
use futures::stream::{BoxStream, SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

use meetcue_core::{codes, MeetCueError};

use super::pad::{self, TrailingQuiet};
use crate::endpoints::EndpointCatalog;
use crate::net::{self, Ws};
use crate::provider::{AsrOptions, AsrProvider, AsrResult, AsrStream, AudioUsage, ModelInfo};

pub const ID: &str = "alibaba";
/// Singapore (international) and Beijing (Chinese mainland).
pub const DEFAULT_HOST: &str = "dashscope-intl.aliyuncs.com";
pub const DEFAULT_MODEL: &str = "fun-asr-realtime";
/// Fun-ASR reads at most this many characters of context.
const CONTEXT_CHARS: usize = 400;
/// Silence sent as a keep-alive while the gate is closed.
const KEEPALIVE_MS: u64 = 100;

pub struct Alibaba {
    key: String,
    host: Option<String>,
    proxy: Option<String>,
    /// The host that last accepted the key.
    good_host: Mutex<Option<String>>,
}

impl Alibaba {
    pub fn new(key: String, host: Option<String>, proxy: Option<String>) -> Self {
        Self {
            key,
            host,
            proxy,
            good_host: Mutex::new(None),
        }
    }

    /// Hosts in the order to try: the one that worked, the chosen one, then
    /// the other regions.
    fn hosts(&self, chosen: Option<&str>) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let good = self.good_host.lock().unwrap().clone();
        let known = EndpointCatalog::builtin().hosts(ID).to_vec();
        for h in good
            .into_iter()
            .chain(chosen.map(String::from))
            .chain(self.host.clone())
            .chain(known)
            .chain(std::iter::once(DEFAULT_HOST.to_string()))
        {
            if !out.contains(&h) {
                out.push(h);
            }
        }
        out
    }

    async fn start(
        &self,
        host: &str,
        opts: &AsrOptions,
        task_id: &str,
    ) -> Result<(SplitSink<Ws, Message>, SplitStream<Ws>), MeetCueError> {
        let url = format!("wss://{host}/api-ws/v1/inference");
        let proxy = opts.proxy.clone().or_else(|| self.proxy.clone());
        let (ws, _) = net::websocket(
            &url,
            &[("Authorization", format!("Bearer {}", self.key))],
            proxy.as_deref(),
        )
        .await?;
        let (mut sink, mut stream) = ws.split();
        sink.send(Message::text(run_task(opts, task_id).to_string()))
            .await
            .map_err(send_err)?;
        // Audio sent before `task-started` is rejected.
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(Message::Text(t)) => match parse_event(t.as_str()) {
                    Event::Started => return Ok((sink, stream)),
                    Event::Failed(e) => return Err(e),
                    _ => {}
                },
                Ok(Message::Close(frame)) => {
                    return Err(MeetCueError::new(
                        codes::ASR_CONNECT,
                        frame
                            .map(|f| f.reason.to_string())
                            .unwrap_or_else(|| "closed before the task started".into()),
                    ))
                }
                Ok(_) => {}
                Err(e) => return Err(send_err(e)),
            }
        }
        Err(MeetCueError::new(
            codes::ASR_CONNECT,
            "closed before the task started",
        ))
    }

    async fn connect(
        &self,
        opts: &AsrOptions,
        task_id: &str,
    ) -> Result<(SplitSink<Ws, Message>, SplitStream<Ws>), MeetCueError> {
        let hosts = self.hosts(opts.host.as_deref());
        let mut last = None;
        for host in &hosts {
            match self.start(host, opts, task_id).await {
                Ok(s) => {
                    *self.good_host.lock().unwrap() = Some(host.clone());
                    return Ok(s);
                }
                // A key from the other region: try that region.
                Err(e) if e.code == codes::ASR_AUTH => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| MeetCueError::new(codes::ASR_CONNECT, "no host")))
    }
}

fn send_err(e: impl std::fmt::Display) -> MeetCueError {
    MeetCueError::new(codes::ASR_CONNECT, e.to_string())
}

fn new_task_id() -> String {
    format!("{:032x}", rand::random::<u128>())
}

/// The `run-task` message that starts recognition.
pub fn run_task(opts: &AsrOptions, task_id: &str) -> serde_json::Value {
    let model = if opts.model.is_empty() {
        DEFAULT_MODEL
    } else {
        opts.model.as_str()
    };
    let mut params = json!({
        "format": "pcm",
        "sample_rate": 16_000,
        // Silent audio is sent while nobody speaks; keep the task open.
        "heartbeat": true,
    });
    if opts.endpoint_silence_ms > 0 {
        params["max_sentence_silence"] = json!(opts.endpoint_silence_ms.clamp(200, 6000));
    }
    // Only the first hint is used, so a single language is pinned; with
    // several the model detects them and handles mixed speech.
    if let [only] = opts.languages.as_slice() {
        params["language_hints"] = json!([only]);
    }
    let mut input = json!({});
    if model.starts_with("fun-asr") && !opts.hotwords.is_empty() {
        let mut terms = opts.hotwords.join(", ");
        if let Some((i, _)) = terms.char_indices().nth(CONTEXT_CHARS) {
            terms.truncate(i);
        }
        input["context"] = json!([{
            "role": "user",
            "content": [{ "type": "input_text", "text": terms }],
        }]);
    }
    json!({
        "header": { "action": "run-task", "task_id": task_id, "streaming": "duplex" },
        "payload": {
            "task_group": "audio",
            "task": "asr",
            "function": "recognition",
            "model": model,
            "parameters": params,
            "input": input,
        },
    })
}

pub fn finish_task(task_id: &str) -> serde_json::Value {
    json!({
        "header": { "action": "finish-task", "task_id": task_id, "streaming": "duplex" },
        "payload": { "input": {} },
    })
}

/// Maps a `task-failed` error code to a MeetCue error code.
pub fn error_code(code: &str, message: &str) -> &'static str {
    let c = format!("{code} {message}").to_ascii_lowercase();
    // Billing first: "account is in arrears" also says "access denied".
    if [
        "throttl",
        "ratelimit",
        "rate limit",
        "quota",
        "arrear",
        "concurren",
    ]
    .iter()
    .any(|k| c.contains(k))
    {
        codes::ASR_RATE_LIMIT
    } else if [
        "apikey",
        "api key",
        "api-key",
        "auth",
        "access",
        "forbidden",
        "unauthorized",
    ]
    .iter()
    .any(|k| c.contains(k))
    {
        codes::ASR_AUTH
    } else if [
        "invalidparameter",
        "invalid parameter",
        "badrequest",
        "model not",
        "unsupported",
    ]
    .iter()
    .any(|k| c.contains(k))
    {
        codes::ASR_PROTOCOL
    } else {
        codes::ASR_CONNECT
    }
}

#[derive(Debug, PartialEq)]
pub enum Event {
    Started,
    Results(Vec<AsrResult>),
    Finished,
    Failed(MeetCueError),
    Other,
}

#[derive(Debug, Deserialize)]
struct Header {
    event: String,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Sentence {
    #[serde(default)]
    begin_time: Option<i64>,
    #[serde(default)]
    end_time: Option<i64>,
    #[serde(default)]
    text: String,
    #[serde(default)]
    heartbeat: Option<bool>,
    #[serde(default)]
    sentence_end: bool,
}

#[derive(Debug, Deserialize)]
struct Output {
    #[serde(default)]
    sentence: Option<Sentence>,
}

#[derive(Debug, Default, Deserialize)]
struct Payload {
    #[serde(default)]
    output: Option<Output>,
}

#[derive(Debug, Deserialize)]
struct Envelope {
    header: Header,
    #[serde(default)]
    payload: Option<Payload>,
}

/// Turns one server message into an event.
pub fn parse_event(text: &str) -> Event {
    let env: Envelope = match serde_json::from_str(text) {
        Ok(e) => e,
        Err(e) => return Event::Failed(MeetCueError::new(codes::ASR_PROTOCOL, e.to_string())),
    };
    match env.header.event.as_str() {
        "task-started" => Event::Started,
        "task-finished" => Event::Finished,
        "task-failed" => {
            let code = env.header.error_code.unwrap_or_default();
            let message = env.header.error_message.unwrap_or_default();
            Event::Failed(MeetCueError::new(
                error_code(&code, &message),
                format!("{code}: {message}"),
            ))
        }
        "result-generated" => {
            let Some(s) = env.payload.and_then(|p| p.output).and_then(|o| o.sentence) else {
                return Event::Other;
            };
            if s.heartbeat == Some(true) {
                return Event::Other;
            }
            let t0 = s.begin_time.unwrap_or(0);
            let t1 = s.end_time.unwrap_or(t0).max(t0);
            let mut out = Vec::new();
            if s.sentence_end {
                if !s.text.is_empty() {
                    out.push(AsrResult::Final {
                        text: s.text,
                        t0_ms: t0,
                        t1_ms: t1,
                    });
                }
                out.push(AsrResult::Endpoint { t_ms: t1 });
            } else if !s.text.is_empty() {
                out.push(AsrResult::Partial {
                    text: s.text,
                    t0_ms: t0,
                    t1_ms: t1,
                });
            }
            Event::Results(out)
        }
        _ => Event::Other,
    }
}

pub fn models() -> Vec<ModelInfo> {
    [
        ("fun-asr-realtime", "Fun-ASR realtime"),
        (
            "paraformer-realtime-v2",
            "Paraformer realtime v2 (Beijing only)",
        ),
    ]
    .into_iter()
    .map(|(id, name)| ModelInfo {
        id: id.into(),
        display_name: name.into(),
        price: None,
    })
    .collect()
}

#[async_trait]
impl AsrProvider for Alibaba {
    fn id(&self) -> &'static str {
        ID
    }

    async fn open(&self, opts: AsrOptions) -> Result<Box<dyn AsrStream>, MeetCueError> {
        let task_id = new_task_id();
        let (sink, mut stream) = self.connect(&opts, &task_id).await?;
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let reader = tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(Message::Text(t)) => match parse_event(t.as_str()) {
                        Event::Results(rs) => {
                            for r in rs {
                                if tx.unbounded_send(r).is_err() {
                                    return;
                                }
                            }
                        }
                        Event::Failed(e) => {
                            let _ = tx.unbounded_send(AsrResult::Error(e));
                            return;
                        }
                        Event::Finished => return,
                        _ => {}
                    },
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
                        let _ = tx.unbounded_send(AsrResult::Error(send_err(e)));
                        return;
                    }
                }
            }
        });
        Ok(Box::new(AlibabaStream {
            sink,
            rx: Some(rx),
            reader,
            task_id,
            sent_ms: 0,
            quiet: TrailingQuiet::default(),
            sentence_silence_ms: u64::from(opts.endpoint_silence_ms.clamp(200, 6000)),
        }))
    }

    /// Starts and finishes an empty task: checks the key without audio.
    async fn verify(&self) -> Result<(), MeetCueError> {
        let opts = AsrOptions {
            model: DEFAULT_MODEL.into(),
            proxy: self.proxy.clone(),
            ..Default::default()
        };
        let task_id = new_task_id();
        let (mut sink, _stream) = self.connect(&opts, &task_id).await?;
        let _ = sink
            .send(Message::text(finish_task(&task_id).to_string()))
            .await;
        let _ = sink.close().await;
        Ok(())
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, MeetCueError> {
        Ok(models())
    }
}

struct AlibabaStream {
    sink: SplitSink<Ws, Message>,
    rx: Option<futures::channel::mpsc::UnboundedReceiver<AsrResult>>,
    reader: tokio::task::JoinHandle<()>,
    task_id: String,
    sent_ms: u64,
    quiet: TrailingQuiet,
    sentence_silence_ms: u64,
}

impl Drop for AlibabaStream {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl AlibabaStream {
    async fn send_pcm(&mut self, pcm: &[i16]) -> Result<(), MeetCueError> {
        if pcm.is_empty() {
            return Ok(());
        }
        self.sink
            .send(Message::binary(super::soniox::pcm_bytes(pcm)))
            .await
            .map_err(send_err)?;
        self.sent_ms += pcm.len() as u64 / 16;
        self.quiet.push(pcm);
        Ok(())
    }
}

#[async_trait]
impl AsrStream for AlibabaStream {
    async fn write(&mut self, pcm16k: &[i16], _capture_ts_ms: i64) -> Result<(), MeetCueError> {
        self.send_pcm(pcm16k).await
    }

    fn results(&mut self) -> BoxStream<'static, AsrResult> {
        match self.rx.take() {
            Some(rx) => rx.boxed(),
            None => futures::stream::empty().boxed(),
        }
    }

    /// There is no keep-alive message: a little silence keeps the task open.
    async fn keepalive(&mut self) -> Result<(), MeetCueError> {
        self.send_pcm(&pad::silence(KEEPALIVE_MS)).await
    }

    /// The sentence ends once the server hears enough silence; send what is
    /// still missing.
    async fn finalize(&mut self) -> Result<(), MeetCueError> {
        let missing = self.quiet.missing(self.sentence_silence_ms + 200);
        self.send_pcm(&pad::silence(missing)).await
    }

    async fn close(mut self: Box<Self>) -> Result<AudioUsage, MeetCueError> {
        let _ = self
            .sink
            .send(Message::text(finish_task(&self.task_id).to_string()))
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

    fn opts() -> AsrOptions {
        AsrOptions {
            languages: vec!["zh".into(), "en".into()],
            hotwords: vec!["MeetCue".into(), "张三".into()],
            endpoint_silence_ms: 700,
            ..Default::default()
        }
    }

    #[test]
    fn run_task_has_format_silence_and_context() {
        let m = run_task(&opts(), "abc");
        assert_eq!(m["header"]["action"], "run-task");
        assert_eq!(m["header"]["task_id"], "abc");
        assert_eq!(m["header"]["streaming"], "duplex");
        let p = &m["payload"];
        assert_eq!(p["task_group"], "audio");
        assert_eq!(p["model"], DEFAULT_MODEL);
        assert_eq!(p["parameters"]["format"], "pcm");
        assert_eq!(p["parameters"]["sample_rate"], 16000);
        assert_eq!(p["parameters"]["max_sentence_silence"], 700);
        assert_eq!(p["parameters"]["heartbeat"], true);
        assert!(
            p["parameters"].get("language_hints").is_none(),
            "two languages: auto-detect"
        );
        assert_eq!(
            p["input"]["context"][0]["content"][0]["text"],
            "MeetCue, 张三"
        );
        assert!(m.to_string().find("Bearer").is_none());
    }

    #[test]
    fn one_language_is_pinned_and_paraformer_gets_no_context() {
        let o = AsrOptions {
            model: "paraformer-realtime-v2".into(),
            languages: vec!["zh".into()],
            endpoint_silence_ms: 50,
            ..opts()
        };
        let m = run_task(&o, "t");
        assert_eq!(m["payload"]["parameters"]["language_hints"], json!(["zh"]));
        assert_eq!(m["payload"]["parameters"]["max_sentence_silence"], 200);
        assert!(m["payload"]["input"].get("context").is_none());
    }

    #[test]
    fn context_is_cut_to_400_characters() {
        let o = AsrOptions {
            hotwords: vec!["术语".repeat(300)],
            ..opts()
        };
        let m = run_task(&o, "t");
        let text = m["payload"]["input"]["context"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(text.chars().count(), 400);
    }

    #[test]
    fn parses_events() {
        assert_eq!(
            parse_event(
                r#"{"header":{"task_id":"a","event":"task-started","attributes":{}},"payload":{}}"#
            ),
            Event::Started
        );
        let begin = r#"{"header":{"task_id":"a","event":"result-generated","attributes":{}},"payload":{"output":{"sentence":{"begin_time":0,"end_time":null,"text":"","sentence_begin":true,"sentence_end":false,"sentence_id":1,"words":[]}},"usage":null}}"#;
        assert_eq!(parse_event(begin), Event::Results(vec![]));
        let partial = r#"{"header":{"task_id":"a","event":"result-generated","attributes":{}},"payload":{"output":{"sentence":{"begin_time":170,"end_time":null,"text":"你们怎么","sentence_end":false,"sentence_id":1,"words":[]}},"usage":null}}"#;
        assert_eq!(
            parse_event(partial),
            Event::Results(vec![AsrResult::Partial {
                text: "你们怎么".into(),
                t0_ms: 170,
                t1_ms: 170
            }])
        );
        let fin = r#"{"header":{"task_id":"a","event":"result-generated","attributes":{}},"payload":{"output":{"sentence":{"begin_time":170,"end_time":920,"text":"OK, got it.","heartbeat":false,"sentence_end":true,"sentence_id":1,"words":[]}},"usage":{"duration":3}}}"#;
        assert_eq!(
            parse_event(fin),
            Event::Results(vec![
                AsrResult::Final {
                    text: "OK, got it.".into(),
                    t0_ms: 170,
                    t1_ms: 920
                },
                AsrResult::Endpoint { t_ms: 920 },
            ])
        );
        let beat = r#"{"header":{"task_id":"a","event":"result-generated","attributes":{}},"payload":{"output":{"sentence":{"begin_time":0,"end_time":0,"text":"","heartbeat":true,"sentence_end":true,"sentence_id":0}},"usage":null}}"#;
        assert_eq!(parse_event(beat), Event::Other);
        assert_eq!(
            parse_event(
                r#"{"header":{"task_id":"a","event":"task-finished","attributes":{}},"payload":{"output":{},"usage":null}}"#
            ),
            Event::Finished
        );
    }

    #[test]
    fn maps_failures() {
        let failed = |code: &str, msg: &str| {
            let t = json!({"header":{"task_id":"a","event":"task-failed","error_code":code,"error_message":msg,"attributes":{}},"payload":{}});
            match parse_event(&t.to_string()) {
                Event::Failed(e) => e.code,
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(
            failed("InvalidApiKey", "Invalid API-key provided."),
            codes::ASR_AUTH
        );
        assert_eq!(
            failed("Throttling.RateQuota", "Requests rate limit exceeded"),
            codes::ASR_RATE_LIMIT
        );
        assert_eq!(
            failed("Arrearage", "Access denied, account is in arrears"),
            codes::ASR_RATE_LIMIT
        );
        assert_eq!(
            failed("InvalidParameter", "bad sample_rate"),
            codes::ASR_PROTOCOL
        );
        assert_eq!(
            failed("CLIENT_ERROR", "request timeout after 23 seconds."),
            codes::ASR_CONNECT
        );
        assert!(matches!(parse_event("not json"), Event::Failed(_)));
    }

    #[test]
    fn tries_the_remembered_host_first_then_both_regions() {
        let a = Alibaba::new("k".into(), Some("dashscope.aliyuncs.com".into()), None);
        assert_eq!(
            a.hosts(None),
            vec!["dashscope.aliyuncs.com", "dashscope-intl.aliyuncs.com"]
        );
        *a.good_host.lock().unwrap() = Some("dashscope-intl.aliyuncs.com".into());
        assert_eq!(
            a.hosts(Some("dashscope.aliyuncs.com")),
            vec!["dashscope-intl.aliyuncs.com", "dashscope.aliyuncs.com"]
        );
    }
}
