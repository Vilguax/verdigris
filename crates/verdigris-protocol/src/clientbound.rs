use crate::framing::{encode_compressed_frame, encode_frame};
use crate::primitives::{write_f32, write_f64, write_i16, write_string, write_u16};
use crate::text::{Component, write_nbt};
use crate::varint::{read_var_int, write_var_int};
use crate::{MAX_PACKET_LEN, MAX_STRING_LEN};

pub const MAX_OUTBOUND_PACKET_LEN: usize = MAX_PACKET_LEN;
pub const NBT_TAG_STRING: u8 = 0x08;
pub const MAX_NBT_STRING_BYTES: usize = u16::MAX as usize;
pub const SET_COMPRESSION_ID: i32 = 0x03;
pub const COMPRESSION_THRESHOLD: i32 = 256;

pub const PLAYER_ENTITY_TYPE: i32 = 159;

const ENTITY_DATA_HEALTH_INDEX: u8 = 0x09;
const ENTITY_DATA_FLOAT_SERIALIZER: u8 = 0x03;
const ENTITY_DATA_TERMINATOR: u8 = 0xFF;
const FULL_HEALTH: f32 = 20.0;

pub fn bundle_delimiter_payload() -> Vec<u8> {
    Vec::new()
}

#[allow(clippy::too_many_arguments)]
pub fn add_entity_payload(
    entity_id: i32,
    uuid: u128,
    entity_type: i32,
    x: f64,
    y: f64,
    z: f64,
    x_rot: u8,
    y_rot: u8,
    y_head_rot: u8,
    data: i32,
) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(entity_id, &mut payload);
    payload.extend_from_slice(&uuid.to_be_bytes());
    write_var_int(entity_type, &mut payload);
    write_f64(x, &mut payload);
    write_f64(y, &mut payload);
    write_f64(z, &mut payload);
    payload.push(0x00);
    payload.push(x_rot);
    payload.push(y_rot);
    payload.push(y_head_rot);
    write_var_int(data, &mut payload);
    payload
}

pub const MOVE_ENTITY_POS_ID: i32 = 0x36;
pub const MOVE_ENTITY_POS_ROT_ID: i32 = 0x37;
pub const MOVE_ENTITY_ROT_ID: i32 = 0x39;
pub const ENTITY_POSITION_SYNC_ID: i32 = 0x23;
pub const ROTATE_HEAD_ID: i32 = 0x55;

pub fn move_entity_pos(
    entity_id: i32,
    dx: i16,
    dy: i16,
    dz: i16,
    on_ground: bool,
    out: &mut Vec<u8>,
) {
    write_var_int(MOVE_ENTITY_POS_ID, out);
    write_var_int(entity_id, out);
    write_var_int(movement_props(on_ground), out);
    write_i16(dx, out);
    write_i16(dy, out);
    write_i16(dz, out);
}

#[allow(clippy::too_many_arguments)]
pub fn move_entity_pos_rot(
    entity_id: i32,
    dx: i16,
    dy: i16,
    dz: i16,
    y_rot: u8,
    x_rot: u8,
    on_ground: bool,
    out: &mut Vec<u8>,
) {
    write_var_int(MOVE_ENTITY_POS_ROT_ID, out);
    write_var_int(entity_id, out);
    write_var_int(movement_props(on_ground), out);
    write_i16(dx, out);
    write_i16(dy, out);
    write_i16(dz, out);
    out.push(y_rot);
    out.push(x_rot);
}

pub fn move_entity_rot(entity_id: i32, on_ground: bool, y_rot: u8, x_rot: u8, out: &mut Vec<u8>) {
    write_var_int(MOVE_ENTITY_ROT_ID, out);
    write_var_int(entity_id, out);
    out.push(u8::from(on_ground));
    out.push(y_rot);
    out.push(x_rot);
}

pub fn rotate_head(entity_id: i32, y_head_rot: u8, out: &mut Vec<u8>) {
    write_var_int(ROTATE_HEAD_ID, out);
    write_var_int(entity_id, out);
    out.push(y_head_rot);
}

#[allow(clippy::too_many_arguments)]
pub fn entity_position_sync(
    entity_id: i32,
    x: f64,
    y: f64,
    z: f64,
    y_rot: f32,
    x_rot: f32,
    on_ground: bool,
    out: &mut Vec<u8>,
) {
    write_var_int(ENTITY_POSITION_SYNC_ID, out);
    write_var_int(entity_id, out);
    write_var_int(0, out);
    write_f64(x, out);
    write_f64(y, out);
    write_f64(z, out);
    write_f32(y_rot, out);
    write_f32(x_rot, out);
    out.push(u8::from(on_ground));
}

fn movement_props(on_ground: bool) -> i32 {
    i32::from(on_ground)
}

pub fn remove_entities_payload(entity_ids: &[i32]) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(entity_ids.len() as i32, &mut payload);
    for &entity_id in entity_ids {
        write_var_int(entity_id, &mut payload);
    }
    payload
}

