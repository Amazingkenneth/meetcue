//! Minimal server-sent events parser for streamed model replies.

/// One SSE event: the `event:` name (empty if absent) and its joined `data:` lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub name: String,
    pub data: String,
}

/// Incremental parser: feed bytes as they arrive, get complete events.
#[derive(Debug, Default)]
pub struct SseParser {
    buf: String,
    pending: Vec<u8>,
}

impl SseParser {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Event> {
        // Keep incomplete UTF-8 sequences until the rest arrives.
        self.pending.extend_from_slice(bytes);
        let valid = match std::str::from_utf8(&self.pending) {
            Ok(s) => s.len(),
            Err(e) => e.valid_up_to(),
        };
        let chunk: Vec<u8> = self.pending.drain(..valid).collect();
        self.buf
            .push_str(&String::from_utf8_lossy(&chunk).replace("\r\n", "\n"));
        let mut out = Vec::new();
        while let Some(end) = self.buf.find("\n\n") {
            let block: String = self.buf.drain(..end + 2).collect();
            let mut name = String::new();
            let mut data: Vec<&str> = Vec::new();
            for line in block.lines() {
                if let Some(v) = line.strip_prefix("event:") {
                    name = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("data:") {
                    data.push(v.strip_prefix(' ').unwrap_or(v));
                }
            }
            if !name.is_empty() || !data.is_empty() {
                out.push(Event {
                    name,
                    data: data.join("\n"),
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_events_across_chunks() {
        let mut p = SseParser::default();
        let raw =
            "event: message_start\ndata: {\"a\":1}\n\nevent: ping\ndata: {}\n\ndata: [DONE]\n\n";
        let bytes = raw.as_bytes();
        let mut events = Vec::new();
        for chunk in bytes.chunks(5) {
            events.extend(p.push(chunk));
        }
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0],
            Event {
                name: "message_start".into(),
                data: "{\"a\":1}".into()
            }
        );
        assert_eq!(events[2].data, "[DONE]");
    }

    #[test]
    fn keeps_split_utf8() {
        let mut p = SseParser::default();
        let raw = "data: {\"t\":\"控制器\"}\r\n\r\n".as_bytes();
        let mut events = p.push(&raw[..12]);
        events.extend(p.push(&raw[12..]));
        assert_eq!(events[0].data, "{\"t\":\"控制器\"}");
    }
}
