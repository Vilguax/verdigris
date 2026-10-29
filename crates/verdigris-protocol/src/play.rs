use crate::chunk::{ChunkColumn, write_chunk_payload};
use crate::error::ProtocolError;
use crate::network::network_data;
use crate::primitives::{
    read_f32, read_f64, read_i64, write_f32, write_f64, write_i32, write_i64, write_string,
};
use crate::varint::{read_var_int, write_var_int};

pub const SECTION_COUNT: usize = 24;
pub const EMPTY_CHUNK_DATA_LEN: usize = SECTION_COUNT * 8;
pub const MAX_CHUNK_DATA_LEN: usize = 2 * 1024 * 1024;
pub const DEFAULT_SEA_LEVEL: i32 = 63;
pub const PREVIOUS_GAME_TYPE_NONE: i32 = -1;
pub const GAME_EVENT_WAIT_FOR_CHUNKS: u8 = 13;

pub const BIOME_REGISTRY: &str = "minecraft:worldgen/biome";
pub const DIMENSION_TYPE_REGISTRY: &str = "minecraft:dimension_type";
pub const PLAINS: &str = "minecraft:plains";

pub mod packet_ids {
    pub mod clientbound {
        pub const LOGIN: i32 = 0x32;
        pub const PLAYER_POSITION: i32 = 0x49;
        pub const SET_CHUNK_CACHE_CENTER: i32 = 0x60;
        pub const LEVEL_CHUNK_WITH_LIGHT: i32 = 0x2E;
        pub const GAME_EVENT: i32 = 0x27;
        pub const KEEP_ALIVE: i32 = 0x2D;
        pub const CHUNK_BATCH_START: i32 = 0x0C;
        pub const CHUNK_BATCH_FINISHED: i32 = 0x0B;
    }

    pub mod serverbound {
        pub const ACCEPT_TELEPORTATION: i32 = 0x00;
        pub const KEEP_ALIVE: i32 = 0x1C;
        pub const PLAYER_LOADED: i32 = 0x2C;
        pub const CHUNK_BATCH_RECEIVED: i32 = 0x0B;
    }
}

const OVERWORLD: &str = "minecraft:overworld";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayError {
    MissingRegistry {
        registry: &'static str,
    },
    MissingEntry {
        registry: &'static str,
        entry: &'static str,
    },
}

impl core::fmt::Display for PlayError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PlayError::MissingRegistry { registry } => {
                write!(
                    f,
                    "registry {registry} absent from synchronised network data"
                )
            }
            PlayError::MissingEntry { registry, entry } => {
                write!(f, "entry {entry} absent from registry {registry}")
            }
        }
    }
}

impl std::error::Error for PlayError {}

fn registry_entry_id(registry: &'static str, entry: &'static str) -> Result<i32, PlayError> {
    network_data()
        .registry(registry)
        .ok_or(PlayError::MissingRegistry { registry })?
        .entry_id(entry)
        .ok_or(PlayError::MissingEntry { registry, entry })
}

pub fn plains_biome_id() -> Result<i32, PlayError> {
    registry_entry_id(BIOME_REGISTRY, PLAINS)
}

pub fn overworld_dimension_type_id() -> Result<i32, PlayError> {
    registry_entry_id(DIMENSION_TYPE_REGISTRY, OVERWORLD)
}

pub fn ensure_play_registries() -> Result<(), PlayError> {
    plains_biome_id()?;
    overworld_dimension_type_id()?;
    Ok(())
}

fn write_bool(value: bool, out: &mut Vec<u8>) {
    out.push(u8::from(value));
}

#[cfg(test)]
fn write_empty_section(biome_id: i32, out: &mut Vec<u8>) {
    crate::chunk::write_section(&crate::chunk::Section::air(biome_id as u16), out);
}

#[cfg(test)]
fn build_empty_chunk_data(biome_id: i32) -> Vec<u8> {
    let mut data = Vec::with_capacity(EMPTY_CHUNK_DATA_LEN);
    crate::chunk::write_chunk_data(&ChunkColumn::empty(biome_id as u16), &mut data);
    data
}

pub fn level_chunk_with_light_payload(x: i32, z: i32) -> Result<Vec<u8>, PlayError> {
    let biome_id = plains_biome_id()?;

    let column = ChunkColumn::empty(biome_id as u16);
    let mut payload = Vec::new();
    write_chunk_payload(x, z, &column, &mut payload);
    debug_assert!(payload.len() <= MAX_CHUNK_DATA_LEN);
    Ok(payload)
}

