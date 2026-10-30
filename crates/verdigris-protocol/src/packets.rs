use crate::error::ProtocolError;
use crate::play::{
    AcceptTeleportation, ChunkBatchReceived, KeepAlive, MovePlayerPos, MovePlayerPosRot,
    MovePlayerRot, MovePlayerStatus, PickItemFromBlock, PlayerAction, PlayerCommand, PlayerInput,
    SetCarriedItem, SetCreativeModeSlot, UseItemOn, decode_accept_teleportation,
    decode_chunk_batch_received, decode_keep_alive, decode_move_player_pos,
    decode_move_player_pos_rot, decode_move_player_rot, decode_move_player_status,
    decode_pick_item_from_block, decode_player_abilities, decode_player_action,
    decode_player_command, decode_player_input, decode_player_loaded, decode_set_carried_item,
    decode_set_creative_mode_slot, decode_use_item_on, packet_ids as play_ids,
};
use crate::primitives::{read_i64, read_string, read_u16};
use crate::varint::read_var_int;
use crate::{MAX_ADDRESS_LEN, MAX_STRING_LEN, State};

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

#[derive(Debug, Clone, PartialEq)]
pub enum Serverbound {
    Handshake(Handshake),
    StatusRequest,
    PingRequest { payload: i64 },
    LoginStart { name: String, uuid: u128 },
    LoginAcknowledged,
    PlayAcceptTeleportation(AcceptTeleportation),
    PlayerLoaded,
    PlayChunkBatchReceived(ChunkBatchReceived),
    PlayKeepAlive(KeepAlive),
    PlayMovePlayerPos(MovePlayerPos),
    PlayMovePlayerPosRot(MovePlayerPosRot),
    PlayMovePlayerRot(MovePlayerRot),
    PlayMovePlayerStatus(MovePlayerStatus),
    PlayPlayerAbilities { flying: bool },
    PlayPlayerAction(PlayerAction),
    PlayPlayerCommand(PlayerCommand),
    PlayPlayerInput(PlayerInput),
    PlayUseItemOn(UseItemOn),
    PlaySetCarriedItem(SetCarriedItem),
    PlaySetCreativeModeSlot(SetCreativeModeSlot),
    PlayPickItemFromBlock(PickItemFromBlock),
    PlayIgnored { id: i32 },
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
            let name = take_string(&mut cursor, MAX_STRING_LEN)?;
            let uuid = take_u128(&mut cursor)?;
            Ok(Serverbound::LoginStart { name, uuid })
        }
        (State::Login, 0x03) => {
            if !cursor.is_empty() {
                return Err(ProtocolError::TrailingBytes);
            }
            Ok(Serverbound::LoginAcknowledged)
        }
        (State::Play, _) => decode_play(id, cursor),
        (State::Configuration, id) => Err(ProtocolError::UnknownPacketId { state, id }),
        (state, id) => Err(ProtocolError::UnknownPacketId { state, id }),
    }
}

