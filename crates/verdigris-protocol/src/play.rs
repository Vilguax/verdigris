use crate::network::network_data;
use crate::primitives::{write_i32, write_i64, write_string, write_u16};
use crate::varint::write_var_int;

pub const SECTION_COUNT: usize = 24;
pub const EMPTY_CHUNK_DATA_LEN: usize = SECTION_COUNT * 8;
pub const MAX_CHUNK_DATA_LEN: usize = 2 * 1024 * 1024;
pub const DEFAULT_SEA_LEVEL: i32 = 63;
pub const PREVIOUS_GAME_TYPE_NONE: i32 = -1;

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

fn write_single_valued_container(value: i32, out: &mut Vec<u8>) {
    out.push(0);
    write_var_int(value, out);
}

fn write_empty_section(biome_id: i32, out: &mut Vec<u8>) {
    write_u16(0, out);
    write_u16(0, out);
    write_single_valued_container(0, out);
    write_single_valued_container(biome_id, out);
}

fn build_empty_chunk_data(biome_id: i32) -> Vec<u8> {
    let mut data = Vec::with_capacity(EMPTY_CHUNK_DATA_LEN);
    for _ in 0..SECTION_COUNT {
        write_empty_section(biome_id, &mut data);
    }
    data
}

pub fn level_chunk_with_light_payload(x: i32, z: i32) -> Result<Vec<u8>, PlayError> {
    let biome_id = plains_biome_id()?;

    let mut payload = Vec::new();
    write_i32(x, &mut payload);
    write_i32(z, &mut payload);

    write_var_int(0, &mut payload);

    let data = build_empty_chunk_data(biome_id);
    debug_assert!(data.len() <= MAX_CHUNK_DATA_LEN);
    write_var_int(data.len() as i32, &mut payload);
    payload.extend_from_slice(&data);

    write_var_int(0, &mut payload);

    for _ in 0..4 {
        write_var_int(0, &mut payload);
    }
    write_var_int(0, &mut payload);
    write_var_int(0, &mut payload);

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
}
