//! Mock providers for tests, the browser preview and `--demo`.
//!
//! `MockAsr` replays a scripted transcript (`samples/transcripts/*.jsonl`) in
//! real time, ignoring the audio it receives. `MockLlm` answers detection and
//! hint requests from `samples/mock/*.json` with streamed text and usage.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::channel::mpsc;
use futures::stream::{BoxStream, StreamExt};
use serde::Deserialize;
use serde_json::json;

use meetcue_core::{Channel, MeetCueError, UsagePurpose};

use crate::prefilter::might_be_question;
use crate::provider::{
    AsrOptions, AsrProvider, AsrResult, AsrStream, AudioUsage, LlmChunk, LlmProvider, LlmRequest,
    LlmStream, ModelInfo,
};

pub const SDN_TRANSCRIPT: &str =
    include_str!("../../../../samples/transcripts/sdn-interview.jsonl");
pub const SDN_LLM: &str = include_str!("../../../../samples/mock/sdn-llm.json");

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ScriptLine {
    pub ch: Channel,
    pub t0: i64,
    pub t1: i64,
    pub text: String,
}

/// Parses a JSON-lines transcript, skipping blank lines.
pub fn parse_script(jsonl: &str) -> Result<Vec<ScriptLine>, MeetCueError> {
    jsonl
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l)
                .map_err(|e| MeetCueError::new(meetcue_core::codes::PRACTICE_FILE, e.to_string()))
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct MockAsr {
    script: Arc<Vec<ScriptLine>>,
    /// Time between the end of speech and the endpoint result.
    pub endpoint_delay_ms: i64,
    pub partial_every_ms: i64,
}

impl MockAsr {
    pub fn new(script: Vec<ScriptLine>) -> Self {
        Self {
            script: Arc::new(script),
            endpoint_delay_ms: 600,
            partial_every_ms: 300,
        }
    }

    pub fn sdn() -> Self {
        Self::new(parse_script(SDN_TRANSCRIPT).expect("bundled transcript is valid"))
    }

    /// The scheduled results of one channel as (emit time, result).
    fn schedule(&self, channel: Channel) -> Vec<(i64, AsrResult)> {
        let mut out = Vec::new();
        for line in self.script.iter().filter(|l| l.ch == channel) {
            let chars: Vec<char> = line.text.chars().collect();
            let span = (line.t1 - line.t0).max(1);
            let mut t = line.t0;
            while t < line.t1 {
                let n = ((t - line.t0 + self.partial_every_ms) * chars.len() as i64 / span)
                    .clamp(1, chars.len() as i64) as usize;
                out.push((
                    t,
                    AsrResult::Partial {
                        text: chars[..n].iter().collect(),
                        t0_ms: line.t0,
                        t1_ms: t,
                    },
                ));
                t += self.partial_every_ms;
            }
            out.push((
                line.t1,
                AsrResult::Final {
                    text: line.text.clone(),
                    t0_ms: line.t0,
                    t1_ms: line.t1,
                },
            ));
            out.push((
                line.t1 + self.endpoint_delay_ms,
                AsrResult::Endpoint { t_ms: line.t1 },
            ));
        }
        out.sort_by_key(|(t, _)| *t);
        out
    }
}

#[async_trait]
impl AsrProvider for MockAsr {
    fn id(&self) -> &'static str {
        "mock"
    }

    async fn open(&self, opts: AsrOptions) -> Result<Box<dyn AsrStream>, MeetCueError> {
        let (tx, rx) = mpsc::unbounded();
        let schedule = opts.channel.map(|c| self.schedule(c)).unwrap_or_default();
        // Without a channel (latency probe) the stream reacts to the audio
        // it receives instead of replaying the script.
        let live = opts.channel.is_none().then(|| tx.clone());
        let start = tokio::time::Instant::now();
        let task = tokio::spawn(async move {
            for (t, result) in schedule {
                tokio::time::sleep_until(start + Duration::from_millis(t.max(0) as u64)).await;
                if tx.unbounded_send(result).is_err() {
                    break;
                }
            }
        });
        Ok(Box::new(MockAsrStream {
            rx: Some(rx),
            task,
            sent_ms: 0,
            live,
        }))
    }
}

