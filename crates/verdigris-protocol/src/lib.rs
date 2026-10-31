pub mod blocks;
pub mod bucket_tables;
pub mod chunk;
pub mod chunkfile;
pub mod clientbound;
pub mod commands;
pub mod config;
pub mod entitydata;
pub mod error;
pub mod framing;
pub mod gamemode;
pub mod interaction_tables;
pub mod light;
pub mod login;
pub mod mth;
pub mod network;
pub mod packets;
pub mod placement_tables;
pub mod play;
pub mod playerlist;
pub mod portal_tables;
pub mod primitives;
pub mod status;
pub mod text;
pub mod tracking;
pub mod varint;
pub mod view;
pub mod world;

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

pub use blocks::{
    Block, BlocksData, ItemState, Property, block_has_property, block_id_of_name,
    block_item_of_state, blocks_data, default_state_of_name, is_air, is_fluid, item_id_of_name,
    item_to_placed_state, property_of, state_to_block, state_with, with_property,
};
pub use bucket_tables::{
    BucketIds, bucket_ids, is_liquid_block, is_source_fluid, is_waterloggable, lava_source_state,
};
pub use chunk::{
    BIOMES_PER_SECTION, BLOCKS_PER_SECTION, COLUMNS_PER_CHUNK, ChunkColumn, HEIGHTMAP_BITS, MIN_Y,
    PaletteThresholds, Section, WORLD_HEIGHT, biome_global_bits, biome_index, biome_thresholds,
    bits_for, block_index, block_thresholds, pack_longs, unpack_longs, world_surface_heights,
    write_chunk_data, write_chunk_payload, write_heightmaps, write_paletted_container,
    write_section,
};
pub use chunkfile::{ChunkFileError, decode_chunk, encode_chunk};
pub use clientbound::{
    COMPRESSION_THRESHOLD, ENTITY_POSITION_SYNC_ID, EncodeError, MAX_NBT_STRING_BYTES,
    MAX_OUTBOUND_PACKET_LEN, MOVE_ENTITY_POS_ID, MOVE_ENTITY_POS_ROT_ID, MOVE_ENTITY_ROT_ID,
    NBT_TAG_STRING, PLAYER_ENTITY_TYPE, ROTATE_HEAD_ID, SET_COMPRESSION_ID, add_entity_payload,
    build_compressed_packet, build_packet, bundle_delimiter_payload, config_disconnect_payload,
    entity_position_sync, frame_entity_pass, login_disconnect_payload, move_entity_pos,
    move_entity_pos_rot, move_entity_rot, remove_entities_payload, rotate_head,
    set_compression_payload, set_entity_data_health_payload, text_component_nbt,
};
pub use commands::{
    ASK_SERVER, ArgParseError, CHAT_TYPE_MSG_INCOMING_HOLDER, CHAT_TYPE_MSG_OUTGOING_HOLDER,
    CHAT_TYPE_SAY_HOLDER, ENTITY_EVENT_PERMISSION_BASE, MAX_PERMISSION_LEVEL,
    MAX_SUGGESTION_LEN_DEFAULT, Parser, argument_error_component,
    command_suggestions_empty_payload, command_usage, command_usages, command_visible,
    commands_payload, context_component, decode_command_suggestion,
    entity_event_permission_payload, parse_gamemode, parse_integer, parse_resource_location,
    parse_time, parse_vec3, root_command_names, unknown_command_components,
};
pub use config::{
    MAX_KNOWN_PACKS, client_has_core, decode_known_packs, known_packs_payload, packet_ids,
    registry_data_payload, server_known_pack, update_tags_payload,
};
pub use entitydata::{
    EntityDataValue, HumanoidArm, Pose, append_entity_data_entries, set_entity_data_payload,
    write_set_entity_data,
};
pub use error::ProtocolError;
pub use framing::{
    MAX_UNCOMPRESSED_LEN, decode_compressed_frame, decode_frame, encode_compressed_frame,
    encode_frame,
};
pub use gamemode::GameMode;
pub use interaction_tables::{InteractionKind, ender_eye_item, interaction_kind};
pub use light::{
    ChunkLight, EMPTY_BLOCK_MASK, compute_chunk_light, full_light_section, write_bitset,
    write_light, write_light_section,
};
pub use login::{
    InvalidName, login_success_payload, offline_uuid, random_session_id, validate_name,
};
pub use mth::{
    Angle, Direction, cos, horizontal_direction, nearest_looking_direction,
    nearest_looking_directions, sin, wrap_degrees,
};
pub use network::{KnownPack, NetworkData, Registry, Source, TagEntry, TagRegistry, network_data};
pub use packets::{Handshake, Intent, Serverbound, decode_serverbound};
pub use placement_tables::{
    HorizFacingKind, PlacementKind, SixFacingKind, SlabType, full_block, horiz_facing_kind,
    is_slab_block, is_stair_block, placement_kind_of_block, placement_kind_of_item,
    six_facing_kind, slab_block_of_item, slab_type, state_in_replaceable, water_source_state,
};
pub use play::{
    ABORT_DESTROY_BLOCK, AIR_STATE, AcceptTeleportation, BlockPos, CHANGE_DESTROY_DIRECTION,
    CHAT_TYPE_CHAT_HOLDER, Chat, ChatCommandSigned, ChatError, ChunkBatchReceived,
    ClientInformation, GAME_EVENT_WAIT_FOR_CHUNKS, ItemStack, KeepAlive, MAIN_HAND_LEFT,
    MAIN_HAND_RIGHT, MAX_CHAT_COMMAND_LEN, MAX_CHAT_MESSAGE_LEN, MovePlayerPos, MovePlayerPosRot,
    MovePlayerRot, MovePlayerStatus, PickItemFromBlock, PlayError, PlayerAction, PlayerCommand,
    PlayerInput, START_DESTROY_BLOCK, STOP_DESTROY_BLOCK, SWING_DURATION, SWING_WHACK,
    SetCarriedItem, SetCreativeModeSlot, TickThrottler, Update, UseItem, UseItemOn,
    block_changed_ack_payload, block_update_payload, chunk_batch_finished_payload,
    chunk_batch_start_payload, container_set_slot_payload, decode_accept_teleportation,
    decode_bounded_string, decode_chat, decode_chat_ack, decode_chat_command,
    decode_chat_command_signed, decode_chat_session_update, decode_chunk_batch_received,
    decode_client_information, decode_keep_alive, decode_move_player_pos,
    decode_move_player_pos_rot, decode_move_player_rot, decode_move_player_status,
    decode_pick_item_from_block, decode_player_abilities, decode_player_action,
    decode_player_command, decode_player_input, decode_player_loaded, decode_punch,
    decode_set_carried_item, decode_set_creative_mode_slot, decode_update, decode_use_item,
    decode_use_item_on, ensure_play_registries, forget_level_chunk_payload, game_event_payload,
    keep_alive_payload, level_chunk_with_light_payload, login_play_payload, normalize_chat,
    overworld_dimension_type_id, plains_biome_id, player_abilities_payload, player_chat_tail,
    player_position_payload, set_chunk_cache_center_payload, set_equipment_payload,
    set_held_slot_payload, swing_animation_payload, validate_chat_content,
    write_item_stack_no_components,
};
pub use playerlist::{
    ListedPlayer, player_info_remove_payload, player_info_update_gamemode_payload,
    player_info_update_init_payload, player_info_update_latency_payload, tab_list_payload,
};
pub use portal_tables::{
    INTERIOR_OFFSETS, INTERIOR_SIZE, RING_SIZE, RING_TEMPLATE, RingSlot, end_portal_state,
    frame_block_id, is_filled_frame, is_frame_facing,
};
pub use primitives::{
    fits_i16, pack_degrees, quantize, read_f32, read_f64, read_i16, read_i32, read_i64,
    read_string, read_u16, write_f32, write_f64, write_i16, write_i32, write_i64, write_string,
    write_u16,
};
pub use status::{
    StatusInfo, encode_pong, encode_status_response, pong_payload, status_json,
    status_response_payload,
};
pub use text::{
    Arg, ClickEvent, Component, Content, HoverEvent, SYSTEM_CHAT, Style, player_display_name,
    root_tag_type, system_chat_payload, write_nbt,
};
pub use tracking::{
    EntitySnapshot, FORCE_SYNC_TICKS, MAX_TELEPORT_DELAY, POSITION_EPSILON_SQ, TrackedState,
};
pub use varint::{read_var_int, read_var_long, write_var_int, write_var_long};
pub use view::{
    KEEP_RADIUS, LOAD_RADIUS, chebyshev, chunk_coord, chunks_to_forget, chunks_to_load,
};
pub use world::{ChunkGenerator, ChunkLoader, FlatGenerator, WORLD_MAX_Y, WORLD_MIN_Y, World};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cible_26_3() {
        assert_eq!((GAME_VERSION, PROTOCOL_VERSION), ("26.3", 777));
    }
}
