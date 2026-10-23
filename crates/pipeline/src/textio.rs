//! Reading user files as text: encoding detection (UTF-8, UTF-16 with BOM,
//! GBK and other legacy encodings), BOM removal and line-ending cleanup.

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{UTF_16BE, UTF_16LE, UTF_8};

/// Decodes bytes of unknown encoding into a string with `\n` line endings.
pub fn decode(bytes: &[u8]) -> String {
    let text = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        UTF_8.decode_without_bom_handling(rest).0.into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        UTF_16LE.decode_without_bom_handling(rest).0.into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        UTF_16BE.decode_without_bom_handling(rest).0.into_owned()
    } else if let Ok(s) = std::str::from_utf8(bytes) {
        s.to_string()
    } else {
        let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
        detector.feed(bytes, true);
        let encoding = detector.guess(None, Utf8Detection::Allow);
        encoding.decode_without_bom_handling(bytes).0.into_owned()
    };
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_common_encodings() {
        assert_eq!(decode(b"\xEF\xBB\xBFhello\r\nworld"), "hello\nworld");
        let (gbk, _, _) =
            encoding_rs::GBK.encode("你们的网络架构是怎么做高可用的？请详细说明一下。");
        assert_eq!(
            decode(&gbk),
            "你们的网络架构是怎么做高可用的？请详细说明一下。"
        );
        let mut utf16 = vec![0xFF, 0xFE];
        for u in "问题".encode_utf16() {
            utf16.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode(&utf16), "问题");
        assert_eq!(decode("纯文本\r旧 Mac".as_bytes()), "纯文本\n旧 Mac");
    }
}

/// Reads a 16 kHz mono 16-bit PCM WAV file (the bundled probe audio).
pub fn wav_pcm16(bytes: &[u8]) -> Option<Vec<i16>> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12;
    let mut format_ok = false;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = bytes.get(pos + 8..pos + 8 + len)?;
        match id {
            b"fmt " => {
                let audio_format = u16::from_le_bytes(body.get(0..2)?.try_into().ok()?);
                let channels = u16::from_le_bytes(body.get(2..4)?.try_into().ok()?);
                let rate = u32::from_le_bytes(body.get(4..8)?.try_into().ok()?);
                let bits = u16::from_le_bytes(body.get(14..16)?.try_into().ok()?);
                format_ok = audio_format == 1 && channels == 1 && rate == 16_000 && bits == 16;
            }
            b"data" if format_ok => {
                return Some(
                    body.chunks_exact(2)
                        .map(|c| i16::from_le_bytes([c[0], c[1]]))
                        .collect(),
                );
            }
            _ => {}
        }
        pos += 8 + len + (len & 1);
    }
    None
}

#[cfg(test)]
mod wav_tests {
    use super::*;

    fn wav(samples: &[i16], rate: u32) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn reads_pcm16_mono_16k_only() {
        assert_eq!(wav_pcm16(&wav(&[1, -2, 3], 16_000)), Some(vec![1, -2, 3]));
        assert_eq!(wav_pcm16(&wav(&[1], 44_100)), None);
        assert_eq!(wav_pcm16(b"nope"), None);
    }

    #[test]
    fn bundled_probe_audio_is_valid() {
        let audio =
            wav_pcm16(include_bytes!("../../../samples/audio/probe-zh-en.wav")).expect("probe wav");
        let secs = audio.len() as f64 / 16_000.0;
        assert!((5.0..=12.0).contains(&secs), "{secs} s");
    }
}