fn entity_packet_len(buf: &[u8]) -> Option<usize> {
    let (id, n0) = read_var_int(buf).ok()??;
    let mut off = n0;
    let (_entity_id, n1) = read_var_int(buf.get(off..)?).ok()??;
    off += n1;
    match id {
        MOVE_ENTITY_POS_ID => {
            let (_props, n) = read_var_int(buf.get(off..)?).ok()??;
            off += n + 6;
        }
        MOVE_ENTITY_POS_ROT_ID => {
            let (_props, n) = read_var_int(buf.get(off..)?).ok()??;
            off += n + 6 + 2;
        }
        MOVE_ENTITY_ROT_ID => {
            off += 1 + 2;
        }
        ROTATE_HEAD_ID => {
            off += 1;
        }
        ENTITY_POSITION_SYNC_ID => {
            let (_type, n) = read_var_int(buf.get(off..)?).ok()??;
            off += n + 24 + 8 + 1;
        }
        _ => return None,
    }
    if off <= buf.len() { Some(off) } else { None }
}

pub fn frame_entity_pass(raw: &[u8], threshold: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut off = 0;
    while off < raw.len() {
        let len =
            entity_packet_len(&raw[off..]).expect("les octets de passe sont toujours bien formés");
        out.extend_from_slice(&encode_compressed_frame(&raw[off..off + len], threshold));
        off += len;
    }
    out
}

pub fn set_entity_data_health_payload(entity_id: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(entity_id, &mut payload);
    payload.push(ENTITY_DATA_HEALTH_INDEX);
    payload.push(ENTITY_DATA_FLOAT_SERIALIZER);
    payload.extend_from_slice(&FULL_HEALTH.to_be_bytes());
    payload.push(ENTITY_DATA_TERMINATOR);
    payload
}

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

