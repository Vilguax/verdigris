use crate::framing::{encode_compressed_frame, encode_frame};
use crate::primitives::{write_string, write_u16};
use crate::varint::write_var_int;
use crate::{MAX_PACKET_LEN, MAX_STRING_LEN};

pub const MAX_OUTBOUND_PACKET_LEN: usize = MAX_PACKET_LEN;
pub const NBT_TAG_STRING: u8 = 0x08;
pub const MAX_NBT_STRING_BYTES: usize = u16::MAX as usize;
pub const SET_COMPRESSION_ID: i32 = 0x03;
pub const COMPRESSION_THRESHOLD: i32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    PacketTooLarge,
    NameTooLong,
}

impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EncodeError::PacketTooLarge => write!(f, "packet too large"),
            EncodeError::NameTooLong => write!(f, "name exceeds 16 characters"),
        }
    }
}

impl std::error::Error for EncodeError {}

pub fn build_packet(id: i32, payload: &[u8]) -> Result<Vec<u8>, EncodeError> {
    let mut body = Vec::with_capacity(payload.len() + 5);
    write_var_int(id, &mut body);
    body.extend_from_slice(payload);
    if body.len() > MAX_OUTBOUND_PACKET_LEN {
        return Err(EncodeError::PacketTooLarge);
    }
    Ok(encode_frame(&body))
}

pub fn build_compressed_packet(
    id: i32,
    payload: &[u8],
    threshold: usize,
) -> Result<Vec<u8>, EncodeError> {
    let mut body = Vec::with_capacity(payload.len() + 5);
    write_var_int(id, &mut body);
    body.extend_from_slice(payload);
    if body.len() > MAX_OUTBOUND_PACKET_LEN {
        return Err(EncodeError::PacketTooLarge);
    }
    Ok(encode_compressed_frame(&body, threshold))
}

pub fn set_compression_payload(threshold: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(threshold, &mut payload);
    payload
}

pub fn login_disconnect_payload(reason: &str) -> Vec<u8> {
    let json = text_component_json(reason);
    let mut payload = Vec::new();
    write_string(&json, &mut payload);
    payload
}

pub fn config_disconnect_payload(reason: &str) -> Vec<u8> {
    text_component_nbt(reason)
}

pub fn text_component_nbt(text: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(NBT_TAG_STRING);
    write_nbt_network_string(text, &mut payload);
    payload
}

fn text_component_json(reason: &str) -> String {
    let prefix = "{\"text\":\"";
    let suffix = "\"}";
    let budget = MAX_STRING_LEN.saturating_sub(prefix.chars().count() + suffix.chars().count());
    let mut escaped = String::new();
    let mut used = 0usize;
    for ch in reason.chars() {
        let piece = escape_json_char(ch);
        let piece_len = piece.chars().count();
        if used + piece_len > budget {
            break;
        }
        escaped.push_str(&piece);
        used += piece_len;
    }
    let mut json = String::with_capacity(prefix.len() + escaped.len() + suffix.len());
    json.push_str(prefix);
    json.push_str(&escaped);
    json.push_str(suffix);
    json
}

fn escape_json_char(ch: char) -> String {
    match ch {
        '"' => "\\\"".to_owned(),
        '\\' => "\\\\".to_owned(),
        '\n' => "\\n".to_owned(),
        '\r' => "\\r".to_owned(),
        '\t' => "\\t".to_owned(),
        '\u{08}' => "\\b".to_owned(),
        '\u{0C}' => "\\f".to_owned(),
        c if (c as u32) < 0x20 => format!("\\u{:04x}", c as u32),
        c => c.to_string(),
    }
}

fn write_nbt_network_string(text: &str, out: &mut Vec<u8>) {
    let mut bytes = Vec::new();
    for ch in text.chars() {
        if bytes.len() + mutf8_len(ch) > MAX_NBT_STRING_BYTES {
            break;
        }
        push_mutf8_char(ch, &mut bytes);
    }
    write_u16(bytes.len() as u16, out);
    out.extend_from_slice(&bytes);
}

fn mutf8_len(ch: char) -> usize {
    let code = ch as u32;
    if code == 0 {
        2
    } else if code < 0x80 {
        1
    } else if code < 0x800 {
        2
    } else if code < 0x10000 {
        3
    } else {
        6
    }
}

fn push_mutf8_char(ch: char, out: &mut Vec<u8>) {
    let code = ch as u32;
    if code == 0 {
        out.push(0xC0);
        out.push(0x80);
    } else if code < 0x80 {
        out.push(code as u8);
    } else if code < 0x800 {
        out.push(0xC0 | (code >> 6) as u8);
        out.push(0x80 | (code & 0x3F) as u8);
    } else if code < 0x10000 {
        push_three_byte(code, out);
    } else {
        let c = code - 0x10000;
        let hi = 0xD800 + (c >> 10);
        let lo = 0xDC00 + (c & 0x3FF);
        push_three_byte(hi, out);
        push_three_byte(lo, out);
    }
}

fn push_three_byte(code: u32, out: &mut Vec<u8>) {
    out.push(0xE0 | (code >> 12) as u8);
    out.push(0x80 | ((code >> 6) & 0x3F) as u8);
    out.push(0x80 | (code & 0x3F) as u8);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::read_string;

    #[test]
    fn build_packet_frames_id_and_payload() {
        let framed = build_packet(0x00, &[0xDE, 0xAD]).unwrap();
        assert_eq!(framed, vec![3, 0x00, 0xDE, 0xAD]);
    }

    #[test]
    fn build_packet_rejects_oversize() {
        let huge = vec![0u8; MAX_OUTBOUND_PACKET_LEN];
        assert_eq!(build_packet(0x00, &huge), Err(EncodeError::PacketTooLarge));
    }

    #[test]
    fn login_disconnect_escapes_nul_and_emoji() {
        let json = "{\"text\":\"\\u0000\u{1F600}\"}";
        let mut expected = Vec::new();
        write_var_int(json.len() as i32, &mut expected);
        expected.extend_from_slice(json.as_bytes());
        assert_eq!(login_disconnect_payload("\u{0}\u{1F600}"), expected);
    }

    #[test]
    fn login_disconnect_truncates_to_string_limit() {
        let reason = "a".repeat(40000);
        let payload = login_disconnect_payload(&reason);
        let (json, _) = read_string(&payload, MAX_STRING_LEN).unwrap().unwrap();
        assert_eq!(json.chars().count(), MAX_STRING_LEN);
        assert!(json.starts_with("{\"text\":\""));
        assert!(json.ends_with("\"}"));
    }

    #[test]
    fn config_disconnect_encodes_nul_and_emoji_mutf8() {
        let expected = vec![
            0x08, 0x00, 0x08, 0xC0, 0x80, 0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80,
        ];
        assert_eq!(config_disconnect_payload("\u{0}\u{1F600}"), expected);
    }

    #[test]
    fn config_disconnect_truncates_without_splitting_char() {
        let reason = "\u{1F600}".repeat(20000);
        let payload = config_disconnect_payload(&reason);
        assert_eq!(payload[0], NBT_TAG_STRING);
        let len = u16::from_be_bytes([payload[1], payload[2]]) as usize;
        assert_eq!(len, 65532);
        assert_eq!(payload.len(), 3 + 65532);
        assert_eq!(len % 6, 0);
    }
}
