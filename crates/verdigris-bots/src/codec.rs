use verdigris_protocol::{
    offline_uuid, read_string, read_var_int, write_f64, write_string, write_u16, write_var_int,
};

pub const PROTOCOL_VERSION: i32 = 777;
pub const GAME_VERSION: &str = "26.3";
pub const CORE_NAMESPACE: &str = "minecraft";
pub const CORE_ID: &str = "core";

pub const INTENT_LOGIN: i32 = 2;

pub mod handshake {
    pub const HANDSHAKE: i32 = 0x00;
}

pub mod login_sb {
    pub const HELLO: i32 = 0x00;
    pub const ACKNOWLEDGED: i32 = 0x03;
}

pub mod login_cb {
    pub const DISCONNECT: i32 = 0x00;
    pub const HELLO: i32 = 0x01;
    pub const FINISHED: i32 = 0x02;
    pub const SET_COMPRESSION: i32 = 0x03;
}

pub mod config_sb {
    pub const CLIENT_INFORMATION: i32 = 0x00;
    pub const FINISH_CONFIGURATION: i32 = 0x03;
    pub const KEEP_ALIVE: i32 = 0x04;
    pub const PONG: i32 = 0x05;
    pub const SELECT_KNOWN_PACKS: i32 = 0x07;
}

pub mod config_cb {
    pub const DISCONNECT: i32 = 0x02;
    pub const FINISH_CONFIGURATION: i32 = 0x03;
    pub const KEEP_ALIVE: i32 = 0x04;
    pub const PING: i32 = 0x05;
    pub const SELECT_KNOWN_PACKS: i32 = 0x0F;
}

pub mod play_sb {
    pub const ACCEPT_TELEPORTATION: i32 = 0x00;
    pub const CHUNK_BATCH_RECEIVED: i32 = 0x0B;
    pub const CLIENT_TICK_END: i32 = 0x0D;
    pub const KEEP_ALIVE: i32 = 0x1C;
    pub const MOVE_PLAYER_POS: i32 = 0x1E;
    pub const PLAYER_LOADED: i32 = 0x2C;
    pub const PONG: i32 = 0x2D;
}

pub mod play_cb {
    pub const CHUNK_BATCH_FINISHED: i32 = 0x0B;
    pub const DISCONNECT: i32 = 0x20;
    pub const KEEP_ALIVE: i32 = 0x2D;
    pub const LEVEL_CHUNK_WITH_LIGHT: i32 = 0x2E;
    pub const PING: i32 = 0x3E;
    pub const PLAYER_POSITION: i32 = 0x49;
}

pub const MOVE_FLAG_ON_GROUND: u8 = 0x01;
pub const SKIN_PARTS_ALL: u8 = 0x7F;
pub const MAIN_HAND_RIGHT: i32 = 1;
pub const CHUNK_BATCH_DESIRED_PER_TICK: f32 = 25.0;

pub fn offline_uuid_bytes(name: &str) -> [u8; 16] {
    offline_uuid(name).to_be_bytes()
}

pub fn handshake_payload(address: &str, port: u16, intent: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(PROTOCOL_VERSION, &mut payload);
    write_string(address, &mut payload);
    write_u16(port, &mut payload);
    write_var_int(intent, &mut payload);
    payload
}

pub fn login_hello_payload(name: &str, uuid: [u8; 16]) -> Vec<u8> {
    let mut payload = Vec::new();
    write_string(name, &mut payload);
    payload.extend_from_slice(&uuid);
    payload
}

pub fn login_acknowledged_payload() -> Vec<u8> {
    Vec::new()
}

pub fn client_information_payload(view_distance: u8) -> Vec<u8> {
    let mut payload = Vec::new();
    write_string("fr_fr", &mut payload);
    payload.push(view_distance);
    write_var_int(0, &mut payload);
    payload.push(1);
    payload.push(SKIN_PARTS_ALL);
    write_var_int(MAIN_HAND_RIGHT, &mut payload);
    payload.push(0);
    payload.push(1);
    write_var_int(0, &mut payload);
    payload
}

pub fn select_known_packs_reply() -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(1, &mut payload);
    write_string(CORE_NAMESPACE, &mut payload);
    write_string(CORE_ID, &mut payload);
    write_string(GAME_VERSION, &mut payload);
    payload
}

pub fn keep_alive_payload(id: i64) -> Vec<u8> {
    id.to_be_bytes().to_vec()
}

pub fn pong_config_payload(id: i32) -> Vec<u8> {
    id.to_be_bytes().to_vec()
}

pub fn finish_configuration_payload() -> Vec<u8> {
    Vec::new()
}

pub fn accept_teleportation_payload(position: &PlayerPosition) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(position.teleport_id, &mut payload);
    write_f64(position.x, &mut payload);
    write_f64(position.y, &mut payload);
    write_f64(position.z, &mut payload);
    payload.extend_from_slice(&position.yaw.to_be_bytes());
    payload.extend_from_slice(&position.pitch.to_be_bytes());
    payload
}