pub fn write_command_suggestions(
    buf: &mut Vec<u8>,
    id: i32,
    start: u32,
    length: u32,
    entries: &[(&str, Option<&Component>)],
) {
    write_var_int(id, buf);
    write_var_int(start as i32, buf);
    write_var_int(length as i32, buf);
    write_var_int(entries.len() as i32, buf);
    for (text, tooltip) in entries {
        write_string(text, buf);
        match tooltip {
            Some(component) => {
                buf.push(0x01);
                write_nbt(buf, component);
            }
            None => buf.push(0x00),
        }
    }
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

pub(crate) fn write_nbt_network_string(text: &str, out: &mut Vec<u8>) {
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
    fn bundle_delimiter_payload_is_empty() {
        assert!(bundle_delimiter_payload().is_empty());
        let framed = build_packet(0x00, &bundle_delimiter_payload()).unwrap();
        assert_eq!(framed, vec![1, 0x00]);
    }

    #[test]
    fn add_entity_payload_exact_bytes() {
        let uuid = u128::from_be_bytes([
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
            0x0E, 0x0F,
        ]);
        let payload = add_entity_payload(
            1,
            uuid,
            PLAYER_ENTITY_TYPE,
            0.5,
            -60.0,
            0.5,
            0x10,
            0x40,
            0x40,
            0,
        );

        let mut expected = Vec::new();
        expected.push(0x01);
        expected.extend_from_slice(&uuid.to_be_bytes());
        expected.extend_from_slice(&[0x9F, 0x01]);
        expected.extend_from_slice(&0.5f64.to_be_bytes());
        expected.extend_from_slice(&(-60.0f64).to_be_bytes());
        expected.extend_from_slice(&0.5f64.to_be_bytes());
        expected.push(0x00);
        expected.push(0x10);
        expected.push(0x40);
        expected.push(0x40);
        expected.push(0x00);

        assert_eq!(payload, expected);
    }

    #[test]
    fn move_entity_pos_exact_bytes() {
        let mut out = Vec::new();
        move_entity_pos(42, 4096, 0, 0, true, &mut out);
        assert_eq!(
            out,
            vec![0x36, 0x2A, 0x01, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn move_entity_pos_rot_order_yrot_then_xrot() {
        let mut out = Vec::new();
        move_entity_pos_rot(42, 4096, 0, 0, 0x40, 0xC0, false, &mut out);
        assert_eq!(
            out,
            vec![
                0x37, 0x2A, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xC0
            ]
        );
    }

    #[test]
    fn move_entity_rot_on_ground_then_angles() {
        let mut out = Vec::new();
        move_entity_rot(42, true, 0x40, 0xC0, &mut out);
        assert_eq!(out, vec![0x39, 0x2A, 0x01, 0x40, 0xC0]);
    }

    #[test]
    fn rotate_head_exact_bytes() {
        let mut out = Vec::new();
        rotate_head(42, 0x80, &mut out);
        assert_eq!(out, vec![0x55, 0x2A, 0x80]);
    }

    #[test]
    fn entity_position_sync_exact_bytes() {
        let mut out = Vec::new();
        entity_position_sync(42, 0.5, -60.0, 0.5, 90.0, -45.0, true, &mut out);

        let mut expected = vec![0x23, 0x2A, 0x00];
        expected.extend_from_slice(&0.5f64.to_be_bytes());
        expected.extend_from_slice(&(-60.0f64).to_be_bytes());
        expected.extend_from_slice(&0.5f64.to_be_bytes());
        expected.extend_from_slice(&90.0f32.to_be_bytes());
        expected.extend_from_slice(&(-45.0f32).to_be_bytes());
        expected.push(0x01);

        assert_eq!(out, expected);
    }

    #[test]
    fn frame_entity_pass_splits_move_and_head_into_two_frames() {
        use crate::framing::decode_compressed_frame;
        let mut raw = Vec::new();
        move_entity_pos(42, 4096, 0, 0, true, &mut raw);
        rotate_head(42, 0x80, &mut raw);
        let framed = frame_entity_pass(&raw, 256);

        let (first, consumed) = decode_compressed_frame(&framed, 256).unwrap().unwrap();
        assert_eq!(
            first,
            vec![0x36, 0x2A, 0x01, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
        let (second, consumed2) = decode_compressed_frame(&framed[consumed..], 256)
            .unwrap()
            .unwrap();
        assert_eq!(second, vec![0x55, 0x2A, 0x80]);
        assert_eq!(consumed + consumed2, framed.len());
    }

    #[test]
    fn frame_entity_pass_handles_position_sync() {
        use crate::framing::decode_compressed_frame;
        let mut raw = Vec::new();
        entity_position_sync(7, 0.5, -60.0, 0.5, 90.0, -45.0, true, &mut raw);
        let framed = frame_entity_pass(&raw, 256);
        let (packet, consumed) = decode_compressed_frame(&framed, 256).unwrap().unwrap();
        assert_eq!(packet, raw);
        assert_eq!(consumed, framed.len());
    }

    #[test]
    fn remove_entities_payload_single_and_triple() {
        assert_eq!(remove_entities_payload(&[7]), vec![0x01, 0x07]);
        assert_eq!(
            remove_entities_payload(&[1, 2, 3]),
            vec![0x03, 0x01, 0x02, 0x03]
        );
    }

    #[test]
    fn set_entity_data_payload_encodes_full_health() {
        assert_eq!(
            set_entity_data_health_payload(5),
            vec![0x05, 0x09, 0x03, 0x41, 0xA0, 0x00, 0x00, 0xFF]
        );
    }

    #[test]
    fn command_suggestions_gamemode_creative_bytes() {
        let mut buf = Vec::new();
        write_command_suggestions(&mut buf, 2, 10, 2, &[("creative", None)]);
        assert_eq!(
            buf,
            vec![
                0x02, 0x0a, 0x02, 0x01, 0x08, b'c', b'r', b'e', b'a', b't', b'i', b'v', b'e', 0x00
            ]
        );
    }

    #[test]
    fn command_suggestions_gamemode_all_bytes() {
        let mut buf = Vec::new();
        write_command_suggestions(
            &mut buf,
            1,
            10,
            0,
            &[
                ("adventure", None),
                ("creative", None),
                ("spectator", None),
                ("survival", None),
            ],
        );
        let mut expected = vec![0x01, 0x0a, 0x00, 0x04];
        for name in ["adventure", "creative", "spectator", "survival"] {
            expected.push(name.len() as u8);
            expected.extend_from_slice(name.as_bytes());
            expected.push(0x00);
        }
        assert_eq!(buf, expected);
    }

    #[test]
    fn command_suggestions_time_marker_bytes() {
        let mut buf = Vec::new();
        write_command_suggestions(&mut buf, 11, 10, 1, &[("minecraft:day", None)]);
        let mut expected = vec![0x0b, 0x0a, 0x01, 0x01, 0x0d];
        expected.extend_from_slice(b"minecraft:day");
        expected.push(0x00);
        assert_eq!(buf, expected);
    }

    #[test]
    fn command_suggestions_entry_with_tooltip_is_all_players() {
        let tooltip = Component::translate("argument.entity.selector.allPlayers");
        let mut buf = Vec::new();
        write_command_suggestions(&mut buf, 2, 0, 0, &[("@a", Some(&tooltip))]);
        let (header, entry) = buf.split_at(4);
        assert_eq!(header, &[0x02, 0x00, 0x00, 0x01]);
        let mut expected = vec![0x02, b'@', b'a', 0x01, 0x0a, 0x08, 0x00, 0x09];
        expected.extend_from_slice(b"translate");
        expected.push(0x00);
        expected.push(0x23);
        expected.extend_from_slice(b"argument.entity.selector.allPlayers");
        expected.push(0x00);
        assert_eq!(entry, expected.as_slice());
    }

    #[test]
    fn command_suggestions_empty_is_id_zero_zero_zero() {
        let mut buf = Vec::new();
        write_command_suggestions(&mut buf, 7, 0, 0, &[]);
        assert_eq!(buf, vec![0x07, 0x00, 0x00, 0x00]);
    }

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
