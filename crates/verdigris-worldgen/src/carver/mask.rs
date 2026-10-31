pub const CARVE_MIN_Y: i32 = -63;
pub const CARVE_MAX_Y: i32 = 312;
pub const CARVE_GEN_MIN_Y: i32 = -64;
pub const CARVE_GEN_TOP_Y: i32 = 319;
pub const CARVE_ROWS: usize = (CARVE_MAX_Y - CARVE_MIN_Y + 1) as usize;
pub const CARVE_COLUMNS: usize = 16 * 16;
pub const CARVE_CELLS: usize = CARVE_COLUMNS * CARVE_ROWS;

pub struct CarveMask {
    bits: Vec<u64>,
}

impl Default for CarveMask {
    fn default() -> Self {
        Self::new()
    }
}

impl CarveMask {
    pub fn new() -> Self {
        CarveMask {
            bits: vec![0u64; CARVE_CELLS.div_ceil(64)],
        }
    }

    pub fn clear(&mut self) {
        self.bits.fill(0);
    }

    pub fn contains_y(y: i32) -> bool {
        (CARVE_MIN_Y..=CARVE_MAX_Y).contains(&y)
    }

    pub fn index(lx: usize, y: i32, lz: usize) -> usize {
        (y - CARVE_MIN_Y) as usize + (lz + 16 * lx) * CARVE_ROWS
    }

    pub fn set(&mut self, lx: usize, y: i32, lz: usize) {
        if !Self::contains_y(y) || lx >= 16 || lz >= 16 {
            return;
        }
        let idx = Self::index(lx, y, lz);
        self.bits[idx >> 6] |= 1u64 << (idx & 63);
    }

    pub fn get(&self, lx: usize, y: i32, lz: usize) -> bool {
        if !Self::contains_y(y) || lx >= 16 || lz >= 16 {
            return false;
        }
        let idx = Self::index(lx, y, lz);
        (self.bits[idx >> 6] >> (idx & 63)) & 1 != 0
    }

    pub fn any(&self) -> bool {
        self.bits.iter().any(|&w| w != 0)
    }

    pub fn count(&self) -> u32 {
        self.bits.iter().map(|w| w.count_ones()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_count_is_376() {
        assert_eq!(CARVE_ROWS, 376);
    }

    #[test]
    fn set_and_get_roundtrip() {
        let mut m = CarveMask::new();
        assert!(!m.get(3, 10, 7));
        m.set(3, 10, 7);
        assert!(m.get(3, 10, 7));
        assert!(!m.get(3, 11, 7));
        assert_eq!(m.count(), 1);
    }

    #[test]
    fn out_of_range_y_is_ignored() {
        let mut m = CarveMask::new();
        m.set(0, -64, 0);
        m.set(0, 313, 0);
        assert!(!m.any());
    }

    #[test]
    fn index_formula_matches_spec() {
        assert_eq!(CarveMask::index(0, CARVE_MIN_Y, 0), 0);
        assert_eq!(CarveMask::index(0, CARVE_MIN_Y + 1, 0), 1);
        assert_eq!(CarveMask::index(0, CARVE_MIN_Y, 1), CARVE_ROWS);
        assert_eq!(CarveMask::index(1, CARVE_MIN_Y, 0), 16 * CARVE_ROWS);
    }
}