fn write_common_player_spawn_info(game_type: i32, dimension_type_id: i32, out: &mut Vec<u8>) {
    write_var_int(dimension_type_id, out);
    write_string(OVERWORLD, out);
    write_i64(0, out);
    write_var_int(game_type, out);
    write_var_int(PREVIOUS_GAME_TYPE_NONE, out);
    write_bool(false, out);
    write_bool(false, out);
    write_bool(false, out);
    write_var_int(0, out);
    write_var_int(DEFAULT_SEA_LEVEL, out);
}

pub fn login_play_payload(
    player_id: i32,
    chunk_radius: i32,
    simulation_distance: i32,
    game_type: i32,
    max_players: i32,
) -> Result<Vec<u8>, PlayError> {
    let dimension_type_id = overworld_dimension_type_id()?;

    let mut payload = Vec::new();
    write_i32(player_id, &mut payload);
    write_bool(false, &mut payload);

    write_var_int(1, &mut payload);
    write_string(OVERWORLD, &mut payload);

    write_var_int(max_players, &mut payload);
    write_var_int(chunk_radius, &mut payload);
    write_var_int(simulation_distance, &mut payload);
    write_bool(false, &mut payload);
    write_bool(true, &mut payload);
    write_bool(false, &mut payload);

    write_common_player_spawn_info(game_type, dimension_type_id, &mut payload);

    write_bool(false, &mut payload);
    write_bool(false, &mut payload);

    Ok(payload)
}

pub fn player_position_payload(
    teleport_id: i32,
    pos: [f64; 3],
    delta: [f64; 3],
    yaw: f32,
    pitch: f32,
    relatives: i32,
) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(teleport_id, &mut payload);
    for value in pos {
        write_f64(value, &mut payload);
    }
    for value in delta {
        write_f64(value, &mut payload);
    }
    write_f32(yaw, &mut payload);
    write_f32(pitch, &mut payload);
    write_i32(relatives, &mut payload);
    payload
}

pub fn game_event_payload(event: u8, param: f32) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(event);
    write_f32(param, &mut payload);
    payload
}

pub fn set_chunk_cache_center_payload(x: i32, z: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(x, &mut payload);
    write_var_int(z, &mut payload);
    payload
}

pub fn chunk_batch_start_payload() -> Vec<u8> {
    Vec::new()
}

pub fn chunk_batch_finished_payload(batch_size: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(batch_size, &mut payload);
    payload
}

pub fn keep_alive_payload(id: i64) -> Vec<u8> {
    let mut payload = Vec::new();
    write_i64(id, &mut payload);
    payload
}

