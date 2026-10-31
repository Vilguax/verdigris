use std::sync::OnceLock;

use crate::blocks::{is_air, is_fluid, state_to_block};
use crate::light::{compute_chunk_light, write_light};
use crate::network::network_data;
use crate::primitives::{write_i32, write_i64, write_u16};
use crate::varint::write_var_int;

pub const SECTION_COUNT: usize = 24;
pub const BLOCKS_PER_SECTION: usize = 4096;
pub const BIOMES_PER_SECTION: usize = 64;
pub const COLUMNS_PER_CHUNK: usize = 256;

pub const MIN_Y: i32 = -64;
pub const WORLD_HEIGHT: usize = SECTION_COUNT * 16;
pub const HEIGHTMAP_BITS: u8 = 9;

const BIOME_REGISTRY: &str = "minecraft:worldgen/biome";
const BLOCK_TAG_REGISTRY: &str = "minecraft:block";
const MOTION_TAG: &str = "minecraft:blocks_motion_in_heightmap";
const MOTION_NO_LEAVES_TAG: &str = "minecraft:blocks_motion_in_heightmap_no_leaves";

const HEIGHTMAP_WORLD_SURFACE: i32 = 1;
const HEIGHTMAP_MOTION_BLOCKING: i32 = 4;
const HEIGHTMAP_MOTION_BLOCKING_NO_LEAVES: i32 = 5;

pub fn bits_for(palette_len: usize) -> u8 {
    if palette_len <= 1 {
        return 0;
    }
    let mut bits = 0u8;
    let mut capacity = 1usize;
    while capacity < palette_len {
        capacity <<= 1;
        bits += 1;
    }
    bits
}

pub fn pack_longs(entries: &[u32], bits: u8) -> Vec<i64> {
    if bits == 0 {
        return Vec::new();
    }
    let values_per_long = 64 / bits as usize;
    let long_count = entries.len().div_ceil(values_per_long);
    let mask: u64 = if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let mut longs = vec![0i64; long_count];
    for (index, &entry) in entries.iter().enumerate() {
        let long_index = index / values_per_long;
        let offset = (index % values_per_long) * bits as usize;
        longs[long_index] |= (((entry as u64) & mask) << offset) as i64;
    }
    longs
}

pub fn unpack_longs(data: &[i64], bits: u8, count: usize) -> Vec<u32> {
    if bits == 0 {
        return vec![0; count];
    }
    let values_per_long = 64 / bits as usize;
    let mask: u64 = if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let mut out = Vec::with_capacity(count);
    for &long in data {
        let block = long as u64;
        for slot in 0..values_per_long {
            if out.len() == count {
                return out;
            }
            let offset = slot * bits as usize;
            out.push(((block >> offset) & mask) as u32);
        }
    }
    out.resize(count, 0);
    out
}

pub struct PaletteThresholds {
    pub min_indirect_bits: u8,
    pub max_indirect_bits: u8,
    pub global_bits: u8,
}

pub fn block_thresholds() -> PaletteThresholds {
    PaletteThresholds {
        min_indirect_bits: 4,
        max_indirect_bits: 8,
        global_bits: 16,
    }
}

pub fn biome_global_bits() -> u8 {
    let count = network_data()
        .registry(BIOME_REGISTRY)
        .map(|registry| registry.entries.len())
        .unwrap_or(0);
    bits_for(count)
}

pub fn biome_thresholds() -> PaletteThresholds {
    PaletteThresholds {
        min_indirect_bits: 1,
        max_indirect_bits: 3,
        global_bits: biome_global_bits(),
    }
}

pub fn write_paletted_container(
    entries: &[u32],
    thresholds: &PaletteThresholds,
    out: &mut Vec<u8>,
) {
    let mut palette: Vec<u32> = Vec::new();
    for &entry in entries {
        if !palette.contains(&entry) {
            palette.push(entry);
        }
    }

    if palette.len() <= 1 {
        out.push(0);
        write_var_int(palette.first().copied().unwrap_or(0) as i32, out);
        return;
    }

    let needed = bits_for(palette.len());
    if needed <= thresholds.max_indirect_bits {
        let storage = needed.max(thresholds.min_indirect_bits);
        out.push(storage);
        write_var_int(palette.len() as i32, out);
        for &id in &palette {
            write_var_int(id as i32, out);
        }
        let indices: Vec<u32> = entries
            .iter()
            .map(|entry| palette.iter().position(|value| value == entry).unwrap() as u32)
            .collect();
        for long in pack_longs(&indices, storage) {
            write_i64(long, out);
        }
    } else {
        out.push(thresholds.global_bits);
        for long in pack_longs(entries, thresholds.global_bits) {
            write_i64(long, out);
        }
    }
}

