use crate::chunk::{ChunkColumn, SECTION_COUNT, block_index, world_surface_heights};
use crate::varint::write_var_int;

pub const LIGHT_SECTION_VALUES: usize = 4096;
pub const LIGHT_SECTION_BYTES: usize = LIGHT_SECTION_VALUES / 2;
pub const SKY_SECTIONS: usize = SECTION_COUNT + 2;
pub const EMPTY_BLOCK_MASK: u64 = (1u64 << SKY_SECTIONS) - 1;
pub const BELOW_WORLD_BIT: u32 = 0;
pub const ABOVE_WORLD_BIT: u32 = (SECTION_COUNT + 1) as u32;
pub const FULL_SKY: u8 = 15;

pub struct ChunkLight {
    pub sky_mask: u64,
    pub block_mask: u64,
    pub empty_sky_mask: u64,
    pub empty_block_mask: u64,
    pub sky_sections: Vec<[u8; LIGHT_SECTION_VALUES]>,
}

pub fn write_bitset(bits: u64, out: &mut Vec<u8>) {
    if bits == 0 {
        write_var_int(0, out);
    } else {
        write_var_int(1, out);
        out.extend_from_slice(&bits.to_be_bytes());
    }
}

pub fn full_light_section(value: u8) -> [u8; LIGHT_SECTION_VALUES] {
    [value & 0x0F; LIGHT_SECTION_VALUES]
}

pub fn write_light_section(nibbles: &[u8; LIGHT_SECTION_VALUES], out: &mut Vec<u8>) {
    write_var_int(LIGHT_SECTION_BYTES as i32, out);
    for pair in 0..LIGHT_SECTION_BYTES {
        let low = nibbles[2 * pair] & 0x0F;
        let high = nibbles[2 * pair + 1] & 0x0F;
        out.push(low | (high << 4));
    }
}

pub fn compute_chunk_light(column: &ChunkColumn) -> ChunkLight {
    let heights = world_surface_heights(column);

    let mut sky_mask = 0u64;
    let mut empty_sky_mask = 1u64 << BELOW_WORLD_BIT;
    let mut sky_sections = Vec::new();

    for section in 0..SECTION_COUNT {
        let base = section * 16;
        let mut nibbles = [0u8; LIGHT_SECTION_VALUES];
        let mut lit = false;
        for z in 0..16 {
            for x in 0..16 {
                let h = heights[x + z * 16] as usize;
                for ly in 0..16 {
                    if base + ly >= h {
                        nibbles[block_index(x, ly, z)] = FULL_SKY;
                        lit = true;
                    }
                }
            }
        }
        let bit = (section + 1) as u32;
        if lit {
            sky_mask |= 1u64 << bit;
            sky_sections.push(nibbles);
        } else {
            empty_sky_mask |= 1u64 << bit;
        }
    }

    sky_mask |= 1u64 << ABOVE_WORLD_BIT;
    sky_sections.push(full_light_section(FULL_SKY));

    debug_assert_eq!(sky_sections.len(), sky_mask.count_ones() as usize);

    ChunkLight {
        sky_mask,
        block_mask: 0,
        empty_sky_mask,
        empty_block_mask: EMPTY_BLOCK_MASK,
        sky_sections,
    }
}

