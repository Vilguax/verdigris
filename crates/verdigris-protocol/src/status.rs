use crate::error::ProtocolError;
use crate::framing::encode_frame;
use crate::primitives::{write_i64, write_string};
use crate::varint::write_var_int;
use crate::{GAME_VERSION, MAX_STATUS_JSON_LEN, PROTOCOL_VERSION};

#[derive(Debug, Clone)]
pub struct StatusInfo<'a> {
    pub version_name: &'a str,
    pub protocol: i32,
    pub max_players: i32,
    pub online_players: i32,
    pub motd: &'a str,
}

impl Default for StatusInfo<'_> {
    fn default() -> Self {
        StatusInfo {
            version_name: GAME_VERSION,
            protocol: PROTOCOL_VERSION,
            max_players: 20,
            online_players: 0,
            motd: "Verdigris",
        }
    }
}

fn escape_json(input: &str, out: &mut String) {
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
}

pub fn status_json(info: &StatusInfo) -> String {
    let mut name = String::new();
    escape_json(info.version_name, &mut name);
    let mut motd = String::new();
    escape_json(info.motd, &mut motd);
    format!(
        "{{\"version\":{{\"name\":\"{name}\",\"protocol\":{protocol}}},\
\"players\":{{\"max\":{max},\"online\":{online}}},\
\"description\":{{\"text\":\"{motd}\"}}}}",
        name = name,
        protocol = info.protocol,
        max = info.max_players,
        online = info.online_players,
        motd = motd,
    )
}

pub fn status_response_payload(json: &str) -> Result<Vec<u8>, ProtocolError> {
    if json.chars().count() > MAX_STATUS_JSON_LEN {
        return Err(ProtocolError::StringTooLong);
    }
    let mut payload = Vec::new();
    write_string(json, &mut payload);
    Ok(payload)
}

pub fn pong_payload(payload: i64) -> Vec<u8> {
    let mut out = Vec::new();
    write_i64(payload, &mut out);
    out
}

pub fn encode_status_response(json: &str) -> Result<Vec<u8>, ProtocolError> {
    let payload = status_response_payload(json)?;
    let mut body = Vec::new();
    write_var_int(0x00, &mut body);
    body.extend_from_slice(&payload);
    Ok(encode_frame(&body))
}

pub fn encode_pong(payload: i64) -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x01, &mut body);
    body.extend_from_slice(&pong_payload(payload));
    encode_frame(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::decode_frame;
    use crate::primitives::{read_i64, read_string};
    use crate::varint::read_var_int;

    #[test]
    fn status_json_contains_required_fields() {
        let json = status_json(&StatusInfo::default());
        assert!(json.contains("\"name\":\"26.3\""));
        assert!(json.contains("\"protocol\":777"));
        assert!(json.contains("\"max\":20"));
        assert!(json.contains("\"online\":0"));
        assert!(json.contains("\"description\""));
        assert!(json.chars().count() <= MAX_STATUS_JSON_LEN);
    }

    #[test]
    fn status_json_escapes_motd() {
        let info = StatusInfo {
            motd: "quote \" and \\ slash",
            ..StatusInfo::default()
        };
        let json = status_json(&info);
        assert!(json.contains("quote \\\" and \\\\ slash"));
    }

    #[test]
    fn status_response_round_trips() {
        let json = status_json(&StatusInfo::default());
        let framed = encode_status_response(&json).unwrap();
        let (body, _) = decode_frame(&framed).unwrap().unwrap();
        let (id, header) = read_var_int(body).unwrap().unwrap();
        assert_eq!(id, 0x00);
        let (decoded, _) = read_string(&body[header..], MAX_STATUS_JSON_LEN)
            .unwrap()
            .unwrap();
        assert_eq!(decoded, json);
    }

    #[test]
    fn pong_round_trips_payload() {
        let framed = encode_pong(-987654321);
        let (body, _) = decode_frame(&framed).unwrap().unwrap();
        let (id, header) = read_var_int(body).unwrap().unwrap();
        assert_eq!(id, 0x01);
        let (payload, _) = read_i64(&body[header..]).unwrap().unwrap();
        assert_eq!(payload, -987654321);
    }

    #[test]
    fn oversize_status_json_rejected() {
        let huge = "a".repeat(MAX_STATUS_JSON_LEN + 1);
        assert_eq!(
            encode_status_response(&huge),
            Err(ProtocolError::StringTooLong)
        );
    }
}
