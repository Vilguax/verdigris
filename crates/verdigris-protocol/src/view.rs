use std::collections::HashSet;

pub const LOAD_RADIUS: i32 = 8;
pub const KEEP_RADIUS: i32 = 9;

pub fn chunk_coord(pos: f64) -> i32 {
    (pos.floor() as i32).div_euclid(16)
}

pub fn chebyshev(center: (i32, i32), chunk: (i32, i32)) -> i32 {
    (chunk.0 - center.0).abs().max((chunk.1 - center.1).abs())
}

fn distance_key(center: (i32, i32), chunk: (i32, i32)) -> (i64, i32, i32) {
    let dx = (chunk.0 - center.0) as i64;
    let dz = (chunk.1 - center.1) as i64;
    (dx * dx + dz * dz, chunk.0, chunk.1)
}

pub fn chunks_to_load(center: (i32, i32), sent: &HashSet<(i32, i32)>) -> Vec<(i32, i32)> {
    let mut missing = Vec::new();
    for dx in -LOAD_RADIUS..=LOAD_RADIUS {
        for dz in -LOAD_RADIUS..=LOAD_RADIUS {
            let chunk = (center.0 + dx, center.1 + dz);
            if !sent.contains(&chunk) {
                missing.push(chunk);
            }
        }
    }
    missing.sort_by_key(|&chunk| distance_key(center, chunk));
    missing
}

pub fn chunks_to_forget(center: (i32, i32), sent: &HashSet<(i32, i32)>) -> Vec<(i32, i32)> {
    let mut forgotten: Vec<(i32, i32)> = sent
        .iter()
        .copied()
        .filter(|&chunk| chebyshev(center, chunk) > KEEP_RADIUS)
        .collect();
    forgotten.sort_unstable();
    forgotten
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(center: (i32, i32), radius: i32) -> HashSet<(i32, i32)> {
        let mut set = HashSet::new();
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                set.insert((center.0 + dx, center.1 + dz));
            }
        }
        set
    }

    #[test]
    fn chunk_coord_floors_towards_negative_infinity() {
        assert_eq!(chunk_coord(0.5), 0);
        assert_eq!(chunk_coord(15.9), 0);
        assert_eq!(chunk_coord(16.0), 1);
        assert_eq!(chunk_coord(48.5), 3);
        assert_eq!(chunk_coord(-0.5), -1);
        assert_eq!(chunk_coord(-16.0), -1);
        assert_eq!(chunk_coord(-16.5), -2);
        assert_eq!(chunk_coord(-60.0), -4);
    }

    #[test]
    fn initial_load_covers_the_whole_radius_eight_square() {
        let empty = HashSet::new();
        let load = chunks_to_load((0, 0), &empty);
        assert_eq!(load.len(), 17 * 17);
        assert_eq!(load.len(), 289);
        assert_eq!(load[0], (0, 0));
        let set: HashSet<(i32, i32)> = load.iter().copied().collect();
        assert_eq!(set, square((0, 0), LOAD_RADIUS));
    }

    #[test]
    fn load_order_is_non_decreasing_distance() {
        let load = chunks_to_load((3, -2), &HashSet::new());
        let mut previous = -1i64;
        for &(x, z) in &load {
            let dx = (x - 3) as i64;
            let dz = (z + 2) as i64;
            let d2 = dx * dx + dz * dz;
            assert!(d2 >= previous, "distance must not decrease");
            previous = d2;
        }
    }

    #[test]
    fn nothing_already_sent_is_loaded_again() {
        let sent = square((0, 0), LOAD_RADIUS);
        assert!(chunks_to_load((0, 0), &sent).is_empty());
    }

    #[test]
    fn moving_three_chunks_loads_only_the_new_columns() {
        let sent = square((0, 0), LOAD_RADIUS);
        let load = chunks_to_load((3, 0), &sent);
        let set: HashSet<(i32, i32)> = load.iter().copied().collect();

        let mut expected = HashSet::new();
        for x in 9..=11 {
            for z in -8..=8 {
                expected.insert((x, z));
            }
        }
        assert_eq!(set, expected);
        assert_eq!(load.len(), 51);
        assert_eq!(load[0], (9, 0));
    }

    #[test]
    fn moving_three_chunks_forgets_only_the_far_columns() {
        let sent = square((0, 0), LOAD_RADIUS);
        let forget = chunks_to_forget((3, 0), &sent);
        let set: HashSet<(i32, i32)> = forget.iter().copied().collect();

        let mut expected = HashSet::new();
        for x in -8..=-7 {
            for z in -8..=8 {
                expected.insert((x, z));
            }
        }
        assert_eq!(set, expected);
        assert_eq!(forget.len(), 34);
    }

    #[test]
    fn keep_radius_tolerates_one_chunk_of_hysteresis() {
        let sent = square((0, 0), LOAD_RADIUS);
        assert!(chunks_to_forget((1, 0), &sent).is_empty());
        assert!(chunks_to_forget((0, 1), &sent).is_empty());
        assert_eq!(chunks_to_forget((2, 0), &sent).len(), 17);
    }
}
