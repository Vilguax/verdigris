use verdigris_protocol::{
    BlockPos, INTERIOR_OFFSETS, INTERIOR_SIZE, RING_SIZE, RING_TEMPLATE, World, end_portal_state,
    is_frame_facing,
};

const ORIGIN_ZERO: BlockPos = BlockPos { x: 0, y: 0, z: 0 };

pub fn ring_at(origin: BlockPos) -> [BlockPos; RING_SIZE] {
    let mut ring = [origin; RING_SIZE];
    for (slot, cell) in RING_TEMPLATE.iter().zip(ring.iter_mut()) {
        *cell = origin.offset(slot.dx, 0, slot.dz);
    }
    ring
}

pub fn interior_cells(origin: BlockPos) -> [BlockPos; INTERIOR_SIZE] {
    let mut cells = [origin; INTERIOR_SIZE];
    for (&(ox, oz), cell) in INTERIOR_OFFSETS.iter().zip(cells.iter_mut()) {
        *cell = origin.offset(ox, 0, oz);
    }
    cells
}

pub fn is_valid_ring(world: &World, origin: BlockPos) -> bool {
    ring_at(origin)
        .iter()
        .zip(RING_TEMPLATE.iter())
        .all(|(&p, slot)| {
            let state = world.state_at(p.x, p.y, p.z) as i32;
            is_frame_facing(state, slot.facing)
        })
}

