use crate::chunk::{ChunkColumn, write_chunk_payload};
use crate::error::ProtocolError;
use crate::network::network_data;
use crate::primitives::{
    read_f32, read_f64, read_i64, read_u16, write_f32, write_f64, write_i32, write_i64,
    write_string,
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
        pub const FORGET_LEVEL_CHUNK: i32 = 0x26;
        pub const PLAYER_ABILITIES: i32 = 0x41;
        pub const BLOCK_UPDATE: i32 = 0x08;
        pub const BLOCK_CHANGED_ACK: i32 = 0x04;
        pub const CONTAINER_SET_SLOT: i32 = 0x14;
        pub const SET_HELD_SLOT: i32 = 0x6B;
        pub const PLAYER_INFO_UPDATE: i32 = 0x47;
        pub const PLAYER_INFO_REMOVE: i32 = 0x46;
        pub const TAB_LIST: i32 = 0x7D;
        pub const BUNDLE_DELIMITER: i32 = 0x00;
        pub const ADD_ENTITY: i32 = 0x01;
        pub const REMOVE_ENTITIES: i32 = 0x4E;
        pub const SET_ENTITY_DATA: i32 = 0x65;
    }

    pub mod serverbound {
        pub const ACCEPT_TELEPORTATION: i32 = 0x00;
        pub const KEEP_ALIVE: i32 = 0x1C;
        pub const PLAYER_LOADED: i32 = 0x2C;
        pub const CHUNK_BATCH_RECEIVED: i32 = 0x0B;
        pub const MOVE_PLAYER_POS: i32 = 0x1E;
        pub const MOVE_PLAYER_POS_ROT: i32 = 0x1F;
        pub const MOVE_PLAYER_ROT: i32 = 0x20;
        pub const MOVE_PLAYER_STATUS_ONLY: i32 = 0x21;
        pub const PLAYER_ABILITIES: i32 = 0x28;
        pub const PLAYER_ACTION: i32 = 0x29;
        pub const PLAYER_COMMAND: i32 = 0x2A;
        pub const PLAYER_INPUT: i32 = 0x2B;
        pub const USE_ITEM_ON: i32 = 0x42;
        pub const USE_ITEM: i32 = 0x43;
        pub const SET_CARRIED_ITEM: i32 = 0x36;
        pub const SET_CREATIVE_MODE_SLOT: i32 = 0x39;
        pub const PICK_ITEM_FROM_BLOCK: i32 = 0x24;
    }
}

pub const AIR_STATE: u16 = 0;

pub const START_DESTROY_BLOCK: i32 = 0;
pub const CHANGE_DESTROY_DIRECTION: i32 = 1;
pub const ABORT_DESTROY_BLOCK: i32 = 2;
pub const STOP_DESTROY_BLOCK: i32 = 3;

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