pub fn block_index(x: usize, y: usize, z: usize) -> usize {
    y * 256 + z * 16 + x
}

pub fn biome_index(x: usize, y: usize, z: usize) -> usize {
    y * 16 + z * 4 + x
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub states: [u16; BLOCKS_PER_SECTION],
    pub biomes: [u16; BIOMES_PER_SECTION],
}

impl Section {
    pub fn air(biome: u16) -> Self {
        Section {
            states: [0; BLOCKS_PER_SECTION],
            biomes: [biome; BIOMES_PER_SECTION],
        }
    }

    pub fn from_cells(
        states: &[u16; BLOCKS_PER_SECTION],
        biomes: &[u16; BIOMES_PER_SECTION],
    ) -> Self {
        Section {
            states: *states,
            biomes: *biomes,
        }
    }

    pub fn state_at(&self, x: usize, y: usize, z: usize) -> u16 {
        self.states[block_index(x, y, z)]
    }

    pub fn biome_at(&self, x: usize, y: usize, z: usize) -> u16 {
        self.biomes[biome_index(x, y, z)]
    }

    pub fn non_empty_block_count(&self) -> u16 {
        self.states
            .iter()
            .filter(|&&state| !is_air(state as i32))
            .count() as u16
    }

    pub fn fluid_count(&self) -> u16 {
        self.states
            .iter()
            .filter(|&&state| is_fluid(state as i32))
            .count() as u16
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkColumn {
    pub sections: Vec<Section>,
}

impl ChunkColumn {
    pub fn empty(biome: u16) -> Self {
        ChunkColumn {
            sections: (0..SECTION_COUNT).map(|_| Section::air(biome)).collect(),
        }
    }

    pub fn from_sections(sections: Vec<Section>) -> Self {
        ChunkColumn { sections }
    }

    pub fn state_at(&self, x: usize, y: usize, z: usize) -> u16 {
        let section = &self.sections[y / 16];
        section.states[block_index(x, y % 16, z)]
    }

    pub fn is_all_air(&self) -> bool {
        self.sections
            .iter()
            .all(|section| section.states.iter().all(|&state| state == 0))
    }
}

pub fn write_section(section: &Section, out: &mut Vec<u8>) {
    write_u16(section.non_empty_block_count(), out);
    write_u16(section.fluid_count(), out);

    let block_entries: Vec<u32> = section.states.iter().map(|&state| state as u32).collect();
    write_paletted_container(&block_entries, &block_thresholds(), out);

    let biome_entries: Vec<u32> = section.biomes.iter().map(|&biome| biome as u32).collect();
    write_paletted_container(&biome_entries, &biome_thresholds(), out);
}

pub fn write_chunk_data(column: &ChunkColumn, out: &mut Vec<u8>) {
    for section in &column.sections {
        write_section(section, out);
    }
}

struct HeightmapTags {
    motion: Vec<i32>,
    motion_no_leaves: Vec<i32>,
}

static HEIGHTMAP_TAGS: OnceLock<HeightmapTags> = OnceLock::new();

fn heightmap_tags() -> &'static HeightmapTags {
    HEIGHTMAP_TAGS.get_or_init(|| {
        let tags = network_data()
            .tags
            .iter()
            .find(|registry| registry.registry == BLOCK_TAG_REGISTRY);
        let fetch = |name: &str| -> Vec<i32> {
            let mut ids = tags
                .and_then(|registry| registry.tags.iter().find(|tag| tag.name == name))
                .map(|tag| tag.ids.clone())
                .unwrap_or_default();
            ids.sort_unstable();
            ids
        };
        HeightmapTags {
            motion: fetch(MOTION_TAG),
            motion_no_leaves: fetch(MOTION_NO_LEAVES_TAG),
        }
    })
}

#[derive(Clone, Copy)]
enum HeightmapKind {
    WorldSurface,
    MotionBlocking,
    MotionBlockingNoLeaves,
}

fn state_matches(kind: HeightmapKind, state: u16) -> bool {
    let state = state as i32;
    match kind {
        HeightmapKind::WorldSurface => !is_air(state),
        HeightmapKind::MotionBlocking => {
            is_fluid(state)
                || heightmap_tags()
                    .motion
                    .binary_search(&state_to_block(state).id)
                    .is_ok()
        }
        HeightmapKind::MotionBlockingNoLeaves => {
            is_fluid(state)
                || heightmap_tags()
                    .motion_no_leaves
                    .binary_search(&state_to_block(state).id)
                    .is_ok()
        }
    }
}

pub fn world_surface_heights(column: &ChunkColumn) -> [u16; COLUMNS_PER_CHUNK] {
    let mut heights = [0u16; COLUMNS_PER_CHUNK];
    for z in 0..16 {
        for x in 0..16 {
            for y in (0..WORLD_HEIGHT).rev() {
                if !is_air(column.state_at(x, y, z) as i32) {
                    heights[x + z * 16] = (y + 1) as u16;
                    break;
                }
            }
        }
    }
    heights
}

fn heightmap_values(column: &ChunkColumn, kind: HeightmapKind) -> Vec<u32> {
    if matches!(kind, HeightmapKind::WorldSurface) {
        return world_surface_heights(column)
            .iter()
            .map(|&height| height as u32)
            .collect();
    }

    let mut values = vec![0u32; COLUMNS_PER_CHUNK];
    for z in 0..16 {
        for x in 0..16 {
            let mut value = 0u32;
            for y in (0..WORLD_HEIGHT).rev() {
                if state_matches(kind, column.state_at(x, y, z)) {
                    value = (y + 1) as u32;
                    break;
                }
            }
            values[x + z * 16] = value;
        }
    }
    values
}

fn write_one_heightmap(type_id: i32, values: &[u32], out: &mut Vec<u8>) {
    write_var_int(type_id, out);
    let longs = pack_longs(values, HEIGHTMAP_BITS);
    write_var_int(longs.len() as i32, out);
    for long in longs {
        write_i64(long, out);
    }
}

pub fn write_heightmaps(column: &ChunkColumn, out: &mut Vec<u8>) {
    if column.is_all_air() {
        write_var_int(0, out);
        return;
    }

    let world_surface = heightmap_values(column, HeightmapKind::WorldSurface);
    let motion_blocking = heightmap_values(column, HeightmapKind::MotionBlocking);
    let motion_blocking_no_leaves = heightmap_values(column, HeightmapKind::MotionBlockingNoLeaves);

    write_var_int(3, out);
    write_one_heightmap(HEIGHTMAP_WORLD_SURFACE, &world_surface, out);
    write_one_heightmap(HEIGHTMAP_MOTION_BLOCKING, &motion_blocking, out);
    write_one_heightmap(
        HEIGHTMAP_MOTION_BLOCKING_NO_LEAVES,
        &motion_blocking_no_leaves,
        out,
    );
}

pub fn write_chunk_payload(x: i32, z: i32, column: &ChunkColumn, out: &mut Vec<u8>) {
    write_i32(x, out);
    write_i32(z, out);

    write_heightmaps(column, out);

    let mut data = Vec::new();
    write_chunk_data(column, &mut data);
    write_var_int(data.len() as i32, out);
    out.extend_from_slice(&data);

    write_var_int(0, out);

    let light = compute_chunk_light(column);
    write_light(&light, out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::varint::read_var_int;

    const WATER_STATE: u16 = 89;
    const WATERLOGGED_OAK_STAIR_STATE: u16 = 5463;

    #[test]
    fn bits_for_ceil_log2() {
        assert_eq!(bits_for(1), 0);
        assert_eq!(bits_for(2), 1);
        assert_eq!(bits_for(3), 2);
        assert_eq!(bits_for(4), 2);
        assert_eq!(bits_for(16), 4);
        assert_eq!(bits_for(17), 5);
        assert_eq!(bits_for(256), 8);
        assert_eq!(bits_for(257), 9);
        assert_eq!(bits_for(67), 7);
    }

    #[test]
    fn pack_long_counts_for_block_widths() {
        let entries = vec![0u32; 4096];
        assert_eq!(pack_longs(&entries, 4).len(), 256);
        assert_eq!(pack_longs(&entries, 5).len(), 342);
        assert_eq!(pack_longs(&entries, 6).len(), 410);
        assert_eq!(pack_longs(&entries, 7).len(), 456);
        assert_eq!(pack_longs(&entries, 8).len(), 512);
        assert_eq!(pack_longs(&entries, 16).len(), 1024);
    }

    #[test]
    fn pack_long_counts_for_biome_widths() {
        let entries = vec![0u32; 64];
        assert_eq!(pack_longs(&entries, 1).len(), 1);
        assert_eq!(pack_longs(&entries, 2).len(), 2);
        assert_eq!(pack_longs(&entries, 3).len(), 4);
        assert_eq!(pack_longs(&entries, 7).len(), 8);
    }

    #[test]
    fn packing_never_straddles_two_longs() {
        for bits in [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 16] {
            let values_per_long = 64 / bits as usize;
            let entries: Vec<u32> = (0..256).map(|i| (i % (1 << bits)) as u32).collect();
            let longs = pack_longs(&entries, bits);
            for (index, &entry) in entries.iter().enumerate() {
                let long_index = index / values_per_long;
                let offset = (index % values_per_long) * bits as usize;
                assert!(offset + bits as usize <= 64);
                let mask: u64 = (1u64 << bits) - 1;
                let stored = ((longs[long_index] as u64) >> offset) & mask;
                assert_eq!(stored as u32, entry);
            }
        }
    }

    fn storage_bits(entries: &[u32], thresholds: &PaletteThresholds) -> u8 {
        let mut buffer = Vec::new();
        write_paletted_container(entries, thresholds, &mut buffer);
        buffer[0]
    }

    fn distinct_block_entries(count: usize) -> Vec<u32> {
        let mut entries = vec![0u32; 4096];
        for (index, slot) in entries.iter_mut().take(count).enumerate() {
            *slot = index as u32;
        }
        entries
    }

    #[test]
    fn block_palette_storage_thresholds() {
        let thresholds = block_thresholds();
        assert_eq!(storage_bits(&distinct_block_entries(1), &thresholds), 0);
        assert_eq!(storage_bits(&distinct_block_entries(2), &thresholds), 4);
        assert_eq!(storage_bits(&distinct_block_entries(16), &thresholds), 4);
        assert_eq!(storage_bits(&distinct_block_entries(17), &thresholds), 5);
        assert_eq!(storage_bits(&distinct_block_entries(256), &thresholds), 8);
        assert_eq!(storage_bits(&distinct_block_entries(257), &thresholds), 16);
    }

    #[test]
    fn biome_palette_storage_thresholds() {
        let thresholds = biome_thresholds();
        let make = |count: usize| {
            let mut entries = vec![0u32; 64];
            for (index, slot) in entries.iter_mut().take(count).enumerate() {
                *slot = index as u32;
            }
            entries
        };
        assert_eq!(storage_bits(&make(1), &thresholds), 0);
        assert_eq!(storage_bits(&make(2), &thresholds), 1);
        assert_eq!(storage_bits(&make(5), &thresholds), 3);
    }

    #[test]
    fn biome_global_bits_from_registry() {
        assert_eq!(biome_global_bits(), 7);
    }

    #[test]
    fn real_biome_ids_round_trip_through_network_and_disk() {
        let mut biomes = [41u16; BIOMES_PER_SECTION];
        biomes[1] = 11;
        biomes[2] = 0;
        biomes[3] = 7;
        let entries: Vec<u32> = biomes.iter().map(|&b| b as u32).collect();

        let mut buffer = Vec::new();
        write_paletted_container(&entries, &biome_thresholds(), &mut buffer);
        assert_eq!(
            buffer[0], 2,
            "4 biomes distincts -> palette indirecte 2 bits"
        );
        let (len, consumed) = read_var_int(&buffer[1..]).unwrap().unwrap();
        assert_eq!(len, 4);
        let mut cursor = 1 + consumed;
        let mut palette = Vec::new();
        for _ in 0..4 {
            let (id, c) = read_var_int(&buffer[cursor..]).unwrap().unwrap();
            palette.push(id as u16);
            cursor += c;
        }
        assert_eq!(
            palette,
            vec![41, 11, 0, 7],
            "palette dans l'ordre de première apparition"
        );

        let states = [0u16; BLOCKS_PER_SECTION];
        let section = Section::from_cells(&states, &biomes);
        assert_eq!(section.biome_at(1, 0, 0), 11);
        assert_eq!(section.biome_at(2, 0, 0), 0);
        assert_eq!(section.biome_at(3, 0, 0), 7);
        let indices: Vec<u32> = entries
            .iter()
            .map(|e| palette.iter().position(|p| *p as u32 == *e).unwrap() as u32)
            .collect();
        let packed = pack_longs(&indices, 2);
        assert_eq!(unpack_longs(&packed, 2, 64), indices);
    }

    #[test]
    fn unique_container_has_no_data_longs() {
        let mut buffer = Vec::new();
        write_paletted_container(&vec![7u32; 4096], &block_thresholds(), &mut buffer);
        assert_eq!(buffer[0], 0);
        let (id, consumed) = read_var_int(&buffer[1..]).unwrap().unwrap();
        assert_eq!(id, 7);
        assert_eq!(1 + consumed, buffer.len());
    }

    #[test]
    fn global_container_has_no_length_prefix() {
        let entries = distinct_block_entries(257);
        let mut buffer = Vec::new();
        write_paletted_container(&entries, &block_thresholds(), &mut buffer);
        assert_eq!(buffer[0], 16);
        assert_eq!(buffer.len(), 1 + 1024 * 8);
    }

    #[test]
    fn empty_section_is_eight_bytes() {
        let mut buffer = Vec::new();
        write_section(&Section::air(41), &mut buffer);
        assert_eq!(buffer, vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x29]);
    }

    #[test]
    fn fluid_count_includes_water_and_waterlogged() {
        let mut section = Section::air(41);
        section.states[0] = WATER_STATE;
        section.states[1] = WATERLOGGED_OAK_STAIR_STATE;
        assert_eq!(section.fluid_count(), 2);
        assert_eq!(section.non_empty_block_count(), 2);
    }

    #[test]
    fn heightmap_top_block_value() {
        let mut column = ChunkColumn::empty(41);
        let y = (63 - MIN_Y) as usize;
        column.sections[y / 16].states[block_index(0, y % 16, 0)] = 1;

        let values = heightmap_values(&column, HeightmapKind::WorldSurface);
        assert_eq!(values[0], 128);

        let longs = pack_longs(&values, HEIGHTMAP_BITS);
        assert_eq!(longs.len(), 37);
    }

    #[test]
    fn heightmaps_of_empty_column_are_absent() {
        let mut buffer = Vec::new();
        write_heightmaps(&ChunkColumn::empty(41), &mut buffer);
        assert_eq!(buffer, vec![0x00]);
    }

    #[test]
    fn non_empty_heightmaps_have_three_entries() {
        let mut column = ChunkColumn::empty(41);
        let y = (63 - MIN_Y) as usize;
        column.sections[y / 16].states[block_index(0, y % 16, 0)] = 1;

        let mut buffer = Vec::new();
        write_heightmaps(&column, &mut buffer);
        let (count, consumed) = read_var_int(&buffer).unwrap().unwrap();
        assert_eq!(count, 3);

        let mut cursor = &buffer[consumed..];
        for expected_type in [1, 4, 5] {
            let (type_id, used) = read_var_int(cursor).unwrap().unwrap();
            assert_eq!(type_id, expected_type);
            cursor = &cursor[used..];
            let (long_count, used) = read_var_int(cursor).unwrap().unwrap();
            assert_eq!(long_count, 37);
            cursor = &cursor[used..];
            cursor = &cursor[long_count as usize * 8..];
        }
        assert!(cursor.is_empty());
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
    fn empty_column_payload_carries_full_sky_light() {
        let mut buffer = Vec::new();
        write_chunk_payload(0, 0, &ChunkColumn::empty(41), &mut buffer);

        let mut cursor = &buffer[8..];
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
    fn index_formulas() {
        assert_eq!(block_index(0, 0, 0), 0);
        assert_eq!(block_index(1, 0, 0), 1);
        assert_eq!(block_index(0, 0, 1), 16);
        assert_eq!(block_index(0, 1, 0), 256);
        assert_eq!(biome_index(0, 0, 0), 0);
        assert_eq!(biome_index(1, 0, 0), 1);
        assert_eq!(biome_index(0, 0, 1), 4);
        assert_eq!(biome_index(0, 1, 0), 16);
    }
}
