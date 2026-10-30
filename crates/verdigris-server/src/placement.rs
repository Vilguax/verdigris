use verdigris_protocol::{
    BlockPos, Direction, HorizFacingKind, PlacementKind, SixFacingKind, SlabType, WORLD_MAX_Y,
    WORLD_MIN_Y, World, block_has_property, full_block, horiz_facing_kind, horizontal_direction,
    is_air, is_slab_block, is_stair_block, item_to_placed_state, nearest_looking_direction,
    nearest_looking_directions, placement_kind_of_block, placement_kind_of_item, property_of,
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
    PlacePair {
        writes: [(BlockPos, i32); 2],
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

fn cell_state(world: &World, pos: BlockPos) -> Option<u16> {
    if in_world(pos) {
        Some(world.state_at(pos.x, pos.y, pos.z))
    } else {
        None
    }
}

fn is_free(world: &World, pos: BlockPos) -> bool {
    matches!(cell_state(world, pos), Some(s) if is_air(s as i32) || state_in_replaceable(s as i32))
}

fn is_full(world: &World, pos: BlockPos) -> bool {
    cell_state(world, pos).is_some_and(|s| full_block(s as i32))
}

fn ccw(dir: Direction) -> Direction {
    match dir {
        Direction::North => Direction::West,
        Direction::West => Direction::South,
        Direction::South => Direction::East,
        Direction::East => Direction::North,
        other => other,
    }
}

fn step(dir: Direction) -> (i32, i32) {
    let (dx, _, dz) = dir.delta();
    (dx, dz)
}

fn facing_to_direction(value: &str) -> Option<Direction> {
    match value {
        "north" => Some(Direction::North),
        "south" => Some(Direction::South),
        "west" => Some(Direction::West),
        "east" => Some(Direction::East),
        _ => None,
    }
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

    match placement_kind_of_item(item) {
        PlacementKind::Door => return place_door(world, ctx, default, place_pos),
        PlacementKind::Bed => return place_bed(world, ctx, default, place_pos),
        PlacementKind::TallPlant => return place_tall_plant(world, default, place_pos),
        PlacementKind::Simple => {}
    }

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

fn pair_placement(a: (BlockPos, i32), b: (BlockPos, i32)) -> Placement {
    Placement::PlacePair {
        writes: [a, b],
        resync: SmallPositions::pair(a.0, b.0),
    }
}

fn same_door_lower(world: &World, pos: BlockPos, block_id: i32) -> bool {
    matches!(
        cell_state(world, pos),
        Some(s) if state_to_block(s as i32).id == block_id
            && property_of(s as i32, "half") == Some("lower")
    )
}

fn door_hinge(
    world: &World,
    ctx: PlaceContext,
    b: BlockPos,
    up: BlockPos,
    facing: Direction,
    block_id: i32,
) -> &'static str {
    let left = ccw(facing);
    let right = facing.horizontal_cw();
    let i = -i32::from(is_full(world, offset(b, left)))
        - i32::from(is_full(world, offset(up, left)))
        + i32::from(is_full(world, offset(b, right)))
        + i32::from(is_full(world, offset(up, right)));
    let door_g = same_door_lower(world, offset(b, left), block_id);
    let door_d = same_door_lower(world, offset(b, right), block_id);

    if (door_g && !door_d) || i > 0 {
        "right"
    } else if (door_d && !door_g) || i < 0 || hinge_tiebreak(ctx, b, facing) {
        "left"
    } else {
        "right"
    }
}

fn hinge_tiebreak(ctx: PlaceContext, b: BlockPos, facing: Direction) -> bool {
    let (sx, sz) = step(facing);
    let u = (ctx.pos.x as f64 + ctx.cursor.0 as f64) - b.x as f64;
    let w = (ctx.pos.z as f64 + ctx.cursor.2 as f64) - b.z as f64;
    (sx >= 0 || w >= 0.5) && (sx <= 0 || w <= 0.5) && (sz >= 0 || u <= 0.5) && (sz <= 0 || u >= 0.5)
}

fn place_door(world: &World, ctx: PlaceContext, default: i32, b: BlockPos) -> Placement {
    let up = b.offset(0, 1, 0);
    if !is_free(world, b) || !is_free(world, up) {
        return Placement::Nothing;
    }
    let block_id = state_to_block(default).id;
    let facing = horizontal_direction(ctx.yaw);
    let fstr = facing.facing_str();
    let hinge = door_hinge(world, ctx, b, up, facing, block_id);
    let lower = state_with(
        block_id,
        &[
            ("facing", fstr),
            ("half", "lower"),
            ("hinge", hinge),
            ("open", "false"),
            ("powered", "false"),
        ],
    );
    let upper = state_with(
        block_id,
        &[
            ("facing", fstr),
            ("half", "upper"),
            ("hinge", hinge),
            ("open", "false"),
            ("powered", "false"),
        ],
    );
    match (lower, upper) {
        (Some(lower), Some(upper)) => pair_placement((b, lower), (up, upper)),
        _ => Placement::Nothing,
    }
}

fn place_bed(world: &World, ctx: PlaceContext, default: i32, b: BlockPos) -> Placement {
    let facing = horizontal_direction(ctx.yaw);
    let head_pos = offset(b, facing);
    if !is_free(world, b) || !is_free(world, head_pos) {
        return Placement::Nothing;
    }
    let block_id = state_to_block(default).id;
    let fstr = facing.facing_str();
    let foot = state_with(
        block_id,
        &[("facing", fstr), ("occupied", "false"), ("part", "foot")],
    );
    let head = state_with(
        block_id,
        &[("facing", fstr), ("occupied", "false"), ("part", "head")],
    );
    match (foot, head) {
        (Some(foot), Some(head)) => pair_placement((b, foot), (head_pos, head)),
        _ => Placement::Nothing,
    }
}

fn place_tall_plant(world: &World, default: i32, b: BlockPos) -> Placement {
    let up = b.offset(0, 1, 0);
    if !is_free(world, b) || !is_free(world, up) {
        return Placement::Nothing;
    }
    let block_id = state_to_block(default).id;
    let lower = state_with(block_id, &[("half", "lower")]);
    let upper = state_with(block_id, &[("half", "upper")]);
    match (lower, upper) {
        (Some(lower), Some(upper)) => pair_placement((b, lower), (up, upper)),
        _ => Placement::Nothing,
    }
}

pub fn twin_to_break(world: &World, pos: BlockPos, state: u16) -> Option<BlockPos> {
    let s = state as i32;
    let block_id = state_to_block(s).id;
    match placement_kind_of_block(block_id) {
        PlacementKind::Door | PlacementKind::TallPlant => half_twin(world, pos, s, block_id),
        PlacementKind::Bed => bed_twin(world, pos, s, block_id),
        PlacementKind::Simple => None,
    }
}

fn half_twin(world: &World, pos: BlockPos, state: i32, block_id: i32) -> Option<BlockPos> {
    let (twin, expected) = match property_of(state, "half")? {
        "upper" => (pos.offset(0, -1, 0), "lower"),
        "lower" => (pos.offset(0, 1, 0), "upper"),
        _ => return None,
    };
    let twin_state = cell_state(world, twin)? as i32;
    if state_to_block(twin_state).id == block_id
        && property_of(twin_state, "half") == Some(expected)
    {
        Some(twin)
    } else {
        None
    }
}

fn bed_twin(world: &World, pos: BlockPos, state: i32, block_id: i32) -> Option<BlockPos> {
    let part = property_of(state, "part")?;
    let facing = property_of(state, "facing")?;
    let dir = facing_to_direction(facing)?;
    let (dx, _, dz) = dir.delta();
    let (twin, expected) = match part {
        "head" => (pos.offset(-dx, 0, -dz), "foot"),
        "foot" => (pos.offset(dx, 0, dz), "head"),
        _ => return None,
    };
    let twin_state = cell_state(world, twin)? as i32;
    if state_to_block(twin_state).id == block_id
        && property_of(twin_state, "part") == Some(expected)
        && property_of(twin_state, "facing") == Some(facing)
    {
        Some(twin)
    } else {
        None
    }
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

    fn paired(result: Placement) -> ([(BlockPos, i32); 2], SmallPositions) {
        match result {
            Placement::PlacePair { writes, resync } => (writes, resync),
            other => panic!("attendu PlacePair, obtenu {other:?}"),
        }
    }

    fn context_xz(pos: BlockPos, cx: f32, cz: f32, yaw: f32) -> PlaceContext {
        PlaceContext {
            pos,
            face: Direction::Up,
            cursor: (cx, 1.0, cz),
            yaw,
            pitch: 0.0,
        }
    }

    fn block_id(name: &str) -> i32 {
        state_to_block(default_state_of_name(name).unwrap()).id
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

    fn door_on_top(world: &World, yaw: f32) -> ([(BlockPos, i32); 2], SmallPositions) {
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 1.0, yaw, 0.0);
        paired(resolve_placement(world, ctx, item_of("minecraft:oak_door")))
    }

    #[test]
    fn door_orients_to_player_look() {
        for (yaw, facing) in [
            (SOUTH_YAW, "south"),
            (90.0, "west"),
            (180.0, "north"),
            (EAST_YAW, "east"),
        ] {
            let world = World::flat().unwrap();
            let (writes, resync) = door_on_top(&world, yaw);
            let lower = BlockPos { x: 0, y: -60, z: 0 };
            let upper = BlockPos { x: 0, y: -59, z: 0 };
            assert_eq!(writes[0].0, lower);
            assert_eq!(writes[1].0, upper);
            assert_eq!(property_of(writes[0].1, "facing"), Some(facing));
            assert_eq!(property_of(writes[1].1, "facing"), Some(facing));
            assert_eq!(property_of(writes[0].1, "half"), Some("lower"));
            assert_eq!(property_of(writes[1].1, "half"), Some("upper"));
            assert_eq!(property_of(writes[0].1, "open"), Some("false"));
            assert_eq!(property_of(writes[0].1, "powered"), Some("false"));
            assert_eq!(resync.as_slice(), &[lower, upper]);
        }
    }

    #[test]
    fn door_hinge_follows_adjacent_full_blocks() {
        let world = World::flat().unwrap();
        world.set_state(1, -60, 0, 1);
        let (writes, _) = door_on_top(&world, SOUTH_YAW);
        assert_eq!(
            property_of(writes[0].1, "hinge"),
            Some("left"),
            "mur plein a gauche (est) => gond a gauche"
        );

        let world = World::flat().unwrap();
        world.set_state(-1, -60, 0, 1);
        let (writes, _) = door_on_top(&world, SOUTH_YAW);
        assert_eq!(
            property_of(writes[0].1, "hinge"),
            Some("right"),
            "mur plein a droite (ouest) => gond a droite"
        );
    }

    #[test]
    fn door_hinge_follows_neighbour_door() {
        let world = World::flat().unwrap();
        let door_lower = state_with(block_id("minecraft:oak_door"), &[("half", "lower")]).unwrap();
        world.set_state(1, -60, 0, door_lower as u16);
        let (writes, _) = door_on_top(&world, SOUTH_YAW);
        assert_eq!(
            property_of(writes[0].1, "hinge"),
            Some("right"),
            "porte voisine identique a gauche => gond a droite"
        );
    }

    #[test]
    fn door_hinge_breaks_tie_with_click() {
        let p = BlockPos { x: 0, y: -61, z: 0 };

        let world = World::flat().unwrap();
        let ctx = context_xz(p, 0.5, 0.1, EAST_YAW);
        let (writes, _) = paired(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:oak_door"),
        ));
        assert_eq!(property_of(writes[0].1, "hinge"), Some("left"));

        let world = World::flat().unwrap();
        let ctx = context_xz(p, 0.5, 0.9, EAST_YAW);
        let (writes, _) = paired(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:oak_door"),
        ));
        assert_eq!(property_of(writes[0].1, "hinge"), Some("right"));
    }

    #[test]
    fn door_refuses_when_upper_blocked_or_out_of_world() {
        let world = World::flat().unwrap();
        world.set_state(0, -59, 0, 1);
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 1.0, SOUTH_YAW, 0.0);
        assert_eq!(
            resolve_placement(&world, ctx, item_of("minecraft:oak_door")),
            Placement::Nothing
        );

        let world = World::flat().unwrap();
        world.set_state(
            0,
            WORLD_MAX_Y,
            0,
            default_state_of_name("minecraft:short_grass").unwrap() as u16,
        );
        let p = BlockPos {
            x: 0,
            y: WORLD_MAX_Y,
            z: 0,
        };
        let ctx = context(p, Direction::Up, 1.0, SOUTH_YAW, 0.0);
        assert_eq!(
            resolve_placement(&world, ctx, item_of("minecraft:oak_door")),
            Placement::Nothing,
            "2eme case hors monde => refus"
        );
    }

    #[test]
    fn bed_places_in_four_directions() {
        for (yaw, facing, hx, hz) in [
            (SOUTH_YAW, "south", 0, 1),
            (90.0, "west", -1, 0),
            (180.0, "north", 0, -1),
            (EAST_YAW, "east", 1, 0),
        ] {
            let world = World::flat().unwrap();
            let p = BlockPos { x: 0, y: -61, z: 0 };
            let ctx = context(p, Direction::Up, 1.0, yaw, 0.0);
            let (writes, resync) = paired(resolve_placement(
                &world,
                ctx,
                item_of("minecraft:white_bed"),
            ));
            let foot = BlockPos { x: 0, y: -60, z: 0 };
            let head = BlockPos {
                x: hx,
                y: -60,
                z: hz,
            };
            assert_eq!(writes[0].0, foot);
            assert_eq!(writes[1].0, head);
            assert_eq!(property_of(writes[0].1, "part"), Some("foot"));
            assert_eq!(property_of(writes[1].1, "part"), Some("head"));
            assert_eq!(property_of(writes[0].1, "facing"), Some(facing));
            assert_eq!(property_of(writes[1].1, "facing"), Some(facing));
            assert_eq!(property_of(writes[0].1, "occupied"), Some("false"));
            assert_eq!(resync.as_slice(), &[foot, head]);
        }
    }

    #[test]
    fn bed_refuses_when_head_blocked() {
        let world = World::flat().unwrap();
        world.set_state(0, -60, 1, 1);
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 1.0, SOUTH_YAW, 0.0);
        assert_eq!(
            resolve_placement(&world, ctx, item_of("minecraft:white_bed")),
            Placement::Nothing
        );
    }

    #[test]
    fn bed_refuses_when_foot_out_of_world() {
        let world = World::flat().unwrap();
        world.set_state(0, WORLD_MAX_Y, 0, 1);
        let p = BlockPos {
            x: 0,
            y: WORLD_MAX_Y,
            z: 0,
        };
        let ctx = context(p, Direction::Up, 1.0, SOUTH_YAW, 0.0);
        assert_eq!(
            resolve_placement(&world, ctx, item_of("minecraft:white_bed")),
            Placement::Nothing
        );
    }

    #[test]
    fn tall_plant_places_two_vertical_cells() {
        let world = World::flat().unwrap();
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 1.0, SOUTH_YAW, 0.0);
        let (writes, resync) = paired(resolve_placement(
            &world,
            ctx,
            item_of("minecraft:tall_grass"),
        ));
        let lower = BlockPos { x: 0, y: -60, z: 0 };
        let upper = BlockPos { x: 0, y: -59, z: 0 };
        assert_eq!(writes[0].0, lower);
        assert_eq!(writes[1].0, upper);
        assert_eq!(property_of(writes[0].1, "half"), Some("lower"));
        assert_eq!(property_of(writes[1].1, "half"), Some("upper"));
        assert_eq!(resync.as_slice(), &[lower, upper]);
    }

    #[test]
    fn tall_plant_refuses_when_base_occupied() {
        let world = World::flat().unwrap();
        world.set_state(0, -60, 0, 1);
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 1.0, SOUTH_YAW, 0.0);
        assert_eq!(
            resolve_placement(&world, ctx, item_of("minecraft:tall_grass")),
            Placement::Nothing,
            "case de pose occupee => refus"
        );
    }

    #[test]
    fn tall_plant_refuses_when_top_occupied() {
        let world = World::flat().unwrap();
        world.set_state(0, -59, 0, 1);
        let p = BlockPos { x: 0, y: -61, z: 0 };
        let ctx = context(p, Direction::Up, 1.0, SOUTH_YAW, 0.0);
        assert_eq!(
            resolve_placement(&world, ctx, item_of("minecraft:tall_grass")),
            Placement::Nothing
        );
    }

    #[test]
    fn tall_plant_refuses_at_top_of_world() {
        let world = World::flat().unwrap();
        world.set_state(
            0,
            WORLD_MAX_Y,
            0,
            default_state_of_name("minecraft:short_grass").unwrap() as u16,
        );
        let p = BlockPos {
            x: 0,
            y: WORLD_MAX_Y,
            z: 0,
        };
        let ctx = context(p, Direction::Up, 1.0, SOUTH_YAW, 0.0);
        assert_eq!(
            resolve_placement(&world, ctx, item_of("minecraft:tall_grass")),
            Placement::Nothing
        );
    }

    #[test]
    fn twin_detection_matches_and_rejects() {
        let world = World::flat().unwrap();
        let door = block_id("minecraft:oak_door");
        let lower = state_with(door, &[("half", "lower")]).unwrap();
        let upper = state_with(door, &[("half", "upper")]).unwrap();
        world.set_state(0, -60, 0, lower as u16);
        world.set_state(0, -59, 0, upper as u16);
        let lower_pos = BlockPos { x: 0, y: -60, z: 0 };
        let upper_pos = BlockPos { x: 0, y: -59, z: 0 };
        assert_eq!(
            twin_to_break(&world, lower_pos, lower as u16),
            Some(upper_pos)
        );
        assert_eq!(
            twin_to_break(&world, upper_pos, upper as u16),
            Some(lower_pos)
        );

        world.set_state(0, -59, 0, lower as u16);
        assert_eq!(
            twin_to_break(&world, lower_pos, lower as u16),
            None,
            "moitie superieure incoherente => pas de jumeau"
        );

        assert_eq!(twin_to_break(&world, lower_pos, 1), None, "bloc simple");
    }

    #[test]
    fn bed_twin_requires_matching_facing_and_part() {
        let world = World::flat().unwrap();
        let bed = block_id("minecraft:white_bed");
        let foot = state_with(bed, &[("facing", "south"), ("part", "foot")]).unwrap();
        let head = state_with(bed, &[("facing", "south"), ("part", "head")]).unwrap();
        world.set_state(0, -60, 0, foot as u16);
        world.set_state(0, -60, 1, head as u16);
        let foot_pos = BlockPos { x: 0, y: -60, z: 0 };
        let head_pos = BlockPos { x: 0, y: -60, z: 1 };
        assert_eq!(twin_to_break(&world, foot_pos, foot as u16), Some(head_pos));
        assert_eq!(twin_to_break(&world, head_pos, head as u16), Some(foot_pos));

        let wrong = state_with(bed, &[("facing", "north"), ("part", "head")]).unwrap();
        world.set_state(0, -60, 1, wrong as u16);
        assert_eq!(
            twin_to_break(&world, foot_pos, foot as u16),
            None,
            "orientation differente => pas de jumeau"
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
