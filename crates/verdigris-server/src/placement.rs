use verdigris_protocol::{
    BlockPos, Direction, HorizFacingKind, SixFacingKind, SlabType, WORLD_MAX_Y, WORLD_MIN_Y, World,
    block_has_property, full_block, horiz_facing_kind, horizontal_direction, is_air, is_slab_block,
    is_stair_block, item_to_placed_state, nearest_looking_direction, nearest_looking_directions,
    six_facing_kind, slab_block_of_item, slab_type, state_in_replaceable, state_to_block,
    state_with, water_source_state, with_property,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaceContext {
    pub pos: BlockPos,
    pub face: Direction,
    pub cursor: (f32, f32, f32),
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmallPositions {
    items: [BlockPos; 2],
    len: usize,
}

impl SmallPositions {
    pub fn one(a: BlockPos) -> Self {
        SmallPositions {
            items: [a, a],
            len: 1,
        }
    }

    pub fn pair(a: BlockPos, b: BlockPos) -> Self {
        if a == b {
            SmallPositions::one(a)
        } else {
            SmallPositions {
                items: [a, b],
                len: 2,
            }
        }
    }

    pub fn as_slice(&self) -> &[BlockPos] {
        &self.items[..self.len]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Place {
        write: BlockPos,
        state: i32,
        resync: SmallPositions,
    },
    Fuse {
        write: BlockPos,
        state: i32,
        resync: SmallPositions,
    },
    Nothing,
}

pub fn offset(pos: BlockPos, dir: Direction) -> BlockPos {
    let (dx, dy, dz) = dir.delta();
    pos.offset(dx, dy, dz)
}

fn in_world(pos: BlockPos) -> bool {
    (WORLD_MIN_Y..=WORLD_MAX_Y).contains(&pos.y)
}

fn apply(state: i32, name: &str, value: &str) -> i32 {
    with_property(state, name, value).unwrap_or(state)
}

fn double_slab(block_id: i32) -> Option<i32> {
    state_with(block_id, &[("type", "double"), ("waterlogged", "false")])
}

fn state_at(world: &World, pos: BlockPos) -> i32 {
    world.state_at(pos.x, pos.y, pos.z) as i32
}

pub fn resolve_placement(world: &World, ctx: PlaceContext, item: i32) -> Placement {
    let Some(default) = item_to_placed_state(item) else {
        return Placement::Nothing;
    };

    let p = ctx.pos;
    let state_p = state_at(world, p);

    if let Some(slab_id) = slab_block_of_item(item) {
        if state_to_block(state_p).id == slab_id
            && let Some(kind) = slab_type(state_p)
        {
            let horizontal = ctx.face.is_horizontal();
            let fuse_here = match kind {
                SlabType::Bottom => ctx.face == Direction::Up || (horizontal && ctx.cursor.1 > 0.5),
                SlabType::Top => ctx.face == Direction::Down || (horizontal && ctx.cursor.1 <= 0.5),
                SlabType::Double => false,
            };
            if fuse_here
                && in_world(p)
                && let Some(state) = double_slab(slab_id)
            {
                return Placement::Fuse {
                    write: p,
                    state,
                    resync: SmallPositions::one(p),
                };
            }
        }

        let pf = offset(p, ctx.face);
        let state_pf = state_at(world, pf);
        if state_to_block(state_pf).id == slab_id
            && matches!(slab_type(state_pf), Some(SlabType::Top | SlabType::Bottom))
            && in_world(pf)
            && let Some(state) = double_slab(slab_id)
        {
            return Placement::Fuse {
                write: pf,
                state,
                resync: SmallPositions::pair(p, pf),
            };
        }
    }

    let place_pos = if state_in_replaceable(state_p) {
        p
    } else {
        offset(p, ctx.face)
    };
    if !in_world(place_pos) {
        return Placement::Nothing;
    }
    let dest = state_at(world, place_pos);
    if !(is_air(dest) || state_in_replaceable(dest)) {
        return Placement::Nothing;
    }
    let dest_was_water = dest == water_source_state();
    let resync = SmallPositions::pair(p, place_pos);

    let Some(state) = placed_state(world, ctx, default, place_pos, dest_was_water) else {
        return Placement::Nothing;
    };

    Placement::Place {
        write: place_pos,
        state,
        resync,
    }
}

fn placed_state(
    world: &World,
    ctx: PlaceContext,
    default: i32,
    place_pos: BlockPos,
    dest_was_water: bool,
) -> Option<i32> {
    let block = state_to_block(default);
    let bid = block.id;
    let face = ctx.face;
    let cy = ctx.cursor.1;
    let lower = face != Direction::Down && (face == Direction::Up || cy <= 0.5);

    let mut state = default;

    if is_stair_block(bid) {
        state = apply(state, "facing", horizontal_direction(ctx.yaw).facing_str());
        state = apply(state, "half", if lower { "bottom" } else { "top" });
        state = apply(state, "shape", "straight");
    } else if is_slab_block(bid) {
        state = apply(state, "type", if lower { "bottom" } else { "top" });
    } else if block_has_property(bid, "axis") {
        state = apply(state, "axis", face.axis());
    } else if let Some(kind) = six_facing_kind(bid) {
        match kind {
            SixFacingKind::OppositeLook => {
                let dir = nearest_looking_direction(ctx.yaw, ctx.pitch).opposite();
                state = apply(state, "facing", dir.facing_str());
            }
            SixFacingKind::Look => {
                let dir = nearest_looking_direction(ctx.yaw, ctx.pitch);
                state = apply(state, "facing", dir.facing_str());
            }
            SixFacingKind::Face => {
                state = apply(state, "facing", face.facing_str());
            }
            SixFacingKind::Default => {}
        }
    } else if block_has_property(bid, "face") {
        state = place_attachable(world, state, place_pos, ctx.yaw, ctx.pitch)?;
    } else if let Some(kind) = horiz_facing_kind(bid) {
        let dir = match kind {
            HorizFacingKind::FenceGate => horizontal_direction(ctx.yaw),
            HorizFacingKind::Anvil => horizontal_direction(ctx.yaw).horizontal_cw(),
            HorizFacingKind::OppositeHoriz => horizontal_direction(ctx.yaw).opposite(),
        };
        state = apply(state, "facing", dir.facing_str());
    }

    if block_has_property(bid, "waterlogged") {
        state = apply(
            state,
            "waterlogged",
            if dest_was_water { "true" } else { "false" },
        );
    }

    Some(state)
}

fn place_attachable(
    world: &World,
    base: i32,
    place_pos: BlockPos,
    yaw: f32,
    pitch: f32,
) -> Option<i32> {
    for dir in nearest_looking_directions(yaw, pitch) {
        let (face_value, facing, support) = match dir {
            Direction::Up => (
                "ceiling",
                horizontal_direction(yaw),
                offset(place_pos, Direction::Up),
            ),
            Direction::Down => (
                "floor",
                horizontal_direction(yaw),
                offset(place_pos, Direction::Down),
            ),
            other => ("wall", other.opposite(), offset(place_pos, other)),
        };
        if full_block(state_at(world, support)) {
            let mut state = apply(base, "face", face_value);
            state = apply(state, "facing", facing.facing_str());
            return Some(state);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use verdigris_protocol::{block_item_of_state, default_state_of_name, property_of};

    fn item_of(name: &str) -> i32 {
        block_item_of_state(default_state_of_name(name).unwrap()).unwrap()
    }

    fn slab_state(name: &str, kind: &str) -> u16 {
        let id = state_to_block(default_state_of_name(name).unwrap()).id;
        state_with(id, &[("type", kind)]).unwrap() as u16
    }

    fn context(pos: BlockPos, face: Direction, cy: f32, yaw: f32, pitch: f32) -> PlaceContext {
        PlaceContext {
            pos,
            face,
            cursor: (0.5, cy, 0.5),
            yaw,
            pitch,
        }
    }

    fn placed(result: Placement) -> (BlockPos, i32, SmallPositions) {
        match result {
            Placement::Place {
                write,
                state,
                resync,
            } => (write, state, resync),
            other => panic!("attendu Place, obtenu {other:?}"),
        }
    }

    fn fused(result: Placement) -> (BlockPos, i32, SmallPositions) {
        match result {
            Placement::Fuse {
                write,
                state,
                resync,
            } => (write, state, resync),
            other => panic!("attendu Fuse, obtenu {other:?}"),
        }
    }

    const EAST_YAW: f32 = -90.0;
    const SOUTH_YAW: f32 = 0.0;

    #[test]
    fn stairs_orient_and_half() {
        let world = World::flat().unwrap();
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 0.2, EAST_YAW, 0.0);
        let (write, state, _) = placed(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:oak_stairs"),
        ));
        assert_eq!(write, BlockPos { x: 0, y: -60, z: 0 });
        assert_eq!(property_of(state, "facing"), Some("east"));
        assert_eq!(property_of(state, "half"), Some("bottom"));
        assert_eq!(property_of(state, "shape"), Some("straight"));

        world.set_state(0, -60, 0, 1);
        let p = BlockPos { x: 0, y: -60, z: 0 };
        let ctx = context(p, Direction::North, 0.7, EAST_YAW, 0.0);
        let (_, state, _) = placed(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:oak_stairs"),
        ));
        assert_eq!(property_of(state, "half"), Some("top"));
    }

    #[test]
    fn slabs_low_and_high() {
        let world = World::flat().unwrap();
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 0.2, SOUTH_YAW, 0.0);
        let (_, state, _) = placed(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:oak_slab"),
        ));
        assert_eq!(property_of(state, "type"), Some("bottom"));

        world.set_state(0, -58, 0, 1);
        let p = BlockPos { x: 0, y: -58, z: 0 };
        let ctx = context(p, Direction::Down, 0.5, SOUTH_YAW, 0.0);
        let (write, state, _) = placed(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:oak_slab"),
        ));
        assert_eq!(write, BlockPos { x: 0, y: -59, z: 0 });
        assert_eq!(property_of(state, "type"), Some("top"));
    }

    #[test]
    fn slab_fuses_in_place_over_and_under() {
        let world = World::flat().unwrap();
        world.set_state(0, -60, 0, slab_state("minecraft:oak_slab", "bottom"));
        let p = BlockPos { x: 0, y: -60, z: 0 };
        let ctx = context(p, Direction::Up, 0.2, SOUTH_YAW, 0.0);
        let (write, state, resync) = fused(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:oak_slab"),
        ));
        assert_eq!(write, p);
        assert_eq!(property_of(state, "type"), Some("double"));
        assert_eq!(property_of(state, "waterlogged"), Some("false"));
        assert_eq!(resync.as_slice(), &[p]);

        world.set_state(0, -50, 0, slab_state("minecraft:oak_slab", "top"));
        let p = BlockPos { x: 0, y: -50, z: 0 };
        let ctx = context(p, Direction::Down, 0.5, SOUTH_YAW, 0.0);
        let (write, state, resync) = fused(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:oak_slab"),
        ));
        assert_eq!(write, p);
        assert_eq!(property_of(state, "type"), Some("double"));
        assert_eq!(resync.as_slice(), &[p]);
    }

    #[test]
    fn slab_fuses_on_face_neighbour() {
        let world = World::flat().unwrap();
        world.set_state(0, -60, 0, 1);
        world.set_state(0, -59, 0, slab_state("minecraft:oak_slab", "bottom"));
        let p = BlockPos { x: 0, y: -60, z: 0 };
        let pf = BlockPos { x: 0, y: -59, z: 0 };
        let ctx = context(p, Direction::Up, 0.9, SOUTH_YAW, 0.0);
        let (write, state, resync) = fused(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:oak_slab"),
        ));
        assert_eq!(write, pf);
        assert_eq!(property_of(state, "type"), Some("double"));
        assert_eq!(resync.as_slice(), &[p, pf]);
    }

    #[test]
    fn log_axis_follows_face() {
        let world = World::flat().unwrap();
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 0.5, SOUTH_YAW, 0.0);
        let (_, state, _) = placed(resolve_placement(&world, ctx, item_of("minecraft:oak_log")));
        assert_eq!(property_of(state, "axis"), Some("y"));

        world.set_state(0, -60, 0, 1);
        let p = BlockPos { x: 0, y: -60, z: 0 };
        let ctx = context(p, Direction::East, 0.5, SOUTH_YAW, 0.0);
        let (_, state, _) = placed(resolve_placement(&world, ctx, item_of("minecraft:oak_log")));
        assert_eq!(property_of(state, "axis"), Some("x"));

        let ctx = context(p, Direction::North, 0.5, SOUTH_YAW, 0.0);
        let (_, state, _) = placed(resolve_placement(&world, ctx, item_of("minecraft:oak_log")));
        assert_eq!(property_of(state, "axis"), Some("z"));
    }

    #[test]
    fn piston_opposes_look_observer_follows_look() {
        let world = World::flat().unwrap();
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 0.5, SOUTH_YAW, 0.0);
        let (_, state, _) = placed(resolve_placement(&world, ctx, item_of("minecraft:piston")));
        assert_eq!(property_of(state, "facing"), Some("north"));

        let (_, state, _) = placed(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:observer"),
        ));
        assert_eq!(property_of(state, "facing"), Some("south"));
    }

    #[test]
    fn lever_floor_ceiling_wall_and_missing_support() {
        let world = World::flat().unwrap();
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let floor = context(p, Direction::Up, 0.5, SOUTH_YAW, 90.0);
        let (_, state, _) = placed(resolve_placement(&world, floor, item_of("minecraft:lever")));
        assert_eq!(property_of(state, "face"), Some("floor"));

        world.set_state(0, -59, 0, 1);
        let ceiling = context(p, Direction::Up, 0.5, SOUTH_YAW, -90.0);
        let (_, state, _) = placed(resolve_placement(
            &world,
            ceiling,
            item_of("minecraft:lever"),
        ));
        assert_eq!(property_of(state, "face"), Some("ceiling"));

        world.set_state(0, -60, 1, 1);
        let wall = context(p, Direction::Up, 0.5, SOUTH_YAW, 0.0);
        let (_, state, _) = placed(resolve_placement(&world, wall, item_of("minecraft:lever")));
        assert_eq!(property_of(state, "face"), Some("wall"));
        assert_eq!(property_of(state, "facing"), Some("north"));

        world.set_state(5, -55, 5, slab_state("minecraft:oak_slab", "bottom"));
        let isolated = BlockPos { x: 5, y: -55, z: 5 };
        let no_support = context(isolated, Direction::Up, 0.5, SOUTH_YAW, 90.0);
        assert_eq!(
            resolve_placement(&world, no_support, item_of("minecraft:lever")),
            Placement::Nothing
        );
    }

    #[test]
    fn fence_gate_follows_horizontal() {
        let world = World::flat().unwrap();
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 0.5, EAST_YAW, 0.0);
        let (_, state, _) = placed(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:oak_fence_gate"),
        ));
        assert_eq!(property_of(state, "facing"), Some("east"));
    }

    #[test]
    fn anvil_turns_clockwise() {
        let world = World::flat().unwrap();
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 0.5, SOUTH_YAW, 0.0);
        let (_, state, _) = placed(resolve_placement(&world, ctx, item_of("minecraft:anvil")));
        assert_eq!(property_of(state, "facing"), Some("west"));
    }

    #[test]
    fn furnace_opposes_horizontal() {
        let world = World::flat().unwrap();
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 0.5, SOUTH_YAW, 0.0);
        let (_, state, _) = placed(resolve_placement(&world, ctx, item_of("minecraft:furnace")));
        assert_eq!(property_of(state, "facing"), Some("north"));
    }

    #[test]
    fn end_portal_frame_opposes_horizontal() {
        let world = World::flat().unwrap();
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 0.5, SOUTH_YAW, 0.0);
        let (_, state, _) = placed(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:end_portal_frame"),
        ));
        assert_eq!(property_of(state, "facing"), Some("north"));
    }

    #[test]
    fn waterlogged_tracks_destination() {
        let world = World::flat().unwrap();
        world.set_state(0, -60, 0, 1);
        world.set_state(0, -59, 0, water_source_state() as u16);
        let p = BlockPos { x: 0, y: -60, z: 0 };
        let ctx = context(p, Direction::Up, 0.2, EAST_YAW, 0.0);
        let (write, state, _) = placed(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:oak_stairs"),
        ));
        assert_eq!(write, BlockPos { x: 0, y: -59, z: 0 });
        assert_eq!(property_of(state, "waterlogged"), Some("true"));

        let p = BlockPos {
            x: 10,
            y: -61,
            z: 0,
        };
        let ctx = context(p, Direction::Up, 0.2, EAST_YAW, 0.0);
        let (_, state, _) = placed(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:oak_stairs"),
        ));
        assert_eq!(property_of(state, "waterlogged"), Some("false"));
    }

    #[test]
    fn replaceable_target_is_overwritten_in_place() {
        let world = World::flat().unwrap();
        world.set_state(
            0,
            -55,
            0,
            default_state_of_name("minecraft:short_grass").unwrap() as u16,
        );
        let p = BlockPos { x: 0, y: -55, z: 0 };
        let ctx = context(p, Direction::Up, 0.5, SOUTH_YAW, 0.0);
        let (write, _, resync) = placed(resolve_placement(&world, ctx, item_of("minecraft:dirt")));
        assert_eq!(write, p);
        assert_eq!(resync.as_slice(), &[p]);
    }

    #[test]
    fn occupied_target_refuses() {
        let world = World::flat().unwrap();
        let p = BlockPos { x: 0, y: -62, z: 0 };
        let ctx = context(p, Direction::Up, 0.5, SOUTH_YAW, 0.0);
        assert_eq!(
            resolve_placement(&world, ctx, item_of("minecraft:dirt")),
            Placement::Nothing
        );
    }

    #[test]
    fn out_of_world_write_is_refused() {
        let world = World::flat().unwrap();
        world.set_state(0, WORLD_MAX_Y, 0, 1);
        let p = BlockPos {
            x: 0,
            y: WORLD_MAX_Y,
            z: 0,
        };
        let ctx = context(p, Direction::Up, 0.5, SOUTH_YAW, 0.0);
        assert_eq!(
            resolve_placement(&world, ctx, item_of("minecraft:dirt")),
            Placement::Nothing
        );
        assert_eq!(
            world.state_at(0, WORLD_MAX_Y, 0),
            1,
            "aucune ecriture au-dessus du plafond"
        );
        assert_eq!(
            world.state_at(0, WORLD_MAX_Y + 1, 0),
            0,
            "lecture hors monde sans panique"
        );

        let p = BlockPos {
            x: 0,
            y: WORLD_MIN_Y,
            z: 0,
        };
        let ctx = context(p, Direction::Down, 0.5, SOUTH_YAW, 0.0);
        assert_eq!(
            resolve_placement(&world, ctx, item_of("minecraft:dirt")),
            Placement::Nothing
        );
    }

    #[test]
    fn slab_fusion_at_ceiling_boundary_still_writes() {
        let world = World::flat().unwrap();
        world.set_state(
            0,
            WORLD_MAX_Y,
            0,
            slab_state("minecraft:oak_slab", "bottom"),
        );
        let p = BlockPos {
            x: 0,
            y: WORLD_MAX_Y,
            z: 0,
        };
        let ctx = context(p, Direction::Up, 0.2, SOUTH_YAW, 0.0);
        let (write, state, _) = fused(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:oak_slab"),
        ));
        assert_eq!(
            write, p,
            "P au plafond reste dans le monde, fusion autorisee"
        );
        assert_eq!(property_of(state, "type"), Some("double"));
    }
}
