//! Tencent Cloud real-time speech recognition.
//!
//! WebSocket `wss://asr.cloud.tencent.com/asr/v2/<appid>?<params>`: every
//! option is a query parameter and the URL is signed with HMAC-SHA1 using the
//! SecretKey. The server answers with a handshake message, then binary PCM
//! goes up and JSON results come back: `slice_type` 0 and 1 are the current
//! sentence, 2 is the sentence final. `{"type":"end"}` ends the audio.
//!
//! The credential is `AppID:SecretId:SecretKey` in one field.

use async_trait::async_trait;
use base64::Engine;
use futures::stream::{BoxStream, SplitSink};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::tungstenite::Message;

use meetcue_core::{codes, MeetCueError};

use super::pad::{self, TrailingQuiet};
use crate::net::{self, Ws};
use crate::provider::{AsrOptions, AsrProvider, AsrResult, AsrStream, AudioUsage, ModelInfo};

pub const ID: &str = "tencent";
pub const DEFAULT_HOST: &str = "asr.cloud.tencent.com";
/// Chinese and English with dialects (large model).
pub const DEFAULT_MODEL: &str = "16k_zh_en";
const MAX_HOTWORDS: usize = 128;
const HOTWORD_WEIGHT: u32 = 10;
/// Silence sent as a keep-alive: the server ends a stream after 6 s without audio.
const KEEPALIVE_MS: u64 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub app_id: String,
    pub secret_id: String,
    pub secret_key: String,
}

impl Credentials {
    /// Reads `AppID:SecretId:SecretKey`; commas or spaces also separate.
    pub fn parse(s: &str) -> Result<Credentials, MeetCueError> {
        let parts: Vec<&str> = s
            .split(|c: char| c == ':' || c == ',' || c.is_whitespace())
            .filter(|p| !p.is_empty())
            .collect();
        match parts.as_slice() {
            [app, id, key] if app.chars().all(|c| c.is_ascii_digit()) => Ok(Credentials {
                app_id: app.to_string(),
                secret_id: id.to_string(),
                secret_key: key.to_string(),
            }),
            _ => Err(MeetCueError::new(
                codes::ASR_AUTH,
                "Tencent Cloud key must be AppID:SecretId:SecretKey",
            )),
        }
    }
}

pub struct Tencent {
    creds: Result<Credentials, MeetCueError>,
    host: String,
    proxy: Option<String>,
}

impl Tencent {
    pub fn new(key: String, host: Option<String>, proxy: Option<String>) -> Self {
        Self {
            creds: Credentials::parse(&key),
            host: host.unwrap_or_else(|| DEFAULT_HOST.into()),
            proxy,
        }
    }