pub fn write_light(light: &ChunkLight, out: &mut Vec<u8>) {
    write_bitset(light.sky_mask, out);
    write_bitset(light.block_mask, out);
    write_bitset(light.empty_sky_mask, out);
    write_bitset(light.empty_block_mask, out);

    write_var_int(light.sky_sections.len() as i32, out);
    for section in &light.sky_sections {
        write_light_section(section, out);
    }

    write_var_int(0, out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{MIN_Y, Section, WORLD_HEIGHT};
    use crate::varint::read_var_int;

    const SOLID: u16 = 1;
    const SKY_MASK_EMPTY: u64 = 0x3FF_FFFE;
    const EMPTY_SKY_MASK_EMPTY: u64 = 0x1;

    fn solid_column() -> ChunkColumn {
        ChunkColumn {
            sections: (0..SECTION_COUNT)
                .map(|_| Section {
                    states: [SOLID; LIGHT_SECTION_VALUES],
                    biomes: [41; 64],
                })
                .collect(),
        }
    }

    fn set_rel(column: &mut ChunkColumn, x: usize, rel_y: usize, z: usize, state: u16) {
        column.sections[rel_y / 16].states[block_index(x, rel_y % 16, z)] = state;
    }

    fn nibble_at(section: &[u8; LIGHT_SECTION_VALUES], x: usize, ly: usize, z: usize) -> u8 {
        section[block_index(x, ly, z)]
    }

    #[test]
    fn bitset_single_low_bit() {
        let mut out = Vec::new();
        write_bitset(1, &mut out);
        assert_eq!(out, vec![0x01, 0, 0, 0, 0, 0, 0, 0, 0x01]);
    }

    #[test]
    fn bitset_above_world_bit() {
        let mut out = Vec::new();
        write_bitset(1u64 << 25, &mut out);
        assert_eq!(out, vec![0x01, 0, 0, 0, 0, 0x02, 0, 0, 0]);
    }

    #[test]
    fn bitset_sky_mask_of_empty_chunk() {
        let mut out = Vec::new();
        write_bitset(0x3FF_FFFE, &mut out);
        assert_eq!(out, vec![0x01, 0, 0, 0, 0, 0x03, 0xFF, 0xFF, 0xFE]);
    }

    #[test]
    fn bitset_zero_is_empty_list() {
        let mut out = Vec::new();
        write_bitset(0, &mut out);
        assert_eq!(out, vec![0x00]);
    }

    #[test]
    fn bitset_all_twenty_six_bits() {
        let mut out = Vec::new();
        write_bitset(EMPTY_BLOCK_MASK, &mut out);
        assert_eq!(out, vec![0x01, 0, 0, 0, 0, 0x03, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn full_fifteen_section_is_all_ff() {
        let mut out = Vec::new();
        write_light_section(&full_light_section(15), &mut out);
        let (len, consumed) = read_var_int(&out).unwrap().unwrap();
        assert_eq!(len, LIGHT_SECTION_BYTES as i32);
        assert_eq!(out.len(), consumed + LIGHT_SECTION_BYTES);
        assert!(out[consumed..].iter().all(|&byte| byte == 0xFF));
    }

    #[test]
    fn full_zero_section_is_all_zero() {
        let mut out = Vec::new();
        write_light_section(&full_light_section(0), &mut out);
        let (_, consumed) = read_var_int(&out).unwrap().unwrap();
        assert!(out[consumed..].iter().all(|&byte| byte == 0x00));
    }

    #[test]
    fn even_nibble_occupies_low_bits() {
        let mut nibbles = [0u8; LIGHT_SECTION_VALUES];
        nibbles[0] = 0xA;
        nibbles[1] = 0xB;
        let mut out = Vec::new();
        write_light_section(&nibbles, &mut out);
        let (_, consumed) = read_var_int(&out).unwrap().unwrap();
        assert_eq!(out[consumed], 0xBA);
    }

    #[test]
    fn empty_chunk_masks() {
        let light = compute_chunk_light(&ChunkColumn::empty(41));
        assert_eq!(light.sky_mask, SKY_MASK_EMPTY);
        assert_eq!(light.block_mask, 0);
        assert_eq!(light.empty_sky_mask, EMPTY_SKY_MASK_EMPTY);
        assert_eq!(light.empty_block_mask, 0x3FF_FFFF);
        assert_eq!(light.sky_sections.len(), 25);
        for section in &light.sky_sections {
            assert!(section.iter().all(|&value| value == FULL_SKY));
        }
    }

    #[test]
    fn flat_four_layers_bottom_section_is_mixed() {
        let mut column = ChunkColumn::empty(41);
        for z in 0..16 {
            for x in 0..16 {
                for rel_y in 0..4 {
                    set_rel(&mut column, x, rel_y, z, SOLID);
                }
            }
        }
        let light = compute_chunk_light(&column);
        assert_eq!(light.sky_mask, SKY_MASK_EMPTY);
        assert_eq!(light.empty_sky_mask, EMPTY_SKY_MASK_EMPTY);

        let bottom = &light.sky_sections[0];
        for ly in 0..4 {
            assert_eq!(nibble_at(bottom, 0, ly, 0), 0);
        }
        for ly in 4..16 {
            assert_eq!(nibble_at(bottom, 0, ly, 0), FULL_SKY);
        }
        for section in &light.sky_sections[1..] {
            assert!(section.iter().all(|&value| value == FULL_SKY));
        }
    }

    #[test]
    fn section_table_count_equals_sky_mask_bits() {
        let empty = compute_chunk_light(&ChunkColumn::empty(41));
        assert_eq!(
            empty.sky_sections.len(),
            empty.sky_mask.count_ones() as usize
        );

        let mut column = ChunkColumn::empty(41);
        for z in 0..16 {
            for x in 0..16 {
                for rel_y in 0..4 {
                    set_rel(&mut column, x, rel_y, z, SOLID);
                }
            }
        }
        let flat = compute_chunk_light(&column);
        assert_eq!(flat.sky_sections.len(), flat.sky_mask.count_ones() as usize);

        let solid = compute_chunk_light(&solid_column());
        assert_eq!(
            solid.sky_sections.len(),
            solid.sky_mask.count_ones() as usize
        );
    }

    #[test]
    fn full_lower_sections_move_to_empty_sky() {
        let mut column = ChunkColumn::empty(41);
        for z in 0..16 {
            for x in 0..16 {
                for rel_y in 0..32 {
                    set_rel(&mut column, x, rel_y, z, SOLID);
                }
            }
        }
        let light = compute_chunk_light(&column);
        assert_eq!(light.empty_sky_mask & (1 << 1), 1 << 1);
        assert_eq!(light.empty_sky_mask & (1 << 2), 1 << 2);
        assert_eq!(light.sky_mask & (1 << 1), 0);
        assert_eq!(light.sky_mask & (1 << 2), 0);
        assert_eq!(light.sky_mask & (1 << 3), 1 << 3);
        assert_eq!(
            light.sky_sections.len(),
            light.sky_mask.count_ones() as usize
        );
    }

    #[test]
    fn boundary_block_near_min_y() {
        let mut column = ChunkColumn::empty(41);
        set_rel(&mut column, 0, 0, 0, SOLID);
        let heights = world_surface_heights(&column);
        assert_eq!(heights[0], 1);

        let light = compute_chunk_light(&column);
        let bottom = &light.sky_sections[0];
        assert_eq!(nibble_at(bottom, 0, 0, 0), 0);
        assert_eq!(nibble_at(bottom, 0, 1, 0), FULL_SKY);
    }

    #[test]
    fn boundary_block_at_world_top() {
        let mut column = ChunkColumn::empty(41);
        for z in 0..16 {
            for x in 0..16 {
                set_rel(&mut column, x, WORLD_HEIGHT - 1, z, SOLID);
            }
        }
        let heights = world_surface_heights(&column);
        assert_eq!(heights[0] as usize, WORLD_HEIGHT);

        let light = compute_chunk_light(&column);
        assert_eq!(light.sky_mask, 1u64 << ABOVE_WORLD_BIT);
        assert_eq!(light.empty_sky_mask, 0x1FF_FFFF);
        assert_eq!(light.sky_sections.len(), 1);
        assert!(light.sky_sections[0].iter().all(|&value| value == FULL_SKY));
    }

    #[test]
    fn empty_column_is_all_sky() {
        let heights = world_surface_heights(&ChunkColumn::empty(41));
        assert!(heights.iter().all(|&h| h == 0));
        let light = compute_chunk_light(&ChunkColumn::empty(41));
        for section in &light.sky_sections {
            assert!(section.iter().all(|&value| value == FULL_SKY));
        }
    }

    #[test]
    fn heightmap_matches_sky_boundary() {
        let mut column = ChunkColumn::empty(41);
        for z in 0..16 {
            for x in 0..16 {
                set_rel(&mut column, x, 20, z, SOLID);
            }
        }
        let heights = world_surface_heights(&column);
        assert_eq!(heights[0] as usize, 21);

        let light = compute_chunk_light(&column);
        let mut section_index = 0usize;
        for section in 0..SECTION_COUNT {
            let bit = (section + 1) as u32;
            if light.sky_mask & (1u64 << bit) == 0 {
                continue;
            }
            let base = section * 16;
            let table = &light.sky_sections[section_index];
            for ly in 0..16 {
                let rel_y = base + ly;
                let expected = if rel_y >= heights[0] as usize {
                    FULL_SKY
                } else {
                    0
                };
                assert_eq!(nibble_at(table, 0, ly, 0), expected);
            }
            section_index += 1;
        }
    }

    #[test]
    fn min_y_is_world_floor() {
        assert_eq!(MIN_Y, -64);
    }
}
