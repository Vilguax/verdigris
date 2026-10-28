use crate::error::ProtocolError;
use crate::primitives::{read_i64, read_string, read_u16};
use crate::varint::read_var_int;
use crate::{MAX_ADDRESS_LEN, MAX_USERNAME_LEN, State};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Status,
    Login,
    Transfer,
    Unknown(i32),
}

impl Intent {
    pub fn from_raw(raw: i32) -> Intent {
        match raw {
            1 => Intent::Status,
            2 => Intent::Login,
            3 => Intent::Transfer,
            other => Intent::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    pub protocol_version: i32,
    pub server_address: String,
    pub server_port: u16,
    pub intent: Intent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Serverbound {
    Handshake(Handshake),
    StatusRequest,
    PingRequest { payload: i64 },
    LoginStart { name: String, uuid: u128 },
}

fn take_var_int(cursor: &mut &[u8]) -> Result<i32, ProtocolError> {
    match read_var_int(cursor)? {
        Some((value, consumed)) => {
            *cursor = &cursor[consumed..];
            Ok(value)
        }
        None => Err(ProtocolError::UnexpectedEof),
    }
}

fn take_string(cursor: &mut &[u8], max_len: usize) -> Result<String, ProtocolError> {
    match read_string(cursor, max_len)? {
        Some((value, consumed)) => {
            *cursor = &cursor[consumed..];
            Ok(value)
        }
        None => Err(ProtocolError::UnexpectedEof),
    }
}

fn take_u16(cursor: &mut &[u8]) -> Result<u16, ProtocolError> {
    match read_u16(cursor)? {
        Some((value, consumed)) => {
            *cursor = &cursor[consumed..];
            Ok(value)
        }
        None => Err(ProtocolError::UnexpectedEof),
    }
}

fn take_i64(cursor: &mut &[u8]) -> Result<i64, ProtocolError> {
    match read_i64(cursor)? {
        Some((value, consumed)) => {
            *cursor = &cursor[consumed..];
            Ok(value)
        }
        None => Err(ProtocolError::UnexpectedEof),
    }
}

fn take_u128(cursor: &mut &[u8]) -> Result<u128, ProtocolError> {
    if cursor.len() < 16 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&cursor[..16]);
    *cursor = &cursor[16..];
    Ok(u128::from_be_bytes(bytes))
}

pub fn decode_serverbound(state: State, body: &[u8]) -> Result<Serverbound, ProtocolError> {
    let mut cursor = body;
    let id = take_var_int(&mut cursor)?;
    match (state, id) {
        (State::Handshaking, 0x00) => {
            let protocol_version = take_var_int(&mut cursor)?;
            let server_address = take_string(&mut cursor, MAX_ADDRESS_LEN)?;
            let server_port = take_u16(&mut cursor)?;
            let intent = Intent::from_raw(take_var_int(&mut cursor)?);
            Ok(Serverbound::Handshake(Handshake {
                protocol_version,
                server_address,
                server_port,
                intent,
            }))
        }
        (State::Status, 0x00) => Ok(Serverbound::StatusRequest),
        (State::Status, 0x01) => {
            let payload = take_i64(&mut cursor)?;
            Ok(Serverbound::PingRequest { payload })
        }
        (State::Login, 0x00) => {
            let name = take_string(&mut cursor, MAX_USERNAME_LEN)?;
            let uuid = take_u128(&mut cursor)?;
            Ok(Serverbound::LoginStart { name, uuid })
        }
        (state, id) => Err(ProtocolError::UnknownPacketId { state, id }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{write_i64, write_string, write_u16};
    use crate::varint::write_var_int;

    fn handshake_body(protocol: i32, address: &str, port: u16, next_state: i32) -> Vec<u8> {
        let mut body = Vec::new();
        write_var_int(0x00, &mut body);
        write_var_int(protocol, &mut body);
        write_string(address, &mut body);
        write_u16(port, &mut body);
        write_var_int(next_state, &mut body);
        body
    }

    #[test]
    fn decode_handshake_status_intent() {
        let body = handshake_body(777, "mc.example.com", 25565, 1);
        let packet = decode_serverbound(State::Handshaking, &body).unwrap();
        assert_eq!(
            packet,
            Serverbound::Handshake(Handshake {
                protocol_version: 777,
                server_address: "mc.example.com".to_owned(),
                server_port: 25565,
                intent: Intent::Status,
            })
        );
    }

    #[test]
    fn decode_handshake_login_intent() {
        let body = handshake_body(777, "host", 25565, 2);
        let packet = decode_serverbound(State::Handshaking, &body).unwrap();
        match packet {
            Serverbound::Handshake(h) => assert_eq!(h.intent, Intent::Login),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn decode_handshake_transfer_intent() {
        let body = handshake_body(777, "host", 25565, 3);
        let packet = decode_serverbound(State::Handshaking, &body).unwrap();
        match packet {
            Serverbound::Handshake(h) => assert_eq!(h.intent, Intent::Transfer),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn decode_status_request() {
        let mut body = Vec::new();
        write_var_int(0x00, &mut body);
        assert_eq!(
            decode_serverbound(State::Status, &body).unwrap(),
            Serverbound::StatusRequest
        );
    }

    #[test]
    fn decode_ping_request() {
        let mut body = Vec::new();
        write_var_int(0x01, &mut body);
        write_i64(0x1122334455667788, &mut body);
        assert_eq!(
            decode_serverbound(State::Status, &body).unwrap(),
            Serverbound::PingRequest {
                payload: 0x1122334455667788
            }
        );
    }

    #[test]
    fn unknown_id_in_handshaking_rejected() {
        let mut body = Vec::new();
        write_var_int(0x05, &mut body);
        assert_eq!(
            decode_serverbound(State::Handshaking, &body),
            Err(ProtocolError::UnknownPacketId {
                state: State::Handshaking,
                id: 0x05
            })
        );
    }

    #[test]
    fn unknown_id_in_status_rejected() {
        let mut body = Vec::new();
        write_var_int(0x09, &mut body);
        assert_eq!(
            decode_serverbound(State::Status, &body),
            Err(ProtocolError::UnknownPacketId {
                state: State::Status,
                id: 0x09
            })
        );
    }

    #[test]
    fn decode_login_start() {
        let mut body = Vec::new();
        write_var_int(0x00, &mut body);
        write_string("Verdi", &mut body);
        body.extend_from_slice(&[0u8; 16]);
        match decode_serverbound(State::Login, &body).unwrap() {
            Serverbound::LoginStart { name, uuid } => {
                assert_eq!(name, "Verdi");
                assert_eq!(uuid, 0);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn login_start_missing_uuid_is_eof() {
        let mut body = Vec::new();
        write_var_int(0x00, &mut body);
        write_string("Verdi", &mut body);
        assert_eq!(
            decode_serverbound(State::Login, &body),
            Err(ProtocolError::UnexpectedEof)
        );
    }

    #[test]
    fn unknown_id_in_login_rejected() {
        let mut body = Vec::new();
        write_var_int(0x42, &mut body);
        assert_eq!(
            decode_serverbound(State::Login, &body),
            Err(ProtocolError::UnknownPacketId {
                state: State::Login,
                id: 0x42
            })
        );
    }

    #[test]
    fn truncated_handshake_is_eof() {
        let mut body = handshake_body(777, "host", 25565, 1);
        body.truncate(3);
        assert_eq!(
            decode_serverbound(State::Handshaking, &body),
            Err(ProtocolError::UnexpectedEof)
        );
    }
}