struct MockAsrStream {
    rx: Option<mpsc::UnboundedReceiver<AsrResult>>,
    task: tokio::task::JoinHandle<()>,
    sent_ms: u64,
    live: Option<mpsc::UnboundedSender<AsrResult>>,
}

#[async_trait]
impl AsrStream for MockAsrStream {
    async fn write(&mut self, pcm16k: &[i16], _capture_ts_ms: i64) -> Result<(), MeetCueError> {
        let voiced = crate::text_level(pcm16k) > -40.0;
        if let (Some(tx), true) = (&self.live, voiced) {
            let t = self.sent_ms as i64;
            let _ = tx.unbounded_send(AsrResult::Partial {
                text: "…".into(),
                t0_ms: t,
                t1_ms: t,
            });
        }
        self.sent_ms += (pcm16k.len() as u64 * 1000) / 16_000;
        Ok(())
    }

    async fn finalize(&mut self) -> Result<(), MeetCueError> {
        if let Some(tx) = &self.live {
            let t = self.sent_ms as i64;
            let _ = tx.unbounded_send(AsrResult::Final {
                text: "probe".into(),
                t0_ms: 0,
                t1_ms: t,
            });
            let _ = tx.unbounded_send(AsrResult::Endpoint { t_ms: t });
        }
        Ok(())
    }