pub fn forget_level_chunk_payload(x: i32, z: i32) -> Vec<u8> {
    let value = ((z as i64 & 0xFFFF_FFFF) << 32) | (x as i64 & 0xFFFF_FFFF);
    let mut payload = Vec::new();
    write_i64(value, &mut payload);
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

pub const DEFAULT_FLYING_SPEED: f32 = 0.05;
pub const DEFAULT_WALKING_SPEED: f32 = 0.1;

pub fn player_abilities_payload(flags: u8) -> Vec<u8> {
    let mut payload = Vec::with_capacity(9);
    payload.push(flags);
    write_f32(DEFAULT_FLYING_SPEED, &mut payload);
    write_f32(DEFAULT_WALKING_SPEED, &mut payload);
    payload
}

pub fn decode_player_abilities(body: &[u8]) -> Result<bool, ProtocolError> {
    let mut cursor = body;
    let flags = take_u8(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok((flags & 0x02) != 0)
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

#[derive(Debug, Clone, PartialEq)]
pub struct MovePlayerPos {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub flags: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MovePlayerPosRot {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub flags: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MovePlayerRot {
    pub yaw: f32,
    pub pitch: f32,
    pub flags: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePlayerStatus {
    pub flags: u8,
}

fn take_u8(cursor: &mut &[u8]) -> Result<u8, ProtocolError> {
    match cursor.split_first() {
        Some((&byte, rest)) => {
            *cursor = rest;
            Ok(byte)
        }
        None => Err(ProtocolError::UnexpectedEof),
    }
}

fn ensure_consumed(cursor: &[u8]) -> Result<(), ProtocolError> {
    if cursor.is_empty() {
        Ok(())
    } else {
        Err(ProtocolError::TrailingBytes)
    }
}

pub fn decode_move_player_pos(body: &[u8]) -> Result<MovePlayerPos, ProtocolError> {
    let mut cursor = body;
    let x = take_f64(&mut cursor)?;
    let y = take_f64(&mut cursor)?;
    let z = take_f64(&mut cursor)?;
    let flags = take_u8(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(MovePlayerPos { x, y, z, flags })
}

pub fn decode_move_player_pos_rot(body: &[u8]) -> Result<MovePlayerPosRot, ProtocolError> {
    let mut cursor = body;
    let x = take_f64(&mut cursor)?;
    let y = take_f64(&mut cursor)?;
    let z = take_f64(&mut cursor)?;
    let yaw = take_f32(&mut cursor)?;
    let pitch = take_f32(&mut cursor)?;
    let flags = take_u8(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(MovePlayerPosRot {
        x,
        y,
        z,
        yaw,
        pitch,
        flags,
    })
}

pub fn decode_move_player_rot(body: &[u8]) -> Result<MovePlayerRot, ProtocolError> {
    let mut cursor = body;
    let yaw = take_f32(&mut cursor)?;
    let pitch = take_f32(&mut cursor)?;
    let flags = take_u8(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(MovePlayerRot { yaw, pitch, flags })
}

pub fn decode_move_player_status(body: &[u8]) -> Result<MovePlayerStatus, ProtocolError> {
    let mut cursor = body;
    let flags = take_u8(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(MovePlayerStatus { flags })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerInput {
    pub flags: u8,
}

impl PlayerInput {
    pub const FORWARD: u8 = 0x01;
    pub const BACKWARD: u8 = 0x02;
    pub const LEFT: u8 = 0x04;
    pub const RIGHT: u8 = 0x08;
    pub const JUMP: u8 = 0x10;
    pub const SNEAK: u8 = 0x20;
    pub const SPRINT: u8 = 0x40;

    pub fn sneaking(&self) -> bool {
        self.flags & PlayerInput::SNEAK != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerCommand {
    pub entity_id: i32,
    pub action: i32,
    pub data: i32,
}

pub fn decode_player_input(body: &[u8]) -> Result<PlayerInput, ProtocolError> {
    let mut cursor = body;
    let flags = take_u8(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(PlayerInput { flags })
}

pub fn decode_player_command(body: &[u8]) -> Result<PlayerCommand, ProtocolError> {
    let mut cursor = body;
    let entity_id = take_var_int(&mut cursor)?;
    let action = take_var_int(&mut cursor)?;
    let data = take_var_int(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(PlayerCommand {
        entity_id,
        action,
        data,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub fn from_long(value: i64) -> BlockPos {
        BlockPos {
            x: (value >> 38) as i32,
            y: ((value << 52) >> 52) as i32,
            z: ((value << 26) >> 38) as i32,
        }
    }

    pub fn to_long(self) -> i64 {
        ((self.x as i64 & 0x3FF_FFFF) << 38)
            | ((self.z as i64 & 0x3FF_FFFF) << 12)
            | (self.y as i64 & 0xFFF)
    }

    pub fn offset(self, dx: i32, dy: i32, dz: i32) -> BlockPos {
        BlockPos {
            x: self.x + dx,
            y: self.y + dy,
            z: self.z + dz,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStack {
    Empty,
    Item { count: i32, item: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerAction {
    pub action: i32,
    pub pos: BlockPos,
    pub face: u8,
    pub sequence: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UseItemOn {
    pub hand: i32,
    pub pos: BlockPos,
    pub face: i32,
    pub cursor: [f32; 3],
    pub inside: bool,
    pub world_border_hit: bool,
    pub sequence: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UseItem {
    pub hand: i32,
    pub sequence: i32,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetCarriedItem {
    pub slot: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetCreativeModeSlot {
    pub slot: i16,
    pub stack: ItemStack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickItemFromBlock {
    pub pos: BlockPos,
    pub include_data: bool,
}

fn take_i16(cursor: &mut &[u8]) -> Result<i16, ProtocolError> {
    match read_u16(cursor)? {
        Some((value, consumed)) => {
            *cursor = &cursor[consumed..];
            Ok(value as i16)
        }
        None => Err(ProtocolError::UnexpectedEof),
    }
}

fn take_bool(cursor: &mut &[u8]) -> Result<bool, ProtocolError> {
    Ok(take_u8(cursor)? != 0)
}

fn skip_bytes(cursor: &mut &[u8], count: usize) -> Result<(), ProtocolError> {
    if cursor.len() < count {
        return Err(ProtocolError::UnexpectedEof);
    }
    *cursor = &cursor[count..];
    Ok(())
}

fn decode_item_stack(cursor: &mut &[u8]) -> Result<ItemStack, ProtocolError> {
    let count = take_var_int(cursor)?;
    if count <= 0 {
        return Ok(ItemStack::Empty);
    }
    let item = take_var_int(cursor)?;
    let added_count = take_var_int(cursor)?;
    let removed_count = take_var_int(cursor)?;
    if added_count < 0 || removed_count < 0 {
        return Err(ProtocolError::PrefixedArrayLengthNegative);
    }
    for _ in 0..added_count {
        let _type = take_var_int(cursor)?;
        let len = take_var_int(cursor)?;
        if len < 0 {
            return Err(ProtocolError::PrefixedArrayLengthNegative);
        }
        skip_bytes(cursor, len as usize)?;
    }
    for _ in 0..removed_count {
        let _type = take_var_int(cursor)?;
    }
    Ok(ItemStack::Item { count, item })
}

pub fn decode_player_action(body: &[u8]) -> Result<PlayerAction, ProtocolError> {
    let mut cursor = body;
    let action = take_var_int(&mut cursor)?;
    let pos = BlockPos::from_long(take_i64(&mut cursor)?);
    let face = take_u8(&mut cursor)?;
    let sequence = take_var_int(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(PlayerAction {
        action,
        pos,
        face,
        sequence,
    })
}

pub fn decode_use_item_on(body: &[u8]) -> Result<UseItemOn, ProtocolError> {
    let mut cursor = body;
    let hand = take_var_int(&mut cursor)?;
    let pos = BlockPos::from_long(take_i64(&mut cursor)?);
    let face = take_var_int(&mut cursor)?;
    let cursor_x = take_f32(&mut cursor)?;
    let cursor_y = take_f32(&mut cursor)?;
    let cursor_z = take_f32(&mut cursor)?;
    let inside = take_bool(&mut cursor)?;
    let world_border_hit = take_bool(&mut cursor)?;
    let sequence = take_var_int(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(UseItemOn {
        hand,
        pos,
        face,
        cursor: [cursor_x, cursor_y, cursor_z],
        inside,
        world_border_hit,
        sequence,
    })
}

pub fn decode_use_item(body: &[u8]) -> Result<UseItem, ProtocolError> {
    let mut cursor = body;
    let hand = take_var_int(&mut cursor)?;
    let sequence = take_var_int(&mut cursor)?;
    let yaw = take_f32(&mut cursor)?;
    let pitch = take_f32(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(UseItem {
        hand,
        sequence,
        yaw,
        pitch,
    })
}

pub fn decode_set_carried_item(body: &[u8]) -> Result<SetCarriedItem, ProtocolError> {
    let mut cursor = body;
    let slot = take_i16(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(SetCarriedItem { slot })
}

pub fn decode_set_creative_mode_slot(body: &[u8]) -> Result<SetCreativeModeSlot, ProtocolError> {
    let mut cursor = body;
    let slot = take_i16(&mut cursor)?;
    let stack = decode_item_stack(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(SetCreativeModeSlot { slot, stack })
}

pub fn decode_pick_item_from_block(body: &[u8]) -> Result<PickItemFromBlock, ProtocolError> {
    let mut cursor = body;
    let pos = BlockPos::from_long(take_i64(&mut cursor)?);
    let include_data = take_bool(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(PickItemFromBlock { pos, include_data })
}

pub fn container_set_slot_payload(
    window_id: i32,
    state_id: i32,
    slot: i16,
    stack: ItemStack,
) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(window_id, &mut payload);
    write_var_int(state_id, &mut payload);
    crate::primitives::write_u16(slot as u16, &mut payload);
    match stack {
        ItemStack::Item { count, item } if count > 0 => {
            write_var_int(count, &mut payload);
            write_var_int(item, &mut payload);
            write_var_int(0, &mut payload);
            write_var_int(0, &mut payload);
        }
        _ => write_var_int(0, &mut payload),
    }
    payload
}

pub fn set_held_slot_payload(slot: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(slot, &mut payload);
    payload
}

pub fn block_update_payload(pos: BlockPos, state: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    write_i64(pos.to_long(), &mut payload);
    write_var_int(state, &mut payload);
    payload
}

pub fn block_changed_ack_payload(sequence: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(sequence, &mut payload);
    payload
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::write_u16;
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
        assert_eq!(packet_ids::serverbound::PLAYER_ACTION, 0x29);
        assert_eq!(packet_ids::serverbound::PLAYER_COMMAND, 0x2A);
        assert_eq!(packet_ids::serverbound::PLAYER_INPUT, 0x2B);
        assert_eq!(packet_ids::serverbound::USE_ITEM_ON, 0x42);
        assert_eq!(packet_ids::serverbound::USE_ITEM, 0x43);
        assert_eq!(packet_ids::serverbound::SET_CARRIED_ITEM, 0x36);
        assert_eq!(packet_ids::serverbound::SET_CREATIVE_MODE_SLOT, 0x39);
        assert_eq!(packet_ids::clientbound::BLOCK_UPDATE, 0x08);
        assert_eq!(packet_ids::clientbound::BLOCK_CHANGED_ACK, 0x04);
        assert_eq!(packet_ids::clientbound::CONTAINER_SET_SLOT, 0x14);
        assert_eq!(packet_ids::clientbound::SET_HELD_SLOT, 0x6B);
        assert_eq!(packet_ids::serverbound::PICK_ITEM_FROM_BLOCK, 0x24);
        assert_eq!(packet_ids::clientbound::PLAYER_INFO_UPDATE, 0x47);
        assert_eq!(packet_ids::clientbound::PLAYER_INFO_REMOVE, 0x46);
        assert_eq!(packet_ids::clientbound::TAB_LIST, 0x7D);
        assert_eq!(packet_ids::clientbound::BUNDLE_DELIMITER, 0x00);
        assert_eq!(packet_ids::clientbound::ADD_ENTITY, 0x01);
        assert_eq!(packet_ids::clientbound::REMOVE_ENTITIES, 0x4E);
        assert_eq!(packet_ids::clientbound::SET_ENTITY_DATA, 0x65);
    }

    #[test]
    fn decode_pick_item_from_block_fields() {
        let mut body = Vec::new();
        write_i64(
            BlockPos {
                x: 5,
                y: -61,
                z: -3,
            }
            .to_long(),
            &mut body,
        );
        body.push(1);
        assert_eq!(
            decode_pick_item_from_block(&body).unwrap(),
            PickItemFromBlock {
                pos: BlockPos {
                    x: 5,
                    y: -61,
                    z: -3
                },
                include_data: true,
            }
        );
    }

    #[test]
    fn decode_pick_item_from_block_reads_false_flag() {
        let mut body = Vec::new();
        write_i64(BlockPos { x: 0, y: 0, z: 0 }.to_long(), &mut body);
        body.push(0);
        assert_eq!(
            decode_pick_item_from_block(&body).unwrap(),
            PickItemFromBlock {
                pos: BlockPos { x: 0, y: 0, z: 0 },
                include_data: false,
            }
        );
    }

    #[test]
    fn decode_pick_item_from_block_truncated_is_eof() {
        let mut body = Vec::new();
        write_i64(BlockPos { x: 1, y: 2, z: 3 }.to_long(), &mut body);
        assert_eq!(
            decode_pick_item_from_block(&body),
            Err(ProtocolError::UnexpectedEof)
        );
    }

    #[test]
    fn decode_pick_item_from_block_trailing_is_error() {
        let mut body = Vec::new();
        write_i64(BlockPos { x: 1, y: 2, z: 3 }.to_long(), &mut body);
        body.push(0);
        body.push(0xFF);
        assert_eq!(
            decode_pick_item_from_block(&body),
            Err(ProtocolError::TrailingBytes)
        );
    }

    #[test]
    fn container_set_slot_payload_with_item() {
        let payload = container_set_slot_payload(0, 7, 36, ItemStack::Item { count: 1, item: 54 });
        let mut cursor = &payload[..];
        let (window, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(window, 0);
        cursor = &cursor[c..];
        let (state_id, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(state_id, 7);
        cursor = &cursor[c..];
        assert_eq!(u16::from_be_bytes([cursor[0], cursor[1]]), 36);
        cursor = &cursor[2..];
        let (count, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(count, 1);
        cursor = &cursor[c..];
        let (item, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(item, 54);
        cursor = &cursor[c..];
        let (added, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(added, 0);
        cursor = &cursor[c..];
        let (removed, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(removed, 0);
        cursor = &cursor[c..];
        assert!(cursor.is_empty());
    }

    #[test]
    fn container_set_slot_payload_empty_is_count_zero() {
        let payload = container_set_slot_payload(0, 3, 9, ItemStack::Empty);
        let mut cursor = &payload[..];
        let (window, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(window, 0);
        cursor = &cursor[c..];
        let (state_id, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(state_id, 3);
        cursor = &cursor[c..];
        assert_eq!(u16::from_be_bytes([cursor[0], cursor[1]]), 9);
        cursor = &cursor[2..];
        let (count, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(count, 0);
        cursor = &cursor[c..];
        assert!(cursor.is_empty());
    }

    #[test]
    fn decode_move_player_rot_round_trip() {
        let mut body = Vec::new();
        write_f32(90.0, &mut body);
        write_f32(30.0, &mut body);
        body.push(0x01);
        assert_eq!(
            decode_move_player_rot(&body).unwrap(),
            MovePlayerRot {
                yaw: 90.0,
                pitch: 30.0,
                flags: 0x01,
            }
        );
    }

    #[test]
    fn decode_move_player_rot_preserves_non_finite() {
        let mut body = Vec::new();
        write_f32(f32::NAN, &mut body);
        write_f32(0.0, &mut body);
        body.push(0x00);
        let decoded = decode_move_player_rot(&body).unwrap();
        assert!(decoded.yaw.is_nan());
        assert_eq!(decoded.pitch, 0.0);
    }

    #[test]
    fn decode_move_player_pos_rot_round_trip() {
        let mut body = Vec::new();
        for value in [48.5f64, -60.0, 0.5] {
            write_f64(value, &mut body);
        }
        write_f32(-45.0, &mut body);
        write_f32(12.5, &mut body);
        body.push(0x00);
        assert_eq!(
            decode_move_player_pos_rot(&body).unwrap(),
            MovePlayerPosRot {
                x: 48.5,
                y: -60.0,
                z: 0.5,
                yaw: -45.0,
                pitch: 12.5,
                flags: 0x00,
            }
        );
    }

    #[test]
    fn decode_player_input_round_trip_and_sneaking() {
        let input = decode_player_input(&[PlayerInput::SNEAK | PlayerInput::FORWARD]).unwrap();
        assert_eq!(input.flags, 0x21);
        assert!(input.sneaking());

        let no_sneak = decode_player_input(&[PlayerInput::SPRINT]).unwrap();
        assert!(!no_sneak.sneaking());
    }

    #[test]
    fn decode_player_input_truncated_is_eof() {
        assert_eq!(decode_player_input(&[]), Err(ProtocolError::UnexpectedEof));
    }

    #[test]
    fn decode_player_input_trailing_is_error() {
        assert_eq!(
            decode_player_input(&[0x20, 0x00]),
            Err(ProtocolError::TrailingBytes)
        );
    }

    #[test]
    fn decode_player_command_round_trip() {
        let mut body = Vec::new();
        write_var_int(1, &mut body);
        write_var_int(0, &mut body);
        write_var_int(0, &mut body);
        assert_eq!(
            decode_player_command(&body).unwrap(),
            PlayerCommand {
                entity_id: 1,
                action: 0,
                data: 0,
            }
        );
    }

    #[test]
    fn set_held_slot_payload_is_var_int() {
        let payload = set_held_slot_payload(8);
        let (slot, consumed) = read_var_int(&payload).unwrap().unwrap();
        assert_eq!(slot, 8);
        assert_eq!(consumed, payload.len());
    }

    #[test]
    fn block_pos_round_trip_including_negatives() {
        for pos in [
            BlockPos { x: 0, y: 0, z: 0 },
            BlockPos { x: 1, y: -60, z: 2 },
            BlockPos {
                x: -1,
                y: -64,
                z: -1,
            },
            BlockPos {
                x: -1000,
                y: 319,
                z: 1000,
            },
            BlockPos {
                x: 33_554_431,
                y: -2048,
                z: -33_554_432,
            },
        ] {
            assert_eq!(BlockPos::from_long(pos.to_long()), pos);
        }
    }

    #[test]
    fn block_pos_known_vector() {
        let pos = BlockPos { x: 1, y: 2, z: 3 };
        let packed = pos.to_long();
        assert_eq!(BlockPos::from_long(packed), pos);
        let negative = BlockPos {
            x: -1,
            y: -1,
            z: -1,
        };
        assert_eq!(BlockPos::from_long(negative.to_long()), negative);
    }

    #[test]
    fn decode_player_action_fields() {
        let mut body = Vec::new();
        write_var_int(0, &mut body);
        write_i64(
            BlockPos {
                x: -1,
                y: -60,
                z: 2,
            }
            .to_long(),
            &mut body,
        );
        body.push(1);
        write_var_int(7, &mut body);
        assert_eq!(
            decode_player_action(&body).unwrap(),
            PlayerAction {
                action: 0,
                pos: BlockPos {
                    x: -1,
                    y: -60,
                    z: 2
                },
                face: 1,
                sequence: 7,
            }
        );
    }

    #[test]
    fn decode_use_item_on_fields() {
        let mut body = Vec::new();
        write_var_int(0, &mut body);
        write_i64(
            BlockPos {
                x: 3,
                y: -61,
                z: -4,
            }
            .to_long(),
            &mut body,
        );
        write_var_int(1, &mut body);
        write_f32(0.5, &mut body);
        write_f32(1.0, &mut body);
        write_f32(0.25, &mut body);
        body.push(0);
        body.push(1);
        write_var_int(12, &mut body);
        assert_eq!(
            decode_use_item_on(&body).unwrap(),
            UseItemOn {
                hand: 0,
                pos: BlockPos {
                    x: 3,
                    y: -61,
                    z: -4
                },
                face: 1,
                cursor: [0.5, 1.0, 0.25],
                inside: false,
                world_border_hit: true,
                sequence: 12,
            }
        );
    }

    #[test]
    fn decode_use_item_fields() {
        let mut body = Vec::new();
        write_var_int(0, &mut body);
        write_var_int(9, &mut body);
        write_f32(12.5, &mut body);
        write_f32(-45.0, &mut body);
        assert_eq!(
            decode_use_item(&body).unwrap(),
            UseItem {
                hand: 0,
                sequence: 9,
                yaw: 12.5,
                pitch: -45.0,
            }
        );
    }

    #[test]
    fn decode_use_item_preserves_non_finite() {
        let mut body = Vec::new();
        write_var_int(1, &mut body);
        write_var_int(3, &mut body);
        write_f32(f32::NAN, &mut body);
        write_f32(0.0, &mut body);
        let decoded = decode_use_item(&body).unwrap();
        assert_eq!(decoded.hand, 1);
        assert!(decoded.yaw.is_nan());
    }

    #[test]
    fn decode_use_item_truncated_is_eof() {
        let mut body = Vec::new();
        write_var_int(0, &mut body);
        write_var_int(1, &mut body);
        write_f32(0.0, &mut body);
        assert_eq!(decode_use_item(&body), Err(ProtocolError::UnexpectedEof));
    }

    #[test]
    fn decode_use_item_trailing_is_error() {
        let mut body = Vec::new();
        write_var_int(0, &mut body);
        write_var_int(1, &mut body);
        write_f32(0.0, &mut body);
        write_f32(0.0, &mut body);
        body.push(0xFF);
        assert_eq!(decode_use_item(&body), Err(ProtocolError::TrailingBytes));
    }

    #[test]
    fn decode_set_carried_item_fields() {
        let mut body = Vec::new();
        write_u16(5u16, &mut body);
        assert_eq!(
            decode_set_carried_item(&body).unwrap(),
            SetCarriedItem { slot: 5 }
        );
    }

    #[test]
    fn decode_set_creative_mode_slot_empty_stack() {
        let mut body = Vec::new();
        write_u16(36u16, &mut body);
        write_var_int(0, &mut body);
        assert_eq!(
            decode_set_creative_mode_slot(&body).unwrap(),
            SetCreativeModeSlot {
                slot: 36,
                stack: ItemStack::Empty,
            }
        );
    }

    #[test]
    fn decode_set_creative_mode_slot_skips_components() {
        let mut body = Vec::new();
        write_u16(36u16, &mut body);
        write_var_int(1, &mut body);
        write_var_int(55, &mut body);
        write_var_int(2, &mut body);
        write_var_int(1, &mut body);
        write_var_int(7, &mut body);
        write_var_int(3, &mut body);
        body.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        write_var_int(8, &mut body);
        write_var_int(1, &mut body);
        body.extend_from_slice(&[0xDD]);
        write_var_int(9, &mut body);
        assert_eq!(
            decode_set_creative_mode_slot(&body).unwrap(),
            SetCreativeModeSlot {
                slot: 36,
                stack: ItemStack::Item { count: 1, item: 55 },
            }
        );
    }

    #[test]
    fn decode_item_stack_truncated_component_is_eof() {
        let mut raw = Vec::new();
        write_var_int(1, &mut raw);
        write_var_int(55, &mut raw);
        write_var_int(1, &mut raw);
        write_var_int(0, &mut raw);
        write_var_int(0, &mut raw);
        write_var_int(4, &mut raw);
        raw.push(0x01);
        let mut body = &raw[..];
        assert_eq!(
            decode_item_stack(&mut body),
            Err(ProtocolError::UnexpectedEof)
        );
    }

    #[test]
    fn block_update_payload_layout() {
        let pos = BlockPos { x: 1, y: -60, z: 2 };
        let payload = block_update_payload(pos, 10);
        assert_eq!(&payload[..8], &pos.to_long().to_be_bytes());
        let (state, consumed) = read_var_int(&payload[8..]).unwrap().unwrap();
        assert_eq!(state, 10);
        assert_eq!(8 + consumed, payload.len());
    }

    #[test]
    fn block_changed_ack_payload_is_var_int() {
        let payload = block_changed_ack_payload(42);
        let (sequence, consumed) = read_var_int(&payload).unwrap().unwrap();
        assert_eq!(sequence, 42);
        assert_eq!(consumed, payload.len());
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

    fn take_bitset(cursor: &mut &[u8]) -> u64 {
        let (len, c) = read_var_int(cursor).unwrap().unwrap();
        *cursor = &cursor[c..];
        let len = len as usize;
        assert!(len <= 8);
        let mut bytes = [0u8; 8];
        bytes[..len].copy_from_slice(&cursor[..len]);
        *cursor = &cursor[len..];
        u64::from_le_bytes(bytes)
    }

    #[test]
    fn chunk_data_prefix_and_light_trailers() {
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

        assert_eq!(take_bitset(&mut cursor), 0x3FF_FFFE);
        assert_eq!(take_bitset(&mut cursor), 0);
        assert_eq!(take_bitset(&mut cursor), 0x1);
        assert_eq!(take_bitset(&mut cursor), 0x3FF_FFFF);

        let (sky_count, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(sky_count, 25);
        cursor = &cursor[c..];
        for _ in 0..sky_count {
            let (len, c) = read_var_int(cursor).unwrap().unwrap();
            assert_eq!(len, 2048);
            cursor = &cursor[c..];
            cursor = &cursor[len as usize..];
        }

        let (block_count, c) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(block_count, 0);
        cursor = &cursor[c..];

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

    fn login_play_game_types(payload: &[u8]) -> (i32, i32) {
        let mut cursor = &payload[4..];
        cursor = &cursor[1..];
        let (_, c) = read_var_int(cursor).unwrap().unwrap();
        cursor = &cursor[c..];
        let len = cursor[0] as usize;
        cursor = &cursor[1 + len..];
        for _ in 0..3 {
            let (_, c) = read_var_int(cursor).unwrap().unwrap();
            cursor = &cursor[c..];
        }
        cursor = &cursor[3..];
        let (_, c) = read_var_int(cursor).unwrap().unwrap();
        cursor = &cursor[c..];
        let len = cursor[0] as usize;
        cursor = &cursor[1 + len + 8..];
        let (game_type, c) = read_var_int(cursor).unwrap().unwrap();
        cursor = &cursor[c..];
        let (previous, _) = read_var_int(cursor).unwrap().unwrap();
        (game_type, previous)
    }

    #[test]
    fn login_play_encodes_each_game_mode_with_no_previous() {
        use crate::gamemode::GameMode;
        for mode in [
            GameMode::Survival,
            GameMode::Creative,
            GameMode::Adventure,
            GameMode::Spectator,
        ] {
            let payload = login_play_payload(1, 8, 8, mode.game_type(), 20).unwrap();
            let (game_type, previous) = login_play_game_types(&payload);
            assert_eq!(game_type, mode.game_type());
            assert_eq!(previous, PREVIOUS_GAME_TYPE_NONE);
        }

        assert_eq!(
            login_play_game_types(&login_play_payload(1, 8, 8, 0, 20).unwrap()).0,
            0
        );
        assert_eq!(
            login_play_game_types(&login_play_payload(1, 8, 8, 1, 20).unwrap()).0,
            1
        );
    }

    #[test]
    fn player_abilities_exact_bytes() {
        use crate::gamemode::GameMode;
        let creative = player_abilities_payload(GameMode::Creative.abilities_flags());
        assert_eq!(
            creative,
            vec![0x0D, 0x3D, 0x4C, 0xCC, 0xCD, 0x3D, 0xCC, 0xCC, 0xCD]
        );
        let spectator = player_abilities_payload(GameMode::Spectator.abilities_flags());
        assert_eq!(
            spectator,
            vec![0x07, 0x3D, 0x4C, 0xCC, 0xCD, 0x3D, 0xCC, 0xCC, 0xCD]
        );
    }

    #[test]
    fn decode_player_abilities_reads_flying_bit() {
        assert_eq!(decode_player_abilities(&[0x02]), Ok(true));
        assert_eq!(decode_player_abilities(&[0x00]), Ok(false));
        assert_eq!(decode_player_abilities(&[0x06]), Ok(true));
        assert_eq!(
            decode_player_abilities(&[]),
            Err(ProtocolError::UnexpectedEof)
        );
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

    #[test]
    fn forget_level_chunk_packs_z_high_x_low() {
        let payload = forget_level_chunk_payload(3, -2);
        assert_eq!(payload.len(), 8);
        let value = i64::from_be_bytes(payload.try_into().unwrap());
        assert_eq!((value & 0xFFFF_FFFF) as i32, 3);
        assert_eq!((value >> 32) as i32, -2);
    }

    #[test]
    fn forget_level_chunk_round_trips_signed_coords() {
        for (x, z) in [(0, 0), (-8, 8), (11, -9), (i32::MIN, i32::MAX)] {
            let payload = forget_level_chunk_payload(x, z);
            let value = i64::from_be_bytes(payload.try_into().unwrap());
            assert_eq!((value & 0xFFFF_FFFF) as i32, x);
            assert_eq!((value >> 32) as i32, z);
        }
    }

    #[test]
    fn move_player_pos_round_trip() {
        let mut body = Vec::new();
        for value in [48.5f64, -60.0, 0.5] {
            body.extend_from_slice(&value.to_be_bytes());
        }
        body.push(0x03);
        assert_eq!(
            decode_move_player_pos(&body).unwrap(),
            MovePlayerPos {
                x: 48.5,
                y: -60.0,
                z: 0.5,
                flags: 0x03,
            }
        );
    }

    #[test]
    fn move_player_pos_missing_flags_is_eof() {
        let mut body = Vec::new();
        for value in [1.0f64, 2.0, 3.0] {
            body.extend_from_slice(&value.to_be_bytes());
        }
        assert_eq!(
            decode_move_player_pos(&body),
            Err(ProtocolError::UnexpectedEof)
        );
    }

    #[test]
    fn move_player_pos_trailing_is_error() {
        let mut body = Vec::new();
        for value in [1.0f64, 2.0, 3.0] {
            body.extend_from_slice(&value.to_be_bytes());
        }
        body.push(0x01);
        body.push(0xFF);
        assert_eq!(
            decode_move_player_pos(&body),
            Err(ProtocolError::TrailingBytes)
        );
    }

    #[test]
    fn move_player_pos_rot_round_trip() {
        let mut body = Vec::new();
        for value in [1.0f64, 2.0, 3.0] {
            body.extend_from_slice(&value.to_be_bytes());
        }
        body.extend_from_slice(&90.0f32.to_be_bytes());
        body.extend_from_slice(&(-45.0f32).to_be_bytes());
        body.push(0x01);
        assert_eq!(
            decode_move_player_pos_rot(&body).unwrap(),
            MovePlayerPosRot {
                x: 1.0,
                y: 2.0,
                z: 3.0,
                yaw: 90.0,
                pitch: -45.0,
                flags: 0x01,
            }
        );
    }

    #[test]
    fn move_player_rot_round_trip() {
        let mut body = Vec::new();
        body.extend_from_slice(&12.5f32.to_be_bytes());
        body.extend_from_slice(&(-7.25f32).to_be_bytes());
        body.push(0x02);
        assert_eq!(
            decode_move_player_rot(&body).unwrap(),
            MovePlayerRot {
                yaw: 12.5,
                pitch: -7.25,
                flags: 0x02,
            }
        );
    }

    #[test]
    fn move_player_status_round_trip() {
        assert_eq!(
            decode_move_player_status(&[0x01]).unwrap(),
            MovePlayerStatus { flags: 0x01 }
        );
        assert_eq!(
            decode_move_player_status(&[]),
            Err(ProtocolError::UnexpectedEof)
        );
        assert_eq!(
            decode_move_player_status(&[0x00, 0x00]),
            Err(ProtocolError::TrailingBytes)
        );
    }
}