    async fn connect(
        &self,
        opts: &AsrOptions,
    ) -> Result<(SplitSink<Ws, Message>, SplitStreamWs), MeetCueError> {
        let creds = self.creds.clone()?;
        let host = opts.host.clone().unwrap_or_else(|| self.host.clone());
        let now = chrono::Utc::now().timestamp();
        let nonce = rand::random::<u32>() % 1_000_000_000 + 1;
        let voice_id = format!("{:032x}", rand::random::<u128>());
        let url = signed_url(&host, &creds, &params(opts, now, nonce, &voice_id));
        let proxy = opts.proxy.clone().or_else(|| self.proxy.clone());
        let (ws, _) = net::websocket(&url, &[], proxy.as_deref()).await?;
        let (sink, mut stream) = ws.split();
        // The first message says whether the parameters and signature were accepted.
        loop {
            match stream.next().await {
                Some(Ok(Message::Text(t))) => {
                    let r = parse_message(t.as_str());
                    if let Some(AsrResult::Error(e)) = r.results.into_iter().next() {
                        return Err(e);
                    }
                    return Ok((sink, stream));
                }
                Some(Ok(Message::Close(frame))) => {
                    return Err(MeetCueError::new(
                        codes::ASR_CONNECT,
                        frame
                            .map(|f| f.reason.to_string())
                            .unwrap_or_else(|| "closed during the handshake".into()),
                    ))
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(send_err(e)),
                None => {
                    return Err(MeetCueError::new(
                        codes::ASR_CONNECT,
                        "closed during the handshake",
                    ))
                }
            }
        }
    }
}

type SplitStreamWs = futures::stream::SplitStream<Ws>;

fn send_err(e: impl std::fmt::Display) -> MeetCueError {
    MeetCueError::new(codes::ASR_CONNECT, e.to_string())
}

fn model_for(opts: &AsrOptions) -> String {
    if !opts.model.is_empty() {
        return opts.model.clone();
    }
    match opts.languages.as_slice() {
        [only] if only == "en" => "16k_en".into(),
        _ => DEFAULT_MODEL.into(),
    }
}

/// Hotwords as `word|weight,…`, without characters that would break the list.
pub fn hotword_list(words: &[String]) -> Option<String> {
    let list: Vec<String> = words
        .iter()
        .map(|w| w.replace(['|', ','], " ").trim().to_string())
        .filter(|w| !w.is_empty() && w.chars().count() <= 30)
        .take(MAX_HOTWORDS)
        .map(|w| format!("{w}|{HOTWORD_WEIGHT}"))
        .collect();
    (!list.is_empty()).then(|| list.join(","))
}

/// Query parameters except `signature`, sorted by name.
pub fn params(
    opts: &AsrOptions,
    now: i64,
    nonce: u32,
    voice_id: &str,
) -> Vec<(&'static str, String)> {
    let creds_free = vec![
        ("engine_model_type", model_for(opts)),
        ("expired", (now + 24 * 3600).to_string()),
        ("needvad", "1".into()),
        ("nonce", nonce.to_string()),
        ("timestamp", now.to_string()),
        (
            "vad_silence_time",
            opts.endpoint_silence_ms.clamp(500, 2000).to_string(),
        ),
        ("voice_format", "1".into()),
        ("voice_id", voice_id.to_string()),
    ];
    let mut p = creds_free;
    if let Some(h) = hotword_list(&opts.hotwords) {
        p.push(("hotword_list", h));
    }
    p.sort_by(|a, b| a.0.cmp(b.0));
    p
}

/// The signed WebSocket URL. The signature covers the raw parameter values;
/// the URL carries them percent-encoded.
pub fn signed_url(host: &str, creds: &Credentials, params: &[(&'static str, String)]) -> String {
    let mut all: Vec<(&str, String)> = params.iter().map(|(k, v)| (*k, v.clone())).collect();
    all.push(("secretid", creds.secret_id.clone()));
    all.sort_by(|a, b| a.0.cmp(b.0));
    let raw: Vec<String> = all.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let path = format!("{host}/asr/v2/{}", creds.app_id);
    let to_sign = format!("{path}?{}", raw.join("&"));
    let mac = hmac_sha1(creds.secret_key.as_bytes(), to_sign.as_bytes());
    let signature = base64::engine::general_purpose::STANDARD.encode(mac);
    let encoded: Vec<String> = all
        .iter()
        .map(|(k, v)| format!("{k}={}", percent_encode(v)))
        .collect();
    format!(
        "wss://{path}?{}&signature={}",
        encoded.join("&"),
        percent_encode(&signature)
    )
}

/// Percent-encodes everything but unreserved characters (RFC 3986).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// SHA-1 (FIPS 180-4), only for the HMAC Tencent's signature requires.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

/// HMAC-SHA1 (RFC 2104).
pub fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; 20] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..20].copy_from_slice(&sha1(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    inner.extend_from_slice(data);
    let mut outer: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    outer.extend_from_slice(&sha1(&inner));
    sha1(&outer)
}

/// Maps Tencent's error codes to MeetCue's.
pub fn error_code(code: i64) -> &'static str {
    match code {
        // Signature, AppID not activated, overseas access to the mainland service.
        4002 | 4003 | 6001 => codes::ASR_AUTH,
        // Resource pack used up, arrears, too many streams.
        4004..=4006 => codes::ASR_RATE_LIMIT,
        4001 | 4007 | 4010 => codes::ASR_PROTOCOL,
        _ => codes::ASR_CONNECT,
    }
}

#[derive(Debug, Deserialize)]
struct Slice {
    #[serde(default)]
    slice_type: i64,
    #[serde(default)]
    start_time: i64,
    #[serde(default)]
    end_time: i64,
    #[serde(default)]
    voice_text_str: String,
}

#[derive(Debug, Deserialize)]
struct Response {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    result: Option<Slice>,
    #[serde(default)]
    r#final: Option<i64>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub results: Vec<AsrResult>,
    /// The server sent its last message.
    pub done: bool,
}

pub fn parse_message(text: &str) -> Parsed {
    let resp: Response = match serde_json::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            return Parsed {
                results: vec![AsrResult::Error(MeetCueError::new(
                    codes::ASR_PROTOCOL,
                    e.to_string(),
                ))],
                done: true,
            }
        }
    };
    if resp.code != 0 {
        return Parsed {
            results: vec![AsrResult::Error(MeetCueError::new(
                error_code(resp.code),
                format!("{} {}", resp.code, resp.message),
            ))],
            done: true,
        };
    }
    let mut out = Vec::new();
    if let Some(s) = resp.result {
        let (t0, t1) = (s.start_time, s.end_time.max(s.start_time));
        if s.slice_type == 2 {
            if !s.voice_text_str.is_empty() {
                out.push(AsrResult::Final {
                    text: s.voice_text_str,
                    t0_ms: t0,
                    t1_ms: t1,
                });
            }
            out.push(AsrResult::Endpoint { t_ms: t1 });
        } else if !s.voice_text_str.is_empty() {
            out.push(AsrResult::Partial {
                text: s.voice_text_str,
                t0_ms: t0,
                t1_ms: t1,
            });
        }
    }
    Parsed {
        results: out,
        done: resp.r#final == Some(1),
    }
}