#[derive(Debug, Clone, PartialEq)]
pub struct AcceptTeleportation {
    pub teleport_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChunkBatchReceived {
    pub desired_chunks_per_tick: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeepAlive {
    pub id: i64,
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

fn take_f32(cursor: &mut &[u8]) -> Result<f32, ProtocolError> {
    match read_f32(cursor)? {
        Some((value, consumed)) => {
            *cursor = &cursor[consumed..];
            Ok(value)
        }
        None => Err(ProtocolError::UnexpectedEof),
    }
}

fn take_f64(cursor: &mut &[u8]) -> Result<f64, ProtocolError> {
    match read_f64(cursor)? {
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

pub fn decode_accept_teleportation(body: &[u8]) -> Result<AcceptTeleportation, ProtocolError> {
    let mut cursor = body;
    let teleport_id = take_var_int(&mut cursor)?;
    let x = take_f64(&mut cursor)?;
    let y = take_f64(&mut cursor)?;
    let z = take_f64(&mut cursor)?;
    let yaw = take_f32(&mut cursor)?;
    let pitch = take_f32(&mut cursor)?;
    if !cursor.is_empty() {
        return Err(ProtocolError::TrailingBytes);
    }
    Ok(AcceptTeleportation {
        teleport_id,
        x,
        y,
        z,
        yaw,
        pitch,
    })
}

pub fn decode_player_loaded(body: &[u8]) -> Result<(), ProtocolError> {
    if !body.is_empty() {
        return Err(ProtocolError::TrailingBytes);
    }
    Ok(())
}

pub fn decode_chunk_batch_received(body: &[u8]) -> Result<ChunkBatchReceived, ProtocolError> {
    let mut cursor = body;
    let desired_chunks_per_tick = take_f32(&mut cursor)?;
    if !cursor.is_empty() {
        return Err(ProtocolError::TrailingBytes);
    }
    Ok(ChunkBatchReceived {
        desired_chunks_per_tick,
    })
}

pub fn decode_keep_alive(body: &[u8]) -> Result<KeepAlive, ProtocolError> {
    let mut cursor = body;
    let id = take_i64(&mut cursor)?;
    if !cursor.is_empty() {
        return Err(ProtocolError::TrailingBytes);
    }
    Ok(KeepAlive { id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::varint::read_var_int;

    #[test]
    fn packet_ids_match_report() {
        assert_eq!(packet_ids::clientbound::LOGIN, 0x32);
        assert_eq!(packet_ids::clientbound::PLAYER_POSITION, 0x49);
        assert_eq!(packet_ids::clientbound::SET_CHUNK_CACHE_CENTER, 0x60);
        assert_eq!(packet_ids::clientbound::LEVEL_CHUNK_WITH_LIGHT, 0x2E);
        assert_eq!(packet_ids::clientbound::GAME_EVENT, 0x27);
        assert_eq!(packet_ids::clientbound::KEEP_ALIVE, 0x2D);
        assert_eq!(packet_ids::clientbound::CHUNK_BATCH_START, 0x0C);
        assert_eq!(packet_ids::clientbound::CHUNK_BATCH_FINISHED, 0x0B);
        assert_eq!(packet_ids::serverbound::ACCEPT_TELEPORTATION, 0x00);
        assert_eq!(packet_ids::serverbound::KEEP_ALIVE, 0x1C);
        assert_eq!(packet_ids::serverbound::PLAYER_LOADED, 0x2C);
        assert_eq!(packet_ids::serverbound::CHUNK_BATCH_RECEIVED, 0x0B);
    }

    #[test]
    fn plains_id_from_network_json() {
        assert_eq!(plains_biome_id().unwrap(), 41);
    }

    #[test]
    fn overworld_dimension_type_from_network_json() {
        assert_eq!(overworld_dimension_type_id().unwrap(), 0);
    }

    #[test]
    fn embedded_play_registries_present() {
        assert_eq!(ensure_play_registries(), Ok(()));
    }

    #[test]
    fn missing_entry_is_error_not_panic() {
        assert_eq!(
            registry_entry_id(BIOME_REGISTRY, "minecraft:not_a_biome"),
            Err(PlayError::MissingEntry {
                registry: BIOME_REGISTRY,
                entry: "minecraft:not_a_biome",
            })
        );
    }

    #[test]
    fn missing_registry_is_error_not_panic() {
        assert_eq!(
            registry_entry_id("minecraft:does_not_exist", "x"),
            Err(PlayError::MissingRegistry {
                registry: "minecraft:does_not_exist",
            })
        );
    }

    #[test]
    fn empty_section_exact_bytes() {
        let mut section = Vec::new();
        write_empty_section(plains_biome_id().unwrap(), &mut section);
        assert_eq!(
            section,
            vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x29]
        );
    }

    #[test]
    fn chunk_data_has_24_sections_of_192_bytes() {
        let data = build_empty_chunk_data(plains_biome_id().unwrap());
        assert_eq!(data.len(), 192);
        assert_eq!(data.len(), SECTION_COUNT * 8);
    }

    #[test]
    fn chunk_header_coordinates_are_big_endian() {
        let payload = level_chunk_with_light_payload(1, -1).unwrap();
        assert_eq!(&payload[..4], &[0x00, 0x00, 0x00, 0x01]);
        assert_eq!(&payload[4..8], &[0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn chunk_header_coordinates_at_bounds() {
        let min = level_chunk_with_light_payload(i32::MIN, i32::MIN).unwrap();
        assert_eq!(&min[..4], &[0x80, 0x00, 0x00, 0x00]);
        assert_eq!(&min[4..8], &[0x80, 0x00, 0x00, 0x00]);

        let max = level_chunk_with_light_payload(i32::MAX, i32::MAX).unwrap();
        assert_eq!(&max[..4], &[0x7F, 0xFF, 0xFF, 0xFF]);
        assert_eq!(&max[4..8], &[0x7F, 0xFF, 0xFF, 0xFF]);

        let zero = level_chunk_with_light_payload(0, 0).unwrap();
        assert_eq!(&zero[..8], &[0x00; 8]);
    }

    #[test]
    fn chunk_data_prefix_and_trailers() {
        let payload = level_chunk_with_light_payload(0, 0).unwrap();
        let mut cursor = &payload[8..];
        let (heightmaps, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(heightmaps, 0);
        cursor = &cursor[c..];

        let (data_len, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(data_len, 192);
        cursor = &cursor[c..];
        cursor = &cursor[data_len as usize..];

        let (block_entities, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(block_entities, 0);
        cursor = &cursor[c..];

        for _ in 0..6 {
            let (value, c) = read_var_int(cursor).unwrap().unwrap();
            assert_eq!(value, 0);
            cursor = &cursor[c..];
        }
        assert!(cursor.is_empty());
    }

    #[test]
    fn login_play_field_order() {
        let payload = login_play_payload(7, 10, 8, 1, 20).unwrap();
        let mut cursor = &payload[..];

        assert_eq!(&cursor[..4], &[0x00, 0x00, 0x00, 0x07]);
        cursor = &cursor[4..];

        assert_eq!(cursor[0], 0x00);
        cursor = &cursor[1..];

        let (levels, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(levels, 1);
        cursor = &cursor[c..];

        let len = cursor[0] as usize;
        cursor = &cursor[1..];
        assert_eq!(&cursor[..len], OVERWORLD.as_bytes());
        cursor = &cursor[len..];

        let (max_players, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(max_players, 20);
        cursor = &cursor[c..];

        let (chunk_radius, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(chunk_radius, 10);
        cursor = &cursor[c..];

        let (simulation_distance, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(simulation_distance, 8);
        cursor = &cursor[c..];

        assert_eq!(cursor[0], 0x00);
        assert_eq!(cursor[1], 0x01);
        assert_eq!(cursor[2], 0x00);
        cursor = &cursor[3..];

        let (dimension_type, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(dimension_type, 0);
        cursor = &cursor[c..];

        let len = cursor[0] as usize;
        cursor = &cursor[1..];
        assert_eq!(&cursor[..len], OVERWORLD.as_bytes());
        cursor = &cursor[len..];

        assert_eq!(&cursor[..8], &[0x00; 8]);
        cursor = &cursor[8..];

        let (game_type, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(game_type, 1);
        cursor = &cursor[c..];

        let (previous_game_type, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(previous_game_type, -1);
        cursor = &cursor[c..];

        assert_eq!(cursor[0], 0x00);
        assert_eq!(cursor[1], 0x00);
        assert_eq!(cursor[2], 0x00);
        cursor = &cursor[3..];

        let (portal_cooldown, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(portal_cooldown, 0);
        cursor = &cursor[c..];

        let (sea_level, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(sea_level, DEFAULT_SEA_LEVEL);
        cursor = &cursor[c..];

        assert_eq!(cursor, &[0x00, 0x00]);
    }

    #[test]
    fn login_play_handles_negative_and_bound_values() {
        let payload = login_play_payload(i32::MIN, 0, 0, -1, i32::MAX).unwrap();
        let mut cursor = &payload[..];

        assert_eq!(&cursor[..4], &[0x80, 0x00, 0x00, 0x00]);
        cursor = &cursor[5..];

        let (levels, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(levels, 1);
        cursor = &cursor[c..];

        let len = cursor[0] as usize;
        cursor = &cursor[1 + len..];

        let (max_players, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(max_players, i32::MAX);
        cursor = &cursor[c..];

        let (chunk_radius, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(chunk_radius, 0);
        cursor = &cursor[c..];

        let (simulation_distance, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(simulation_distance, 0);
        cursor = &cursor[c..];

        cursor = &cursor[3..];

        let (dimension_type, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(dimension_type, 0);
        cursor = &cursor[c..];

        let len = cursor[0] as usize;
        cursor = &cursor[1 + len + 8..];

        let (game_type, _) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(game_type, -1);
    }

    #[test]
    fn player_position_layout() {
        let payload = player_position_payload(7, [0.5, 100.0, 0.5], [0.0, 0.0, 0.0], 0.0, 0.0, 0);
        let mut cursor = &payload[..];

        let (teleport_id, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(teleport_id, 7);
        cursor = &cursor[c..];

        for expected in [0.5f64, 100.0, 0.5, 0.0, 0.0, 0.0] {
            assert_eq!(&cursor[..8], &expected.to_be_bytes());
            cursor = &cursor[8..];
        }
        assert_eq!(&cursor[..4], &0.0f32.to_be_bytes());
        cursor = &cursor[4..];
        assert_eq!(&cursor[..4], &0.0f32.to_be_bytes());
        cursor = &cursor[4..];
        assert_eq!(cursor, &[0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn game_event_layout() {
        let payload = game_event_payload(GAME_EVENT_WAIT_FOR_CHUNKS, 0.0);
        assert_eq!(payload, vec![13, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn set_chunk_cache_center_layout() {
        let payload = set_chunk_cache_center_payload(0, 0);
        assert_eq!(payload, vec![0x00, 0x00]);
        let payload = set_chunk_cache_center_payload(-1, 1);
        let mut cursor = &payload[..];
        let (x, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(x, -1);
        cursor = &cursor[c..];
        let (z, _) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(z, 1);
    }

    #[test]
    fn chunk_batch_start_is_empty() {
        assert!(chunk_batch_start_payload().is_empty());
    }

    #[test]
    fn chunk_batch_finished_layout() {
        let payload = chunk_batch_finished_payload(81);
        let (count, consumed) = read_var_int(&payload).unwrap().unwrap();
        assert_eq!(count, 81);
        assert_eq!(consumed, payload.len());
    }

    #[test]
    fn accept_teleportation_round_trip() {
        let mut body = Vec::new();
        write_var_int(42, &mut body);
        for value in [1.0f64, 2.0, 3.0] {
            body.extend_from_slice(&value.to_be_bytes());
        }
        body.extend_from_slice(&90.0f32.to_be_bytes());
        body.extend_from_slice(&(-45.0f32).to_be_bytes());
        assert_eq!(
            decode_accept_teleportation(&body).unwrap(),
            AcceptTeleportation {
                teleport_id: 42,
                x: 1.0,
                y: 2.0,
                z: 3.0,
                yaw: 90.0,
                pitch: -45.0,
            }
        );
    }

    #[test]
    fn accept_teleportation_truncated_is_eof() {
        let mut body = Vec::new();
        write_var_int(42, &mut body);
        body.extend_from_slice(&1.0f64.to_be_bytes());
        assert_eq!(
            decode_accept_teleportation(&body),
            Err(ProtocolError::UnexpectedEof)
        );
    }

    #[test]
    fn accept_teleportation_trailing_is_error() {
        let mut body = Vec::new();
        write_var_int(42, &mut body);
        for value in [1.0f64, 2.0, 3.0] {
            body.extend_from_slice(&value.to_be_bytes());
        }
        body.extend_from_slice(&0.0f32.to_be_bytes());
        body.extend_from_slice(&0.0f32.to_be_bytes());
        body.push(0xFF);
        assert_eq!(
            decode_accept_teleportation(&body),
            Err(ProtocolError::TrailingBytes)
        );
    }

    #[test]
    fn player_loaded_requires_empty_body() {
        assert_eq!(decode_player_loaded(&[]), Ok(()));
        assert_eq!(
            decode_player_loaded(&[0x00]),
            Err(ProtocolError::TrailingBytes)
        );
    }

    #[test]
    fn chunk_batch_received_round_trip() {
        let mut body = Vec::new();
        body.extend_from_slice(&8.0f32.to_be_bytes());
        assert_eq!(
            decode_chunk_batch_received(&body).unwrap(),
            ChunkBatchReceived {
                desired_chunks_per_tick: 8.0
            }
        );
    }

    #[test]
    fn chunk_batch_received_truncated_is_eof() {
        assert_eq!(
            decode_chunk_batch_received(&[0x00, 0x00]),
            Err(ProtocolError::UnexpectedEof)
        );
    }

    #[test]
    fn chunk_batch_received_trailing_is_error() {
        let mut body = Vec::new();
        body.extend_from_slice(&8.0f32.to_be_bytes());
        body.push(0x00);
        assert_eq!(
            decode_chunk_batch_received(&body),
            Err(ProtocolError::TrailingBytes)
        );
    }

    #[test]
    fn keep_alive_payload_is_eight_big_endian_bytes() {
        let payload = keep_alive_payload(0x0102030405060708);
        assert_eq!(payload.len(), 8);
        assert_eq!(
            payload,
            vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
        );
    }

    #[test]
    fn keep_alive_round_trip() {
        for id in [1i64, 0, -1, i64::MAX, i64::MIN, 123456789] {
            let payload = keep_alive_payload(id);
            assert_eq!(decode_keep_alive(&payload).unwrap(), KeepAlive { id });
        }
    }

    #[test]
    fn keep_alive_too_short_is_eof() {
        assert_eq!(
            decode_keep_alive(&[0x00, 0x00, 0x00]),
            Err(ProtocolError::UnexpectedEof)
        );
    }

    #[test]
    fn keep_alive_trailing_is_error() {
        let mut body = keep_alive_payload(7);
        body.push(0xFF);
        assert_eq!(decode_keep_alive(&body), Err(ProtocolError::TrailingBytes));
    }
}