pub fn try_complete(world: &World, q: BlockPos, out: &mut Vec<(BlockPos, i32)>) {
    let mut origins = [ORIGIN_ZERO; RING_SIZE];
    let mut valid = 0usize;
    for slot in RING_TEMPLATE.iter() {
        let origin = q.offset(-slot.dx, 0, -slot.dz);
        if is_valid_ring(world, origin) {
            origins[valid] = origin;
            valid += 1;
        }
    }
    if valid == 0 {
        return;
    }

    let portal = end_portal_state();
    let start = out.len();
    for &origin in &origins[..valid] {
        for cell in interior_cells(origin) {
            if out[start..].iter().any(|&(pos, _)| pos == cell) {
                continue;
            }
            world.set_state(cell.x, cell.y, cell.z, portal as u16);
            out.push((cell, portal));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verdigris_protocol::{block_id_of_name, default_state_of_name, state_with};

    const Y: i32 = -40;

    fn empty_world() -> World {
        World::flat().unwrap()
    }

    fn frame_state(eye: &str, facing: &str) -> u16 {
        let id = block_id_of_name("minecraft:end_portal_frame").unwrap();
        state_with(id, &[("eye", eye), ("facing", facing)]).unwrap() as u16
    }

    fn put(world: &World, p: BlockPos, state: u16) {
        world.set_state(p.x, p.y, p.z, state);
    }

    fn build_ring(world: &World, origin: BlockPos, skip: Option<usize>, wrong: Option<usize>) {
        for (index, slot) in RING_TEMPLATE.iter().enumerate() {
            if Some(index) == skip {
                continue;
            }
            let p = origin.offset(slot.dx, 0, slot.dz);
            let facing = if Some(index) == wrong {
                if slot.facing == "north" {
                    "south"
                } else {
                    "north"
                }
            } else {
                slot.facing
            };
            put(world, p, frame_state("true", facing));
        }
    }

    fn portal_state() -> i32 {
        default_state_of_name("minecraft:end_portal").unwrap()
    }

    fn complete_from(world: &World, q: BlockPos) -> Vec<(BlockPos, i32)> {
        let mut out = Vec::new();
        try_complete(world, q, &mut out);
        out
    }

    #[test]
    fn last_eye_completes_from_each_of_four_sides() {
        let probes = [
            RING_TEMPLATE[0],
            RING_TEMPLATE[3],
            RING_TEMPLATE[6],
            RING_TEMPLATE[9],
        ];
        for (index, slot) in RING_TEMPLATE.iter().enumerate() {
            if !probes.contains(slot) {
                continue;
            }
            let world = empty_world();
            let origin = BlockPos {
                x: 100,
                y: Y,
                z: 100,
            };
            build_ring(&world, origin, Some(index), None);
            let last = origin.offset(slot.dx, 0, slot.dz);
            put(&world, last, frame_state("true", slot.facing));

            let updates = complete_from(&world, last);
            assert_eq!(updates.len(), 9, "neuf cases posees pour le cote {index}");
            let expected = interior_cells(origin);
            for cell in expected {
                assert!(
                    updates
                        .iter()
                        .any(|&(p, s)| p == cell && s == portal_state()),
                    "case interieure {cell:?} posee en end_portal"
                );
                assert_eq!(
                    world.state_at(cell.x, cell.y, cell.z) as i32,
                    portal_state()
                );
            }
        }
    }

    #[test]
    fn misoriented_frame_blocks_completion() {
        let world = empty_world();
        let origin = BlockPos { x: 0, y: Y, z: 0 };
        build_ring(&world, origin, None, Some(5));
        let q = origin.offset(RING_TEMPLATE[0].dx, 0, RING_TEMPLATE[0].dz);
        assert!(complete_from(&world, q).is_empty());
    }

    #[test]
    fn missing_eye_blocks_completion() {
        let world = empty_world();
        let origin = BlockPos { x: 0, y: Y, z: 0 };
        build_ring(&world, origin, None, None);
        let missing = origin.offset(RING_TEMPLATE[7].dx, 0, RING_TEMPLATE[7].dz);
        put(
            &world,
            missing,
            frame_state("false", RING_TEMPLATE[7].facing),
        );
        let q = origin.offset(RING_TEMPLATE[0].dx, 0, RING_TEMPLATE[0].dz);
        assert!(complete_from(&world, q).is_empty());
    }

    #[test]
    fn occupied_corner_does_not_prevent_completion() {
        let world = empty_world();
        let origin = BlockPos { x: 0, y: Y, z: 0 };
        build_ring(&world, origin, None, None);
        let corner = origin.offset(-1, 0, -1);
        put(&world, corner, frame_state("true", "south"));
        let q = origin.offset(RING_TEMPLATE[0].dx, 0, RING_TEMPLATE[0].dz);
        let updates = complete_from(&world, q);
        assert_eq!(updates.len(), 9);
    }

    #[test]
    fn frames_on_two_levels_do_not_complete() {
        let world = empty_world();
        let origin = BlockPos { x: 0, y: Y, z: 0 };
        for (index, slot) in RING_TEMPLATE.iter().enumerate() {
            let mut p = origin.offset(slot.dx, 0, slot.dz);
            if index % 2 == 0 {
                p.y += 1;
            }
            put(&world, p, frame_state("true", slot.facing));
        }
        let q = origin.offset(RING_TEMPLATE[1].dx, 0, RING_TEMPLATE[1].dz);
        assert!(complete_from(&world, q).is_empty());
    }

    #[test]
    fn shared_frame_in_two_rings_materialises_both_interiors_without_duplicates() {
        let world = empty_world();
        let left = BlockPos { x: 0, y: Y, z: 0 };
        let right = BlockPos {
            x: left.x + 1,
            y: Y,
            z: 0,
        };
        build_ring(&world, left, None, None);
        build_ring(&world, right, None, None);

        let shared_candidates: Vec<BlockPos> = ring_at(left)
            .into_iter()
            .filter(|p| ring_at(right).contains(p))
            .collect();
        assert!(
            !shared_candidates.is_empty(),
            "les deux anneaux partagent au moins un cadre"
        );
        let q = shared_candidates[0];

        let updates = complete_from(&world, q);
        let mut keys: Vec<(i32, i32, i32)> =
            updates.iter().map(|&(p, _)| (p.x, p.y, p.z)).collect();
        let total = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), total, "aucune case dupliquee");

        for cell in interior_cells(left) {
            assert!(updates.iter().any(|&(p, _)| p == cell));
        }
        for cell in interior_cells(right) {
            assert!(updates.iter().any(|&(p, _)| p == cell));
        }

        let shared_interiors: Vec<BlockPos> = interior_cells(left)
            .into_iter()
            .filter(|p| interior_cells(right).contains(p))
            .collect();
        for cell in shared_interiors {
            let count = updates.iter().filter(|&&(p, _)| p == cell).count();
            assert_eq!(count, 1, "case partagee {cell:?} posee une seule fois");
        }

        for cell in interior_cells(left)
            .into_iter()
            .chain(interior_cells(right))
        {
            assert_eq!(
                world.state_at(cell.x, cell.y, cell.z) as i32,
                portal_state()
            );
        }
    }
}