pub fn models() -> Vec<ModelInfo> {
    [
        ("16k_zh_en", "Chinese & English, dialects (large model)"),
        ("16k_zh", "Mandarin (standard)"),
        ("16k_yue", "Cantonese"),
        ("16k_en", "English"),
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
impl AsrProvider for Tencent {
    fn id(&self) -> &'static str {
        ID
    }

    async fn open(&self, opts: AsrOptions) -> Result<Box<dyn AsrStream>, MeetCueError> {
        let (sink, mut stream) = self.connect(&opts).await?;
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let reader = tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(Message::Text(t)) => {
                        let parsed = parse_message(t.as_str());
                        for r in parsed.results {
                            if tx.unbounded_send(r).is_err() {
                                return;
                            }
                        }
                        if parsed.done {
                            return;
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
                        let _ = tx.unbounded_send(AsrResult::Error(send_err(e)));
                        return;
                    }
                }
            }
        });
        Ok(Box::new(TencentStream {
            sink,
            rx: Some(rx),
            reader,
            sent_ms: 0,
            quiet: TrailingQuiet::default(),
            sentence_silence_ms: u64::from(opts.endpoint_silence_ms.clamp(500, 2000)),
        }))
    }

    /// Opens a stream and ends it at once: the handshake checks the signature.
    async fn verify(&self) -> Result<(), MeetCueError> {
        let opts = AsrOptions {
            proxy: self.proxy.clone(),
            endpoint_silence_ms: 700,
            ..Default::default()
        };
        let (mut sink, _stream) = self.connect(&opts).await?;
        let _ = sink.send(Message::text(r#"{"type":"end"}"#)).await;
        let _ = sink.close().await;
        Ok(())
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, MeetCueError> {
        Ok(models())
    }
}

struct TencentStream {
    sink: SplitSink<Ws, Message>,
    rx: Option<futures::channel::mpsc::UnboundedReceiver<AsrResult>>,
    reader: tokio::task::JoinHandle<()>,
    sent_ms: u64,
    quiet: TrailingQuiet,
    sentence_silence_ms: u64,
}

impl Drop for TencentStream {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl TencentStream {
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
impl AsrStream for TencentStream {
    async fn write(&mut self, pcm16k: &[i16], _capture_ts_ms: i64) -> Result<(), MeetCueError> {
        self.send_pcm(pcm16k).await
    }

    fn results(&mut self) -> BoxStream<'static, AsrResult> {
        match self.rx.take() {
            Some(rx) => rx.boxed(),
            None => futures::stream::empty().boxed(),
        }
    }

    async fn keepalive(&mut self) -> Result<(), MeetCueError> {
        self.send_pcm(&pad::silence(KEEPALIVE_MS)).await
    }

    async fn finalize(&mut self) -> Result<(), MeetCueError> {
        let missing = self.quiet.missing(self.sentence_silence_ms + 200);
        self.send_pcm(&pad::silence(missing)).await
    }

    async fn close(mut self: Box<Self>) -> Result<AudioUsage, MeetCueError> {
        let _ = self.sink.send(Message::text(r#"{"type":"end"}"#)).await;
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

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha1_and_hmac_match_reference_vectors() {
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        // RFC 2202 test cases 1, 2 and 6.
        assert_eq!(
            hex(&hmac_sha1(&[0x0b; 20], b"Hi There")),
            "b617318655057264e28bc0b6fb378c8ef146be00"
        );
        assert_eq!(
            hex(&hmac_sha1(b"Jefe", b"what do ya want for nothing?")),
            "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79"
        );
        assert_eq!(
            hex(&hmac_sha1(
                &[0xaa; 80],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "aa4ae5e15272d00e95705637ce8a3b55ed402112"
        );
    }

    #[test]
    fn parses_credentials() {
        let c = Credentials::parse(" 1259228442:AKIDabc:secretXYZ\n").unwrap();
        assert_eq!(c.app_id, "1259228442");
        assert_eq!(c.secret_id, "AKIDabc");
        assert_eq!(c.secret_key, "secretXYZ");
        assert_eq!(
            Credentials::parse("1259228442 AKIDabc secretXYZ").unwrap(),
            c
        );
        assert!(Credentials::parse("AKIDabc:secretXYZ").is_err());
        assert!(Credentials::parse("app:AKIDabc:secretXYZ").is_err());
        assert_eq!(Credentials::parse("").unwrap_err().code, codes::ASR_AUTH);
    }

    #[test]
    fn builds_sorted_params_with_hotwords() {
        let opts = AsrOptions {
            languages: vec!["zh".into(), "en".into()],
            hotwords: vec!["MeetCue".into(), "a|b,c".into(), " ".into()],
            endpoint_silence_ms: 300,
            ..Default::default()
        };
        let p = params(&opts, 1_700_000_000, 42, "vid");
        let keys: Vec<&str> = p.iter().map(|(k, _)| *k).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        let get = |k: &str| p.iter().find(|(n, _)| *n == k).unwrap().1.clone();
        assert_eq!(get("engine_model_type"), "16k_zh_en");
        assert_eq!(get("vad_silence_time"), "500");
        assert_eq!(get("voice_format"), "1");
        assert_eq!(get("needvad"), "1");
        assert_eq!(get("expired"), "1700086400");
        assert_eq!(get("hotword_list"), "MeetCue|10,a b c|10");
        let en = AsrOptions {
            languages: vec!["en".into()],
            ..Default::default()
        };
        let p = params(&en, 0, 1, "v");
        assert!(p
            .iter()
            .any(|(k, v)| *k == "engine_model_type" && v == "16k_en"));
        assert!(!p.iter().any(|(k, _)| *k == "hotword_list"));
    }

    #[test]
    fn signs_like_the_documentation() {
        let creds = Credentials {
            app_id: "125922".into(),
            secret_id: "AKIDtest".into(),
            secret_key: "key".into(),
        };
        let params = vec![
            ("engine_model_type", "16k_zh".to_string()),
            ("hotword_list", "腾讯云|10".to_string()),
            ("timestamp", "1".to_string()),
        ];
        let url = signed_url("asr.cloud.tencent.com", &creds, &params);
        let to_sign = "asr.cloud.tencent.com/asr/v2/125922?engine_model_type=16k_zh&hotword_list=腾讯云|10&secretid=AKIDtest&timestamp=1";
        let sig =
            base64::engine::general_purpose::STANDARD.encode(hmac_sha1(b"key", to_sign.as_bytes()));
        assert!(url.starts_with("wss://asr.cloud.tencent.com/asr/v2/125922?engine_model_type=16k_zh&hotword_list=%E8%85%BE%E8%AE%AF%E4%BA%91%7C10&secretid=AKIDtest&timestamp=1&signature="));
        assert!(url.ends_with(&percent_encode(&sig)));
        assert!(!url.contains("key&"), "the SecretKey never goes in the URL");
        assert_eq!(percent_encode("a+/="), "a%2B%2F%3D");
    }

    #[test]
    fn parses_slices() {
        assert_eq!(
            parse_message(r#"{"code":0,"message":"success","voice_id":"v"}"#),
            Parsed::default()
        );
        let partial = r#"{"code":0,"message":"success","voice_id":"v","message_id":"v_1_0","result":{"slice_type":1,"index":0,"start_time":100,"end_time":900,"voice_text_str":"实时语音","word_size":0,"word_list":[]}}"#;
        assert_eq!(
            parse_message(partial).results,
            vec![AsrResult::Partial {
                text: "实时语音".into(),
                t0_ms: 100,
                t1_ms: 900
            }]
        );
        let fin = r#"{"code":0,"message":"success","voice_id":"v","message_id":"v_2_0","result":{"slice_type":2,"index":0,"start_time":100,"end_time":2840,"voice_text_str":"实时语音识别。","word_size":0,"word_list":[]}}"#;
        assert_eq!(
            parse_message(fin).results,
            vec![
                AsrResult::Final {
                    text: "实时语音识别。".into(),
                    t0_ms: 100,
                    t1_ms: 2840
                },
                AsrResult::Endpoint { t_ms: 2840 },
            ]
        );
        let start = r#"{"code":0,"message":"success","voice_id":"v","result":{"slice_type":0,"index":1,"start_time":3000,"end_time":3000,"voice_text_str":""}}"#;
        assert!(parse_message(start).results.is_empty());
        let end = parse_message(
            r#"{"code":0,"message":"success","voice_id":"v","message_id":"v_241","final":1}"#,
        );
        assert!(end.done && end.results.is_empty());
    }

    #[test]
    fn maps_errors() {
        let code =
            |c: i64| match &parse_message(&format!(r#"{{"code":{c},"message":"x"}}"#)).results[0] {
                AsrResult::Error(e) => e.code.clone(),
                other => panic!("{other:?}"),
            };
        assert_eq!(code(4002), codes::ASR_AUTH);
        assert_eq!(code(6001), codes::ASR_AUTH);
        assert_eq!(code(4005), codes::ASR_RATE_LIMIT);
        assert_eq!(code(4006), codes::ASR_RATE_LIMIT);
        assert_eq!(code(4001), codes::ASR_PROTOCOL);
        assert_eq!(code(4008), codes::ASR_CONNECT);
        assert_eq!(code(5000), codes::ASR_CONNECT);
        assert!(parse_message("nope").done);
    }
}
