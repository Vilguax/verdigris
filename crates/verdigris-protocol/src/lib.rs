pub mod clientbound;
pub mod config;
pub mod error;
pub mod framing;
pub mod login;
pub mod network;
pub mod packets;
pub mod play;
pub mod primitives;
pub mod status;
pub mod varint;

pub const GAME_VERSION: &str = "26.3";
pub const PROTOCOL_VERSION: i32 = 777;

pub const MAX_PACKET_LEN: usize = 2 * 1024 * 1024 - 1;
pub const MAX_STRING_LEN: usize = 32767;
pub const MAX_ADDRESS_LEN: usize = 255;
pub const MAX_STATUS_JSON_LEN: usize = 32767;
pub const MAX_USERNAME_LEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Handshaking,
    Status,
    Login,
    Configuration,
    Play,
}

pub use clientbound::{
    COMPRESSION_THRESHOLD, EncodeError, MAX_NBT_STRING_BYTES, MAX_OUTBOUND_PACKET_LEN,
    NBT_TAG_STRING, SET_COMPRESSION_ID, build_compressed_packet, build_packet,
    config_disconnect_payload, login_disconnect_payload, set_compression_payload,
};
pub use config::{
    MAX_KNOWN_PACKS, client_has_core, decode_known_packs, known_packs_payload, packet_ids,
    registry_data_payload, server_known_pack, update_tags_payload,
};
pub use error::ProtocolError;
pub use framing::{
    MAX_UNCOMPRESSED_LEN, decode_compressed_frame, decode_frame, encode_compressed_frame,
    encode_frame,
};
pub use login::{
    InvalidName, login_success_payload, offline_uuid, random_session_id, validate_name,
};
pub use network::{KnownPack, NetworkData, Registry, Source, TagEntry, TagRegistry, network_data};
pub use packets::{Handshake, Intent, Serverbound, decode_serverbound};
pub use play::{
    AcceptTeleportation, ChunkBatchReceived, GAME_EVENT_WAIT_FOR_CHUNKS, PlayError,
    chunk_batch_finished_payload, chunk_batch_start_payload, decode_accept_teleportation,
    decode_chunk_batch_received, decode_player_loaded, ensure_play_registries, game_event_payload,
    level_chunk_with_light_payload, login_play_payload, overworld_dimension_type_id,
    plains_biome_id, player_position_payload, set_chunk_cache_center_payload,
};
pub use primitives::{
    read_f32, read_f64, read_i32, read_i64, read_string, read_u16, write_f32, write_f64, write_i32,
    write_i64, write_string, write_u16,
};
pub use status::{
    StatusInfo, encode_pong, encode_status_response, pong_payload, status_json,
    status_response_payload,
};
pub use varint::{read_var_int, read_var_long, write_var_int, write_var_long};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cible_26_3() {
        assert_eq!((GAME_VERSION, PROTOCOL_VERSION), ("26.3", 777));
    }
}