fn decode_play(id: i32, body: &[u8]) -> Result<Serverbound, ProtocolError> {
    if id == play_ids::serverbound::ACCEPT_TELEPORTATION {
        Ok(Serverbound::PlayAcceptTeleportation(
            decode_accept_teleportation(body)?,
        ))
    } else if id == play_ids::serverbound::PLAYER_LOADED {
        decode_player_loaded(body)?;
        Ok(Serverbound::PlayerLoaded)
    } else if id == play_ids::serverbound::CHUNK_BATCH_RECEIVED {
        Ok(Serverbound::PlayChunkBatchReceived(
            decode_chunk_batch_received(body)?,
        ))
    } else if id == play_ids::serverbound::KEEP_ALIVE {
        Ok(Serverbound::PlayKeepAlive(decode_keep_alive(body)?))
    } else if id == play_ids::serverbound::MOVE_PLAYER_POS {
        Ok(Serverbound::PlayMovePlayerPos(decode_move_player_pos(
            body,
        )?))
    } else if id == play_ids::serverbound::MOVE_PLAYER_POS_ROT {
        Ok(Serverbound::PlayMovePlayerPosRot(
            decode_move_player_pos_rot(body)?,
        ))
    } else if id == play_ids::serverbound::MOVE_PLAYER_ROT {
        Ok(Serverbound::PlayMovePlayerRot(decode_move_player_rot(
            body,
        )?))
    } else if id == play_ids::serverbound::MOVE_PLAYER_STATUS_ONLY {
        Ok(Serverbound::PlayMovePlayerStatus(
            decode_move_player_status(body)?,
        ))
    } else if id == play_ids::serverbound::PLAYER_ABILITIES {
        Ok(Serverbound::PlayPlayerAbilities {
            flying: decode_player_abilities(body)?,
        })
    } else if id == play_ids::serverbound::PLAYER_ACTION {
        Ok(Serverbound::PlayPlayerAction(decode_player_action(body)?))
    } else if id == play_ids::serverbound::PLAYER_COMMAND {
        Ok(Serverbound::PlayPlayerCommand(decode_player_command(body)?))
    } else if id == play_ids::serverbound::PLAYER_INPUT {
        Ok(Serverbound::PlayPlayerInput(decode_player_input(body)?))
    } else if id == play_ids::serverbound::USE_ITEM_ON {
        Ok(Serverbound::PlayUseItemOn(decode_use_item_on(body)?))
    } else if id == play_ids::serverbound::SET_CARRIED_ITEM {
        Ok(Serverbound::PlaySetCarriedItem(decode_set_carried_item(
            body,
        )?))
    } else if id == play_ids::serverbound::SET_CREATIVE_MODE_SLOT {
        Ok(Serverbound::PlaySetCreativeModeSlot(
            decode_set_creative_mode_slot(body)?,
        ))
    } else if id == play_ids::serverbound::PICK_ITEM_FROM_BLOCK {
        Ok(Serverbound::PlayPickItemFromBlock(
            decode_pick_item_from_block(body)?,
        ))
    } else {
        Ok(Serverbound::PlayIgnored { id })
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
    fn decode_login_start_accepts_name_over_sixteen() {
        let long = "a".repeat(17);
        let mut body = Vec::new();
        write_var_int(0x00, &mut body);
        write_string(&long, &mut body);
        body.extend_from_slice(&[0u8; 16]);
        match decode_serverbound(State::Login, &body).unwrap() {
            Serverbound::LoginStart { name, .. } => assert_eq!(name, long),
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
    fn decode_login_acknowledged() {
        let mut body = Vec::new();
        write_var_int(0x03, &mut body);
        assert_eq!(
            decode_serverbound(State::Login, &body).unwrap(),
            Serverbound::LoginAcknowledged
        );
    }

    #[test]
    fn login_acknowledged_with_trailing_rejected() {
        let mut body = Vec::new();
        write_var_int(0x03, &mut body);
        body.push(0xFF);
        assert_eq!(
            decode_serverbound(State::Login, &body),
            Err(ProtocolError::TrailingBytes)
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

    #[test]
    fn decode_play_accept_teleportation() {
        let mut body = Vec::new();
        write_var_int(0x00, &mut body);
        write_var_int(5, &mut body);
        for value in [0.5f64, 100.0, 0.5] {
            body.extend_from_slice(&value.to_be_bytes());
        }
        body.extend_from_slice(&0.0f32.to_be_bytes());
        body.extend_from_slice(&0.0f32.to_be_bytes());
        match decode_serverbound(State::Play, &body).unwrap() {
            Serverbound::PlayAcceptTeleportation(accept) => {
                assert_eq!(accept.teleport_id, 5);
                assert_eq!((accept.x, accept.y, accept.z), (0.5, 100.0, 0.5));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn decode_play_player_loaded() {
        let mut body = Vec::new();
        write_var_int(0x2C, &mut body);
        assert_eq!(
            decode_serverbound(State::Play, &body).unwrap(),
            Serverbound::PlayerLoaded
        );
    }

    #[test]
    fn decode_play_chunk_batch_received() {
        let mut body = Vec::new();
        write_var_int(0x0B, &mut body);
        body.extend_from_slice(&8.0f32.to_be_bytes());
        match decode_serverbound(State::Play, &body).unwrap() {
            Serverbound::PlayChunkBatchReceived(received) => {
                assert_eq!(received.desired_chunks_per_tick, 8.0);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn decode_play_keep_alive() {
        let mut body = Vec::new();
        write_var_int(0x1C, &mut body);
        write_i64(42, &mut body);
        assert_eq!(
            decode_serverbound(State::Play, &body).unwrap(),
            Serverbound::PlayKeepAlive(KeepAlive { id: 42 })
        );
    }

    #[test]
    fn decode_play_keep_alive_trailing_is_error() {
        let mut body = Vec::new();
        write_var_int(0x1C, &mut body);
        write_i64(42, &mut body);
        body.push(0x00);
        assert_eq!(
            decode_serverbound(State::Play, &body),
            Err(ProtocolError::TrailingBytes)
        );
    }

    #[test]
    fn decode_play_move_player_pos() {
        let mut body = Vec::new();
        write_var_int(0x1E, &mut body);
        for value in [48.5f64, -60.0, 0.5] {
            body.extend_from_slice(&value.to_be_bytes());
        }
        body.push(0x01);
        match decode_serverbound(State::Play, &body).unwrap() {
            Serverbound::PlayMovePlayerPos(pos) => {
                assert_eq!((pos.x, pos.y, pos.z), (48.5, -60.0, 0.5));
                assert_eq!(pos.flags, 0x01);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn decode_play_move_player_status_only() {
        let mut body = Vec::new();
        write_var_int(0x21, &mut body);
        body.push(0x00);
        assert_eq!(
            decode_serverbound(State::Play, &body).unwrap(),
            Serverbound::PlayMovePlayerStatus(crate::play::MovePlayerStatus { flags: 0x00 })
        );
    }

    #[test]
    fn decode_play_player_abilities() {
        let mut body = Vec::new();
        write_var_int(0x28, &mut body);
        body.push(0x02);
        assert_eq!(
            decode_serverbound(State::Play, &body).unwrap(),
            Serverbound::PlayPlayerAbilities { flying: true }
        );

        let mut grounded = Vec::new();
        write_var_int(0x28, &mut grounded);
        grounded.push(0x00);
        assert_eq!(
            decode_serverbound(State::Play, &grounded).unwrap(),
            Serverbound::PlayPlayerAbilities { flying: false }
        );
    }

    #[test]
    fn decode_play_pick_item_from_block() {
        use crate::play::BlockPos;
        let mut body = Vec::new();
        write_var_int(0x24, &mut body);
        write_i64(BlockPos { x: 2, y: -60, z: 7 }.to_long(), &mut body);
        body.push(0);
        assert_eq!(
            decode_serverbound(State::Play, &body).unwrap(),
            Serverbound::PlayPickItemFromBlock(crate::play::PickItemFromBlock {
                pos: BlockPos { x: 2, y: -60, z: 7 },
                include_data: false,
            })
        );
    }

    #[test]
    fn decode_play_player_input() {
        let mut body = Vec::new();
        write_var_int(0x2B, &mut body);
        body.push(0x20);
        assert_eq!(
            decode_serverbound(State::Play, &body).unwrap(),
            Serverbound::PlayPlayerInput(crate::play::PlayerInput { flags: 0x20 })
        );
    }

    #[test]
    fn decode_play_player_command() {
        let mut body = Vec::new();
        write_var_int(0x2A, &mut body);
        write_var_int(1, &mut body);
        write_var_int(0, &mut body);
        write_var_int(0, &mut body);
        assert_eq!(
            decode_serverbound(State::Play, &body).unwrap(),
            Serverbound::PlayPlayerCommand(crate::play::PlayerCommand {
                entity_id: 1,
                action: 0,
                data: 0,
            })
        );
    }

    #[test]
    fn decode_play_unknown_id_is_ignored() {
        let mut body = Vec::new();
        write_var_int(0x7F, &mut body);
        body.extend_from_slice(&[0xDE, 0xAD]);
        assert_eq!(
            decode_serverbound(State::Play, &body).unwrap(),
            Serverbound::PlayIgnored { id: 0x7F }
        );
    }

    #[test]
    fn decode_play_malformed_known_packet_is_error() {
        let mut body = Vec::new();
        write_var_int(0x2C, &mut body);
        body.push(0xFF);
        assert_eq!(
            decode_serverbound(State::Play, &body),
            Err(ProtocolError::TrailingBytes)
        );
    }

    #[test]
    fn decode_configuration_unknown_id_is_error() {
        let mut body = Vec::new();
        write_var_int(0x7E, &mut body);
        assert_eq!(
            decode_serverbound(State::Configuration, &body),
            Err(ProtocolError::UnknownPacketId {
                state: State::Configuration,
                id: 0x7E
            })
        );
    }
}