pub fn player_loaded_payload() -> Vec<u8> {
    Vec::new()
}

pub fn chunk_batch_received_payload(desired: f32) -> Vec<u8> {
    desired.to_be_bytes().to_vec()
}

pub fn pong_play_payload(id: i32) -> Vec<u8> {
    id.to_be_bytes().to_vec()
}

pub fn move_player_pos_payload(x: f64, y: f64, z: f64, flags: u8) -> Vec<u8> {
    let mut payload = Vec::new();
    write_f64(x, &mut payload);
    write_f64(y, &mut payload);
    write_f64(z, &mut payload);
    payload.push(flags);
    payload
}

pub fn client_tick_end_payload() -> Vec<u8> {
    Vec::new()
}

pub struct PlayerPosition {
    pub teleport_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

pub fn decode_player_position(body: &[u8]) -> Option<PlayerPosition> {
    let (teleport_id, mut off) = read_var_int(body).ok()??;
    let x = read_f64_at(body, &mut off)?;
    let y = read_f64_at(body, &mut off)?;
    let z = read_f64_at(body, &mut off)?;
    for _ in 0..3 {
        read_f64_at(body, &mut off)?;
    }
    let yaw = read_f32_at(body, &mut off)?;
    let pitch = read_f32_at(body, &mut off)?;
    Some(PlayerPosition {
        teleport_id,
        x,
        y,
        z,
        yaw,
        pitch,
    })
}

pub fn decode_keep_alive(body: &[u8]) -> Option<i64> {
    if body.len() < 8 {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&body[..8]);
    Some(i64::from_be_bytes(bytes))
}

pub fn decode_ping(body: &[u8]) -> Option<i32> {
    if body.len() < 4 {
        return None;
    }
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&body[..4]);
    Some(i32::from_be_bytes(bytes))
}

pub fn decode_set_compression(body: &[u8]) -> Option<i32> {
    let (threshold, _) = read_var_int(body).ok()??;
    Some(threshold)
}

fn read_f32_at(body: &[u8], off: &mut usize) -> Option<f32> {
    if body.len() < *off + 4 {
        return None;
    }
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&body[*off..*off + 4]);
    *off += 4;
    Some(f32::from_be_bytes(bytes))
}

fn read_f64_at(body: &[u8], off: &mut usize) -> Option<f64> {
    if body.len() < *off + 8 {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&body[*off..*off + 8]);
    *off += 8;
    Some(f64::from_be_bytes(bytes))
}

pub fn parse_disconnect_reason(body: &[u8]) -> Option<String> {
    if let Some(text) = parse_nbt_string_reason(body) {
        return Some(text);
    }
    if let Some((json, _)) = read_string(body, verdigris_protocol::MAX_STRING_LEN).ok()? {
        return Some(extract_component_text(&json));
    }
    None
}

fn parse_nbt_string_reason(body: &[u8]) -> Option<String> {
    if body.first() != Some(&0x08) || body.len() < 3 {
        return None;
    }
    let len = u16::from_be_bytes([body[1], body[2]]) as usize;
    let start = 3;
    let end = start + len;
    if body.len() < end {
        return None;
    }
    decode_mutf8(&body[start..end])
}

fn extract_component_text(json: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(serde_json::Value::Object(map)) => map
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned())
            .unwrap_or_else(|| json.to_owned()),
        _ => json.to_owned(),
    }
}