    fn results(&mut self) -> BoxStream<'static, AsrResult> {
        match self.rx.take() {
            Some(rx) => rx.boxed(),
            None => futures::stream::empty().boxed(),
        }
    }

    async fn close(self: Box<Self>) -> Result<AudioUsage, MeetCueError> {
        self.task.abort();
        Ok(AudioUsage {
            audio_ms: self.sent_ms,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockPoint {
    pub n: u32,
    pub head: String,
    pub rest: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockQuestion {
    #[serde(rename = "match")]
    pub keywords: Vec<String>,
    pub short: String,
    pub tags: Vec<String>,
    pub sub_questions: Vec<String>,
    pub points: Vec<MockPoint>,
    pub answer: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MockFallback {
    pub points: Vec<MockPoint>,
    pub answer: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockData {
    pub questions: Vec<MockQuestion>,
    pub fallback: MockFallback,
    pub follow_up: String,
    pub review: Vec<String>,
}

impl MockData {
    fn find(&self, text: &str) -> Option<&MockQuestion> {
        let lower = text.to_lowercase();
        self.questions
            .iter()
            .find(|q| q.keywords.iter().any(|k| lower.contains(&k.to_lowercase())))
    }
}

#[derive(Debug)]
pub struct MockLlm {
    data: MockData,
    pub first_token_ms: u64,
    pub chunk_ms: u64,
    pub chunk_chars: usize,
    warmed: AtomicBool,
}

impl MockLlm {
    pub fn new(data: MockData) -> Self {
        Self {
            data,
            first_token_ms: 280,
            chunk_ms: 15,
            chunk_chars: 8,
            warmed: AtomicBool::new(false),
        }
    }

    pub fn sdn() -> Self {
        Self::new(serde_json::from_str(SDN_LLM).expect("bundled mock data is valid"))
    }

    fn reply(&self, req: &LlmRequest) -> String {
        let user = req
            .messages
            .last()
            .map(|m| m.content.as_str())
            .unwrap_or("");
        match req.purpose {
            UsagePurpose::Detect => {
                let last = user
                    .lines()
                    .rev()
                    .find_map(|l| l.strip_prefix("[Them] "))
                    .unwrap_or("");
                let hit = self.data.find(last);
                let is_question = might_be_question(last);
                json!({
                    "isQuestion": is_question,
                    "directedToMe": true,
                    "merged": false,
                    "interruption": false,
                    "short": hit.map(|q| q.short.as_str()).unwrap_or(""),
                    "subQuestions": hit.map(|q| q.sub_questions.clone()).unwrap_or_default(),
                    "tags": hit.map(|q| q.tags.clone()).unwrap_or_default(),
                })
                .to_string()
            }
            UsagePurpose::Hints | UsagePurpose::Probe => {
                let question = user.split("Current question:").nth(1).unwrap_or(user);
                let (points, answer) = match self.data.find(question) {
                    Some(q) => (&q.points, &q.answer),
                    None => (&self.data.fallback.points, &self.data.fallback.answer),
                };
                let mut s = String::new();
                for p in points {
                    s.push_str(&json!({"n": p.n, "head": p.head, "rest": p.rest}).to_string());
                    s.push('\n');
                }
                s.push_str("---\n");
                s.push_str(answer);
                s
            }
            UsagePurpose::FollowUp => self.data.follow_up.clone(),
            UsagePurpose::Review => serde_json::to_string(&self.data.review).unwrap_or_default(),
            UsagePurpose::Classify => {
                let items: Vec<_> = user
                    .lines()
                    .filter_map(|l| {
                        let (i, text) = l.split_once(' ')?;
                        let i: u32 = i.trim_end_matches(['.', ':']).parse().ok()?;
                        let kind = if might_be_question(text) {
                            "question"
                        } else {
                            "answer"
                        };
                        Some(json!({"i": i, "type": kind}))
                    })
                    .collect();
                serde_json::Value::Array(items).to_string()
            }
            UsagePurpose::Asr => String::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for MockLlm {
    fn id(&self) -> &'static str {
        "mock"
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, MeetCueError> {
        Ok(vec![ModelInfo {
            id: "mock-fast".into(),
            display_name: "Mock (fast)".into(),
        }])
    }

    async fn stream(&self, req: LlmRequest) -> Result<LlmStream, MeetCueError> {
        let text = self.reply(&req);
        let system_tokens = (req.system.chars().count() / 3) as u64;
        let user_tokens = (req
            .messages
            .iter()
            .map(|m| m.content.chars().count())
            .sum::<usize>()
            / 2) as u64;
        let cached = req.cache_system && self.warmed.swap(true, Ordering::Relaxed);
        let usage = LlmChunk::Usage {
            input: user_tokens + if cached { 0 } else { system_tokens },
            output: (text.chars().count() / 2).max(1) as u64,
            cache_read: if cached { system_tokens } else { 0 },
            cache_write: 0,
        };
        let (tx, rx) = mpsc::unbounded();
        let first = Duration::from_millis(self.first_token_ms);
        let every = Duration::from_millis(self.chunk_ms);
        let size = self.chunk_chars.max(1);
        tokio::spawn(async move {
            tokio::time::sleep(first).await;
            let chars: Vec<char> = text.chars().collect();
            for (i, chunk) in chars.chunks(size).enumerate() {
                if i > 0 {
                    tokio::time::sleep(every).await;
                }
                if tx
                    .unbounded_send(Ok(LlmChunk::Text(chunk.iter().collect())))
                    .is_err()
                {
                    return;
                }
            }
            let _ = tx.unbounded_send(Ok(usage));
        });
        Ok(rx.boxed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::complete;

    #[test]
    fn bundled_script_parses() {
        let s = parse_script(SDN_TRANSCRIPT).unwrap();
        assert!(s.len() >= 10);
        assert!(s.iter().any(|l| l.ch == Channel::Me));
        assert!(s.windows(2).all(|w| w[0].t0 <= w[1].t0));
    }

    #[test]
    fn schedule_is_progressive() {
        let asr = MockAsr::new(vec![ScriptLine {
            ch: Channel::Remote,
            t0: 0,
            t1: 900,
            text: "一二三四五六".into(),
        }]);
        let s = asr.schedule(Channel::Remote);
        let texts: Vec<_> = s
            .iter()
            .filter_map(|(_, r)| match r {
                AsrResult::Partial { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["一二", "一二三四", "一二三四五六"]);
        assert!(matches!(
            s.last().unwrap().1,
            AsrResult::Endpoint { t_ms: 900 }
        ));
        assert!(asr.schedule(Channel::Me).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn llm_detects_and_streams_hints() {
        let llm = MockLlm::sdn();
        let req = LlmRequest::new("mock-fast", UsagePurpose::Detect)
            .user("[Me] 好\n[Them] 控制器挂了还能转发吗？\n");
        let (text, usage) = complete(&llm, req).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["isQuestion"], true);
        assert_eq!(v["short"], "控制器故障还能转发吗");
        assert!(usage.is_some());

        let req = LlmRequest::new("mock-fast", UsagePurpose::Hints)
            .user("Current question:\n怎么做高可用的？");
        let (text, _) = complete(&llm, req).await.unwrap();
        assert!(text.starts_with("{\"head\":\"双活数据中心\"") || text.contains("双活数据中心"));
        assert!(text.contains("\n---\n"));
    }
}
