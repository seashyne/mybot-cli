//! Minimal, dependency-free SSE (text/event-stream) decoder.
//!
//! Operates on raw bytes so chunk boundaries may split anywhere, including in
//! the middle of a multi-byte UTF-8 sequence.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: Vec<u8>,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed raw bytes, returning every complete event now available.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some((index, delimiter_len)) = find_event_end(&self.buf) {
            let block: Vec<u8> = self.buf.drain(..index).collect();
            self.buf.drain(..delimiter_len);
            if let Some(event) = parse_block(&block) {
                events.push(event);
            }
        }
        events
    }

    /// Flush a trailing block that was not terminated by a blank line.
    pub fn finish(&mut self) -> Option<SseEvent> {
        if self.buf.is_empty() {
            return None;
        }
        let block = std::mem::take(&mut self.buf);
        parse_block(&block)
    }
}

/// Returns `(index_of_delimiter, delimiter_len)` for the first blank line.
fn find_event_end(buf: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < buf.len() {
        if buf[i] == b'\n' && buf.get(i + 1) == Some(&b'\n') {
            return Some((i, 2));
        }
        if buf[i] == b'\r' && buf.get(i + 1) == Some(&b'\n')
            && buf.get(i + 2) == Some(&b'\r') && buf.get(i + 3) == Some(&b'\n') {
                return Some((i, 4));
            }
        i += 1;
    }
    None
}

fn parse_block(block: &[u8]) -> Option<SseEvent> {
    let text = String::from_utf8_lossy(block);
    let mut event = None;
    let mut data_lines: Vec<&str> = Vec::new();

    for raw_line in text.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim_start().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.strip_prefix(' ').unwrap_or(value));
        }
        // `id:` and `retry:` are intentionally ignored.
    }

    if event.is_none() && data_lines.is_empty() {
        return None;
    }

    Some(SseEvent {
        event,
        data: data_lines.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "event: message_start\ndata: {\"a\":1}\n\nevent: content_block_delta\ndata: {\"b\":\ndata: 2}\n\n";

    #[test]
    fn parses_two_events() {
        let mut decoder = SseDecoder::new();
        let events = decoder.push(FIXTURE.as_bytes());
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.as_deref(), Some("message_start"));
        assert_eq!(events[0].data, "{\"a\":1}");
        assert_eq!(events[1].event.as_deref(), Some("content_block_delta"));
        // Multi-line data is joined with a newline.
        assert_eq!(events[1].data, "{\"b\":\n2}");
    }

    #[test]
    fn survives_every_chunk_boundary() {
        let bytes = FIXTURE.as_bytes();
        for split in 0..=bytes.len() {
            let mut decoder = SseDecoder::new();
            let mut events = decoder.push(&bytes[..split]);
            events.extend(decoder.push(&bytes[split..]));
            assert_eq!(events.len(), 2, "split at {split}");
            assert_eq!(events[0].data, "{\"a\":1}", "split at {split}");
        }
    }

    #[test]
    fn survives_byte_at_a_time() {
        let mut decoder = SseDecoder::new();
        let mut events = Vec::new();
        for byte in FIXTURE.as_bytes() {
            events.extend(decoder.push(&[*byte]));
        }
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn handles_crlf() {
        let mut decoder = SseDecoder::new();
        let events = decoder.push(b"event: ping\r\ndata: {}\r\n\r\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("ping"));
        assert_eq!(events[0].data, "{}");
    }

    #[test]
    fn handles_comments_and_openai_done() {
        let mut decoder = SseDecoder::new();
        let events = decoder.push(b": keep-alive\n\ndata: [DONE]\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, None);
        assert_eq!(events[0].data, "[DONE]");
    }

    #[test]
    fn flushes_unterminated_tail() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push(b"data: {\"x\":1}").is_empty());
        let tail = decoder.finish().expect("tail event");
        assert_eq!(tail.data, "{\"x\":1}");
    }

    #[test]
    fn utf8_split_inside_multibyte_char() {
        // "é" is 0xC3 0xA9; split between the two bytes.
        let payload = "data: é\n\n".as_bytes();
        let mut decoder = SseDecoder::new();
        let mut events = decoder.push(&payload[..7]);
        events.extend(decoder.push(&payload[7..]));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "é");
    }
}