fn decode_mutf8(bytes: &[u8]) -> Option<String> {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0 {
            return None;
        }
        if b < 0x80 {
            out.push(b as char);
            i += 1;
        } else if b & 0xE0 == 0xC0 {
            if i + 1 >= bytes.len() {
                return None;
            }
            let b2 = bytes[i + 1];
            let code = (((b & 0x1F) as u32) << 6) | ((b2 & 0x3F) as u32);
            out.push(char::from_u32(code)?);
            i += 2;
        } else if b & 0xF0 == 0xE0 {
            if i + 2 >= bytes.len() {
                return None;
            }
            let b2 = bytes[i + 1];
            let b3 = bytes[i + 2];
            let code =
                (((b & 0x0F) as u32) << 12) | (((b2 & 0x3F) as u32) << 6) | ((b3 & 0x3F) as u32);
            out.push(char::from_u32(code)?);
            i += 3;
        } else {
            return None;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_exact_bytes() {
        let payload = handshake_payload("127.0.0.1", 25565, INTENT_LOGIN);
        let mut expected = vec![0x89, 0x06, 0x09];
        expected.extend_from_slice(b"127.0.0.1");
        expected.extend_from_slice(&[0x63, 0xDD, 0x02]);
        assert_eq!(payload, expected);
    }

    #[test]
    fn offline_uuid_verdi_frozen_vector() {
        let uuid = offline_uuid_bytes("Verdi");
        assert_eq!(
            uuid,
            [
                0xaa, 0x17, 0xce, 0xf9, 0xd8, 0x07, 0x39, 0xfc, 0xbf, 0xaa, 0x77, 0x41, 0xe2, 0xee,
                0x90, 0x33
            ]
        );
        assert_eq!(uuid[6] & 0xF0, 0x30);
        assert_eq!(uuid[8] & 0xC0, 0x80);
    }

    #[test]
    fn login_hello_exact_bytes() {
        let uuid = offline_uuid_bytes("Verdi");
        let payload = login_hello_payload("Verdi", uuid);
        let mut expected = vec![0x05];
        expected.extend_from_slice(b"Verdi");
        expected.extend_from_slice(&uuid);
        assert_eq!(payload, expected);
    }

    #[test]
    fn client_information_exact_bytes() {
        let payload = client_information_payload(8);
        let expected = vec![
            0x05, b'f', b'r', b'_', b'f', b'r', 0x08, 0x00, 0x01, 0x7F, 0x01, 0x00, 0x01, 0x00,
        ];
        assert_eq!(payload, expected);
    }

    #[test]
    fn move_player_pos_exact_bytes() {
        let payload = move_player_pos_payload(1.5, -60.0, 0.25, MOVE_FLAG_ON_GROUND);
        let mut expected = Vec::new();
        expected.extend_from_slice(&1.5f64.to_be_bytes());
        expected.extend_from_slice(&(-60.0f64).to_be_bytes());
        expected.extend_from_slice(&0.25f64.to_be_bytes());
        expected.push(0x01);
        assert_eq!(payload, expected);
    }

    #[test]
    fn select_known_packs_reply_is_core() {
        let payload = select_known_packs_reply();
        let mut expected = vec![0x01, 0x09];
        expected.extend_from_slice(b"minecraft");
        expected.push(0x04);
        expected.extend_from_slice(b"core");
        expected.push(0x04);
        expected.extend_from_slice(b"26.3");
        assert_eq!(payload, expected);
    }

    #[test]
    fn decode_player_position_reads_coordinates() {
        let mut body = Vec::new();
        write_var_int(7, &mut body);
        body.extend_from_slice(&0.5f64.to_be_bytes());
        body.extend_from_slice(&64.0f64.to_be_bytes());
        body.extend_from_slice(&(-0.5f64).to_be_bytes());
        body.extend_from_slice(&[0u8; 24]);
        body.extend_from_slice(&90.0f32.to_be_bytes());
        body.extend_from_slice(&(-15.0f32).to_be_bytes());
        body.extend_from_slice(&0i32.to_be_bytes());
        let decoded = decode_player_position(&body).unwrap();
        assert_eq!(decoded.teleport_id, 7);
        assert_eq!((decoded.x, decoded.y, decoded.z), (0.5, 64.0, -0.5));
        assert_eq!((decoded.yaw, decoded.pitch), (90.0, -15.0));
    }

    #[test]
    fn accept_teleportation_echoes_position_and_rotation() {
        let position = PlayerPosition {
            teleport_id: 300,
            x: 0.5,
            y: -60.0,
            z: 0.5,
            yaw: 90.0,
            pitch: -12.5,
        };
        let payload = accept_teleportation_payload(&position);
        let decoded = verdigris_protocol::decode_accept_teleportation(&payload).unwrap();
        assert_eq!(decoded.teleport_id, 300);
        assert_eq!((decoded.x, decoded.y, decoded.z), (0.5, -60.0, 0.5));
        assert_eq!((decoded.yaw, decoded.pitch), (90.0, -12.5));
    }

    #[test]
    fn disconnect_reason_json_object() {
        let mut body = Vec::new();
        write_string("{\"text\":\"adieu\"}", &mut body);
        assert_eq!(parse_disconnect_reason(&body), Some("adieu".to_owned()));
    }

    #[test]
    fn disconnect_reason_json_string() {
        let mut body = Vec::new();
        write_string("\"au revoir\"", &mut body);
        assert_eq!(parse_disconnect_reason(&body), Some("au revoir".to_owned()));
    }

    #[test]
    fn disconnect_reason_nbt_string() {
        let text = "trop de monde";
        let mut body = vec![0x08];
        body.extend_from_slice(&(text.len() as u16).to_be_bytes());
        body.extend_from_slice(text.as_bytes());
        assert_eq!(parse_disconnect_reason(&body), Some(text.to_owned()));
    }

    #[test]
    fn disconnect_reason_unreadable_is_none() {
        let body = [0xFF, 0xFE, 0xFD];
        assert_eq!(parse_disconnect_reason(&body), None);
    }
}
