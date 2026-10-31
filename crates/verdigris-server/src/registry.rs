use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::UnboundedSender;
use verdigris_protocol::{
    EntityDataValue, EntitySnapshot, HumanoidArm, ItemStack, ListedPlayer, Pose, TrackedState,
    chunk_coord, frame_entity_pass, set_entity_data_payload,
};

#[derive(Debug, Clone)]
pub enum Outbound {
    Frame(Arc<[u8]>),
    PlayerChat(Arc<[u8]>),
}

pub type OutboundSender = UnboundedSender<Outbound>;

pub const TRACK_RANGE_BLOCKS: f64 = 128.0;
const SPECTATOR_GAME_MODE: i32 = 3;
const SWING_DURATION_TICKS: u64 = 6;

pub fn accepts_swing(last_start: Option<u64>, now: u64, duration: u64) -> bool {
    match last_start {
        None => true,
        Some(start) => {
            let elapsed = now.saturating_sub(start);
            elapsed == 0 || elapsed > duration / 2
        }
    }
}

const META_FLAGS: u8 = 0;
const META_POSE: u8 = 6;
const META_HEALTH: u8 = 9;
const META_ARM: u8 = 15;
const META_SKIN: u8 = 16;
const FULL_HEALTH: f32 = 20.0;
const CROUCH_FLAG: u8 = 0x02;
const SPRINT_FLAG: u8 = 0x08;

#[derive(Debug, Clone, Copy)]
struct PlayerMeta {
    flags: u8,
    pose: Pose,
    arm: HumanoidArm,
    skin: u8,
    dirty: u32,
}

impl PlayerMeta {
    fn new() -> Self {
        PlayerMeta {
            flags: 0,
            pose: Pose::Standing,
            arm: HumanoidArm::Right,
            skin: 0,
            dirty: 0,
        }
    }

    fn non_default_entries(&self, out: &mut Vec<(u8, EntityDataValue)>) {
        out.clear();
        if self.flags != 0 {
            out.push((META_FLAGS, EntityDataValue::Byte(self.flags as i8)));
        }
        if self.pose != Pose::Standing {
            out.push((META_POSE, EntityDataValue::Pose(self.pose)));
        }
        out.push((META_HEALTH, EntityDataValue::Float(FULL_HEALTH)));
        if self.arm != HumanoidArm::Right {
            out.push((META_ARM, EntityDataValue::HumanoidArm(self.arm)));
        }
        out.push((META_SKIN, EntityDataValue::Byte(self.skin as i8)));
    }

    fn dirty_entries(&self, out: &mut Vec<(u8, EntityDataValue)>) {
        out.clear();
        if self.dirty & (1 << META_FLAGS) != 0 {
            out.push((META_FLAGS, EntityDataValue::Byte(self.flags as i8)));
        }
        if self.dirty & (1 << META_POSE) != 0 {
            out.push((META_POSE, EntityDataValue::Pose(self.pose)));
        }
        if self.dirty & (1 << META_ARM) != 0 {
            out.push((META_ARM, EntityDataValue::HumanoidArm(self.arm)));
        }
        if self.dirty & (1 << META_SKIN) != 0 {
            out.push((META_SKIN, EntityDataValue::Byte(self.skin as i8)));
        }
    }
}

static NEXT_ENTITY_ID: AtomicI32 = AtomicI32::new(1);

pub fn allocate_entity_id() -> i32 {
    NEXT_ENTITY_ID.fetch_add(1, Ordering::Relaxed)
}

struct Slot {
    name: String,
    game_mode: i32,
    latency_ms: i32,
    entity_id: i32,
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
    sneaking: bool,
    on_ground: bool,
    ready: bool,
    tracked: TrackedState,
    sent_chunks: HashSet<(i32, i32)>,
    observers: HashSet<i32>,
    meta: PlayerMeta,
    mainhand: ItemStack,
    last_swing_start_tick: Option<u64>,
    permission_level: u8,
    sender: OutboundSender,
}

impl Slot {
    fn snapshot(&self) -> EntitySnapshot {
        EntitySnapshot {
            x: self.x,
            y: self.y,
            z: self.z,
            yaw: self.yaw,
            pitch: self.pitch,
            head_yaw: self.yaw,
            on_ground: self.on_ground,
        }
    }
}

pub struct RegisterSnapshot {
    pub existing: Vec<ListedPlayer>,
    pub other_senders: Vec<OutboundSender>,
}

#[derive(Clone)]
pub struct PlayerHandle {
    pub uuid: u128,
    pub name: String,
    pub x: f64,
    pub z: f64,
    pub sender: OutboundSender,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntitySpawn {
    pub entity_id: i32,
    pub uuid: u128,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw_byte: u8,
    pub pitch_byte: u8,
    pub meta_payload: Vec<u8>,
    pub mainhand: ItemStack,
}

pub struct ObserverActions {
    pub sender: OutboundSender,
    pub removed: Vec<i32>,
    pub entered: Vec<EntitySpawn>,
    pub moved: Vec<Arc<[u8]>>,
}

pub struct MetaUpdate {
    pub payload: Vec<u8>,
    pub targets: Vec<OutboundSender>,
}

pub struct TickOutput {
    pub actions: Vec<ObserverActions>,
    pub meta: Vec<MetaUpdate>,
}

pub struct Departure {
    pub entity_id: i32,
    pub observers: Vec<OutboundSender>,
    pub remaining: Vec<OutboundSender>,
}

pub struct GamemodeChange {
    pub senders: Vec<OutboundSender>,
    pub actions: Vec<ObserverActions>,
}

struct ReadyInfo {
    uuid: u128,
    entity_id: i32,
    x: f64,
    z: f64,
    game_mode: i32,
}

fn is_spectator(game_mode: i32) -> bool {
    game_mode == SPECTATOR_GAME_MODE
}

#[inline]
pub fn broadcast_to(viewer_spectator: bool, target_spectator: bool) -> bool {
    if viewer_spectator {
        true
    } else {
        !target_spectator
    }
}

fn within_range(a: &ReadyInfo, b: &ReadyInfo) -> bool {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    dx * dx + dz * dz <= TRACK_RANGE_BLOCKS * TRACK_RANGE_BLOCKS
}

fn voit(viewer: &ReadyInfo, viewer_sent: &HashSet<(i32, i32)>, target: &ReadyInfo) -> bool {
    within_range(viewer, target)
        && viewer_sent.contains(&(chunk_coord(target.x), chunk_coord(target.z)))
        && broadcast_to(
            is_spectator(viewer.game_mode),
            is_spectator(target.game_mode),
        )
}

fn entity_spawn_of(
    slot: &Slot,
    uuid: u128,
    scratch: &mut Vec<(u8, EntityDataValue)>,
) -> EntitySpawn {
    let (x, y, z) = slot.tracked.base_position();
    slot.meta.non_default_entries(scratch);
    let meta_payload = set_entity_data_payload(slot.entity_id, scratch);
    EntitySpawn {
        entity_id: slot.entity_id,
        uuid,
        x,
        y,
        z,
        yaw_byte: slot.tracked.base_yaw_byte(),
        pitch_byte: slot.tracked.base_pitch_byte(),
        meta_payload,
        mainhand: slot.mainhand,
    }
}

pub struct PlayerRegistry {
    slots: Mutex<HashMap<u128, Slot>>,
    motd: String,
    max_players: i32,
    ops: Vec<String>,
    default_permission: u8,
    tick_counter: AtomicU64,
}

impl PlayerRegistry {
    pub fn new(motd: &str, max_players: i32) -> Self {
        let ops = std::env::var(crate::player::OPS_ENV)
            .ok()
            .map(|raw| {
                raw.split(',')
                    .map(|entry| entry.trim().to_owned())
                    .filter(|entry| !entry.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let default_permission = crate::player::resolve_permission(
            "",
            None,
            std::env::var(crate::player::DEFAULT_PERMISSION_ENV)
                .ok()
                .as_deref(),
        );
        PlayerRegistry::with_permissions(motd, max_players, ops, default_permission)
    }

    pub fn with_permissions(
        motd: &str,
        max_players: i32,
        ops: Vec<String>,
        default_permission: u8,
    ) -> Self {
        PlayerRegistry {
            slots: Mutex::new(HashMap::new()),
            motd: motd.to_owned(),
            max_players,
            ops,
            default_permission,
            tick_counter: AtomicU64::new(0),
        }
    }

    pub fn permission_for(&self, name: &str) -> u8 {
        if self
            .ops
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(name))
        {
            crate::player::OP_PERMISSION_LEVEL
        } else {
            self.default_permission
        }
    }

    pub fn current_tick(&self) -> u64 {
        self.tick_counter.load(Ordering::Relaxed)
    }

    pub fn player_names(&self) -> Vec<String> {
        let slots = self.slots.lock().unwrap();
        let mut names: Vec<String> = slots.values().map(|slot| slot.name.clone()).collect();
        names.sort();
        names
    }

    pub fn try_swing(&self, uuid: u128) -> Option<(i32, Vec<OutboundSender>)> {
        let now = self.tick_counter.load(Ordering::Relaxed);
        let mut slots = self.slots.lock().unwrap();
        let slot = slots.get(&uuid)?;
        if !accepts_swing(slot.last_swing_start_tick, now, SWING_DURATION_TICKS) {
            return None;
        }
        let entity_id = slot.entity_id;
        let observers = slot.observers.clone();
        slots.get_mut(&uuid).unwrap().last_swing_start_tick = Some(now);
        let senders = slots
            .values()
            .filter(|slot| observers.contains(&slot.entity_id))
            .map(|slot| slot.sender.clone())
            .collect();
        Some((entity_id, senders))
    }

    pub fn update_mainhand(
        &self,
        uuid: u128,
        item: ItemStack,
    ) -> Option<(i32, Vec<OutboundSender>)> {
        let mut slots = self.slots.lock().unwrap();
        let slot = slots.get(&uuid)?;
        if slot.mainhand == item {
            return None;
        }
        let entity_id = slot.entity_id;
        let observers = slot.observers.clone();
        slots.get_mut(&uuid).unwrap().mainhand = item;
        let senders = slots
            .values()
            .filter(|slot| observers.contains(&slot.entity_id))
            .map(|slot| slot.sender.clone())
            .collect();
        Some((entity_id, senders))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn register(
        &self,
        uuid: u128,
        entity_id: i32,
        spawn: (f64, f64, f64),
        name: String,
        game_mode: i32,
        latency_ms: i32,
        sender: OutboundSender,
    ) -> RegisterSnapshot {
        let mut slots = self.slots.lock().unwrap();
        let existing = slots
            .iter()
            .map(|(&uuid, slot)| ListedPlayer {
                uuid,
                name: slot.name.clone(),
                game_mode: slot.game_mode,
                latency_ms: slot.latency_ms,
                listed: true,
            })
            .collect();
        let other_senders = slots.values().map(|slot| slot.sender.clone()).collect();
        let snapshot = EntitySnapshot {
            x: spawn.0,
            y: spawn.1,
            z: spawn.2,
            yaw: 0.0,
            pitch: 0.0,
            head_yaw: 0.0,
            on_ground: true,
        };
        slots.insert(
            uuid,
            Slot {
                name,
                game_mode,
                latency_ms,
                entity_id,
                x: spawn.0,
                y: spawn.1,
                z: spawn.2,
                yaw: 0.0,
                pitch: 0.0,
                sneaking: false,
                on_ground: true,
                ready: false,
                tracked: TrackedState::new(entity_id, &snapshot),
                sent_chunks: HashSet::new(),
                observers: HashSet::new(),
                meta: PlayerMeta::new(),
                mainhand: ItemStack::Empty,
                last_swing_start_tick: None,
                permission_level: 0,
                sender,
            },
        );
        RegisterSnapshot {
            existing,
            other_senders,
        }
    }

    pub fn activate(&self, uuid: u128) -> bool {
        let mut slots = self.slots.lock().unwrap();
        match slots.get_mut(&uuid) {
            Some(slot) => {
                slot.ready = true;
                true
            }
            None => false,
        }
    }

    pub fn depart(&self, uuid: u128) -> Departure {
        let mut slots = self.slots.lock().unwrap();
        let (entity_id, observer_ids) = match slots.get(&uuid) {
            Some(slot) => (slot.entity_id, slot.observers.clone()),
            None => (0, HashSet::new()),
        };
        slots.remove(&uuid);
        let observers = slots
            .values()
            .filter(|slot| observer_ids.contains(&slot.entity_id))
            .map(|slot| slot.sender.clone())
            .collect();
        for slot in slots.values_mut() {
            slot.observers.remove(&entity_id);
        }
        let remaining = slots.values().map(|slot| slot.sender.clone()).collect();
        Departure {
            entity_id,
            observers,
            remaining,
        }
    }

    pub fn set_latency(&self, uuid: u128, latency_ms: i32) {
        let mut slots = self.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(&uuid) {
            slot.latency_ms = latency_ms;
        }
    }

    pub fn set_position(&self, uuid: u128, x: f64, y: f64, z: f64, on_ground: bool) {
        let mut slots = self.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(&uuid) {
            slot.x = x;
            slot.y = y;
            slot.z = z;
            slot.on_ground = on_ground;
        }
    }

    pub fn set_ground(&self, uuid: u128, on_ground: bool) {
        let mut slots = self.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(&uuid) {
            slot.on_ground = on_ground;
        }
    }

    pub fn set_orientation(&self, uuid: u128, yaw: f32, pitch: f32, sneaking: bool) {
        let mut slots = self.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(&uuid) {
            slot.yaw = yaw;
            slot.pitch = pitch;
            slot.sneaking = sneaking;
        }
    }

    pub fn set_appearance(&self, uuid: u128, skin: u8, arm: HumanoidArm) {
        let mut slots = self.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(&uuid) {
            slot.meta.skin = skin;
            slot.meta.arm = arm;
        }
    }

    pub fn apply_crouch(&self, uuid: u128, shift: bool, flying: bool) {
        let mut slots = self.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(&uuid) {
            let new_flags = if shift {
                slot.meta.flags | CROUCH_FLAG
            } else {
                slot.meta.flags & !CROUCH_FLAG
            };
            if new_flags != slot.meta.flags {
                slot.meta.flags = new_flags;
                slot.meta.dirty |= 1 << META_FLAGS;
            }
            let new_pose = if shift && !flying {
                Pose::Crouching
            } else {
                Pose::Standing
            };
            if new_pose != slot.meta.pose {
                slot.meta.pose = new_pose;
                slot.meta.dirty |= 1 << META_POSE;
            }
        }
    }

    pub fn apply_sprint(&self, uuid: u128, sprinting: bool) {
        let mut slots = self.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(&uuid) {
            let new_flags = if sprinting {
                slot.meta.flags | SPRINT_FLAG
            } else {
                slot.meta.flags & !SPRINT_FLAG
            };
            if new_flags != slot.meta.flags {
                slot.meta.flags = new_flags;
                slot.meta.dirty |= 1 << META_FLAGS;
            }
        }
    }

    pub fn apply_appearance_change(&self, uuid: u128, skin: u8, arm: HumanoidArm) {
        let mut slots = self.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(&uuid) {
            if slot.meta.skin != skin {
                slot.meta.skin = skin;
                slot.meta.dirty |= 1 << META_SKIN;
            }
            if slot.meta.arm != arm {
                slot.meta.arm = arm;
                slot.meta.dirty |= 1 << META_ARM;
            }
        }
    }

    pub fn update_sent_chunks(&self, uuid: u128, sent: &HashSet<(i32, i32)>) {
        let mut slots = self.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(&uuid) {
            slot.sent_chunks.clear();
            slot.sent_chunks.extend(sent.iter().copied());
        }
    }

    pub fn orientation(&self, uuid: u128) -> Option<(f32, f32, bool)> {
        let slots = self.slots.lock().unwrap();
        slots
            .get(&uuid)
            .map(|slot| (slot.yaw, slot.pitch, slot.sneaking))
    }

    pub fn set_gamemode(&self, uuid: u128, game_mode: i32) -> Vec<OutboundSender> {
        let mut slots = self.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(&uuid) {
            slot.game_mode = game_mode;
        }
        slots.values().map(|slot| slot.sender.clone()).collect()
    }

    pub fn change_gamemode(&self, uuid: u128, game_mode: i32) -> GamemodeChange {
        let mut slots = self.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(&uuid) {
            slot.game_mode = game_mode;
        }
        let senders: Vec<OutboundSender> = slots.values().map(|slot| slot.sender.clone()).collect();

        let p = match slots.get(&uuid) {
            Some(slot) if slot.ready => ReadyInfo {
                uuid,
                entity_id: slot.entity_id,
                x: slot.x,
                z: slot.z,
                game_mode: slot.game_mode,
            },
            _ => {
                return GamemodeChange {
                    senders,
                    actions: Vec::new(),
                };
            }
        };

        let others: Vec<ReadyInfo> = slots
            .iter()
            .filter(|&(&o_uuid, slot)| slot.ready && o_uuid != uuid)
            .map(|(&o_uuid, slot)| ReadyInfo {
                uuid: o_uuid,
                entity_id: slot.entity_id,
                x: slot.x,
                z: slot.z,
                game_mode: slot.game_mode,
            })
            .collect();

        let mut scratch: Vec<(u8, EntityDataValue)> = Vec::new();
        let mut actions: Vec<ObserverActions> = Vec::new();
        let mut p_removed: Vec<i32> = Vec::new();
        let mut p_entered: Vec<EntitySpawn> = Vec::new();
        let mut inserts: Vec<(u128, i32)> = Vec::new();
        let mut removes: Vec<(u128, i32)> = Vec::new();

        for o in &others {
            let mut o_removed: Vec<i32> = Vec::new();
            let mut o_entered: Vec<EntitySpawn> = Vec::new();

            let was_o_sees_p = slots.get(&p.uuid).unwrap().observers.contains(&o.entity_id);
            let now_o_sees_p = voit(o, &slots.get(&o.uuid).unwrap().sent_chunks, &p);
            if was_o_sees_p && !now_o_sees_p {
                o_removed.push(p.entity_id);
                removes.push((p.uuid, o.entity_id));
            } else if !was_o_sees_p && now_o_sees_p {
                o_entered.push(entity_spawn_of(
                    slots.get(&p.uuid).unwrap(),
                    p.uuid,
                    &mut scratch,
                ));
                inserts.push((p.uuid, o.entity_id));
            }

            let was_p_sees_o = slots.get(&o.uuid).unwrap().observers.contains(&p.entity_id);
            let now_p_sees_o = voit(&p, &slots.get(&p.uuid).unwrap().sent_chunks, o);
            if was_p_sees_o && !now_p_sees_o {
                p_removed.push(o.entity_id);
                removes.push((o.uuid, p.entity_id));
            } else if !was_p_sees_o && now_p_sees_o {
                p_entered.push(entity_spawn_of(
                    slots.get(&o.uuid).unwrap(),
                    o.uuid,
                    &mut scratch,
                ));
                inserts.push((o.uuid, p.entity_id));
            }

            if !o_removed.is_empty() || !o_entered.is_empty() {
                actions.push(ObserverActions {
                    sender: slots.get(&o.uuid).unwrap().sender.clone(),
                    removed: o_removed,
                    entered: o_entered,
                    moved: Vec::new(),
                });
            }
        }

        if !p_removed.is_empty() || !p_entered.is_empty() {
            actions.push(ObserverActions {
                sender: slots.get(&p.uuid).unwrap().sender.clone(),
                removed: p_removed,
                entered: p_entered,
                moved: Vec::new(),
            });
        }

        for (target, observer_entity) in inserts {
            if let Some(slot) = slots.get_mut(&target) {
                slot.observers.insert(observer_entity);
            }
        }
        for (target, observer_entity) in removes {
            if let Some(slot) = slots.get_mut(&target) {
                slot.observers.remove(&observer_entity);
            }
        }

        GamemodeChange { senders, actions }
    }

    pub fn latency_broadcast(&self) -> (Vec<(u128, i32)>, Vec<OutboundSender>) {
        let slots = self.slots.lock().unwrap();
        let entries = slots
            .iter()
            .map(|(&uuid, slot)| (uuid, slot.latency_ms))
            .collect();
        let senders = slots.values().map(|slot| slot.sender.clone()).collect();
        (entries, senders)
    }

    pub fn tick(&self, threshold: usize) -> TickOutput {
        self.tick_counter.fetch_add(1, Ordering::Relaxed);
        let mut slots = self.slots.lock().unwrap();

        let ready: Vec<ReadyInfo> = slots
            .iter()
            .filter(|(_, slot)| slot.ready)
            .map(|(&uuid, slot)| ReadyInfo {
                uuid,
                entity_id: slot.entity_id,
                x: slot.x,
                z: slot.z,
                game_mode: slot.game_mode,
            })
            .collect();

        let mut new_obs: HashMap<i32, HashSet<i32>> = ready
            .iter()
            .map(|r| (r.entity_id, HashSet::new()))
            .collect();
        for a in &ready {
            let a_sent = &slots.get(&a.uuid).unwrap().sent_chunks;
            for b in &ready {
                if a.entity_id == b.entity_id {
                    continue;
                }
                if voit(a, a_sent, b) {
                    new_obs.get_mut(&b.entity_id).unwrap().insert(a.entity_id);
                }
            }
        }

        let before: HashMap<i32, HashSet<i32>> = ready
            .iter()
            .map(|r| (r.entity_id, slots.get(&r.uuid).unwrap().observers.clone()))
            .collect();
        for r in &ready {
            let next = new_obs.get(&r.entity_id).cloned().unwrap_or_default();
            slots.get_mut(&r.uuid).unwrap().observers = next;
        }

        let mut move_frames: HashMap<i32, Arc<[u8]>> = HashMap::new();
        let mut scratch = Vec::new();
        for r in &ready {
            scratch.clear();
            let slot = slots.get_mut(&r.uuid).unwrap();
            let snapshot = slot.snapshot();
            slot.tracked.on_tick(&snapshot, &mut scratch);
            if !scratch.is_empty() {
                let framed = frame_entity_pass(&scratch, threshold);
                move_frames.insert(r.entity_id, Arc::from(framed.into_boxed_slice()));
            }
        }

        let mut actions = Vec::new();
        let mut meta_scratch: Vec<(u8, EntityDataValue)> = Vec::new();
        for a in &ready {
            let mut removed = Vec::new();
            let mut entered = Vec::new();
            let mut moved = Vec::new();
            for b in &ready {
                if a.entity_id == b.entity_id {
                    continue;
                }
                let was = before
                    .get(&b.entity_id)
                    .is_some_and(|s| s.contains(&a.entity_id));
                let now = new_obs
                    .get(&b.entity_id)
                    .is_some_and(|s| s.contains(&a.entity_id));
                if was && !now {
                    removed.push(b.entity_id);
                } else if !was && now {
                    entered.push(entity_spawn_of(
                        slots.get(&b.uuid).unwrap(),
                        b.uuid,
                        &mut meta_scratch,
                    ));
                } else if was
                    && now
                    && let Some(frame) = move_frames.get(&b.entity_id)
                {
                    moved.push(Arc::clone(frame));
                }
            }
            if removed.is_empty() && entered.is_empty() && moved.is_empty() {
                continue;
            }
            let sender = slots.get(&a.uuid).unwrap().sender.clone();
            actions.push(ObserverActions {
                sender,
                removed,
                entered,
                moved,
            });
        }

        let uuid_of: HashMap<i32, u128> = ready.iter().map(|r| (r.entity_id, r.uuid)).collect();
        let mut meta = Vec::new();
        for b in &ready {
            let slot = slots.get(&b.uuid).unwrap();
            if slot.meta.dirty == 0 {
                continue;
            }
            slot.meta.dirty_entries(&mut meta_scratch);
            let payload = set_entity_data_payload(b.entity_id, &meta_scratch);
            let mut targets = vec![slot.sender.clone()];
            let continuing = before
                .get(&b.entity_id)
                .zip(new_obs.get(&b.entity_id))
                .map(|(was, now)| was.intersection(now).copied().collect::<Vec<_>>())
                .unwrap_or_default();
            for observer_id in continuing {
                if let Some(obs_uuid) = uuid_of.get(&observer_id)
                    && let Some(obs_slot) = slots.get(obs_uuid)
                {
                    targets.push(obs_slot.sender.clone());
                }
            }
            meta.push(MetaUpdate { payload, targets });
            slots.get_mut(&b.uuid).unwrap().meta.dirty = 0;
        }

        TickOutput { actions, meta }
    }

    pub fn chat_targets(&self) -> Vec<OutboundSender> {
        self.slots
            .lock()
            .unwrap()
            .values()
            .map(|slot| slot.sender.clone())
            .collect()
    }

    pub fn sender_of(&self, uuid: u128) -> Option<OutboundSender> {
        self.slots
            .lock()
            .unwrap()
            .get(&uuid)
            .map(|slot| slot.sender.clone())
    }

    pub fn set_permission(&self, uuid: u128, level: u8) -> Option<(i32, OutboundSender)> {
        let mut slots = self.slots.lock().unwrap();
        slots.get_mut(&uuid).map(|slot| {
            slot.permission_level = level;
            (slot.entity_id, slot.sender.clone())
        })
    }

    pub fn players(&self) -> Vec<PlayerHandle> {
        self.slots
            .lock()
            .unwrap()
            .iter()
            .map(|(&uuid, slot)| PlayerHandle {
                uuid,
                name: slot.name.clone(),
                x: slot.x,
                z: slot.z,
                sender: slot.sender.clone(),
            })
            .collect()
    }

    pub fn listed_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .slots
            .lock()
            .unwrap()
            .values()
            .map(|slot| slot.name.clone())
            .collect();
        names.sort();
        names
    }

    pub fn max_players(&self) -> i32 {
        self.max_players
    }

    pub fn count(&self) -> usize {
        self.slots.lock().unwrap().len()
    }

    pub fn tab_strings(&self) -> (String, String) {
        let footer = format!("{}/{}", self.count(), self.max_players);
        (self.motd.clone(), footer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    fn registry() -> PlayerRegistry {
        PlayerRegistry::new("Verdigris", 20)
    }

    fn full_square(center: (i32, i32)) -> HashSet<(i32, i32)> {
        let mut set = HashSet::new();
        for x in (center.0 - 8)..=(center.0 + 8) {
            for z in (center.1 - 8)..=(center.1 + 8) {
                set.insert((x, z));
            }
        }
        set
    }

    #[test]
    fn register_returns_existing_and_other_senders() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let snapshot_a = reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        assert!(snapshot_a.existing.is_empty());
        assert!(snapshot_a.other_senders.is_empty());

        let (tx_b, _rx_b) = unbounded_channel();
        let snapshot_b = reg.register(2, 11, (1.5, -60.0, 2.5), "Bob".to_owned(), 0, 25, tx_b);
        assert_eq!(snapshot_b.existing.len(), 1);
        assert_eq!(snapshot_b.existing[0].uuid, 1);
        assert_eq!(snapshot_b.existing[0].name, "Alice");
        assert!(snapshot_b.existing[0].listed);
        assert_eq!(snapshot_b.other_senders.len(), 1);
        assert_eq!(reg.count(), 2);
    }

    #[test]
    fn allocate_entity_id_yields_distinct_values() {
        assert_ne!(allocate_entity_id(), allocate_entity_id());
    }

    #[test]
    fn register_starts_without_observers_and_tracks_spawn() {
        let reg = registry();
        let (tx, _rx) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx);
        let slots = reg.slots.lock().unwrap();
        let slot = slots.get(&1).unwrap();
        assert!(slot.observers.is_empty());
        assert!(!slot.ready);
        assert_eq!(slot.tracked.base_position(), (0.5, -60.0, 0.5));
    }

    #[test]
    fn activate_sets_ready() {
        let reg = registry();
        let (tx, _rx) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx);
        assert!(reg.activate(1));
        assert!(!reg.activate(99));
        assert!(reg.slots.lock().unwrap().get(&1).unwrap().ready);
    }

    #[test]
    fn tick_spawns_entering_pair_once() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let (tx_b, _rx_b) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.register(2, 11, (0.5, -60.0, 0.5), "Bob".to_owned(), 1, 0, tx_b);
        reg.update_sent_chunks(1, &full_square((0, 0)));
        reg.update_sent_chunks(2, &full_square((0, 0)));
        reg.activate(1);
        reg.activate(2);

        let actions = reg.tick(256).actions;
        assert_eq!(actions.len(), 2, "les deux joueurs voient l'autre entrer");
        for action in &actions {
            assert_eq!(action.entered.len(), 1);
            assert!(action.removed.is_empty());
            assert!(action.moved.is_empty());
            let spawn = &action.entered[0];
            assert_eq!((spawn.x, spawn.y, spawn.z), (0.5, -60.0, 0.5));
        }

        let again = reg.tick(256).actions;
        assert!(
            again.iter().all(|a| a.entered.is_empty()),
            "pas de ré-apparition une fois déjà observé"
        );
    }

    #[test]
    fn tick_removes_when_out_of_range() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let (tx_b, _rx_b) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.register(2, 11, (0.5, -60.0, 0.5), "Bob".to_owned(), 1, 0, tx_b);
        reg.update_sent_chunks(1, &full_square((0, 0)));
        reg.update_sent_chunks(2, &full_square((0, 0)));
        reg.activate(1);
        reg.activate(2);
        reg.tick(256);

        reg.set_position(2, 400.0, -60.0, 0.5, true);
        let actions = reg.tick(256).actions;
        let alice = actions
            .iter()
            .find(|a| a.removed == vec![11])
            .expect("Alice perd Bob hors de portée");
        assert!(alice.entered.is_empty());
    }

    #[test]
    fn broadcast_to_survival_sees_survival() {
        assert!(broadcast_to(false, false));
    }

    #[test]
    fn broadcast_to_survival_does_not_see_spectator() {
        assert!(!broadcast_to(false, true));
    }

    #[test]
    fn broadcast_to_spectator_sees_survival() {
        assert!(broadcast_to(true, false));
    }

    #[test]
    fn broadcast_to_spectator_sees_spectator() {
        assert!(broadcast_to(true, true));
    }

    #[test]
    fn change_gamemode_spectator_removes_from_non_spectator_only() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let (tx_b, _rx_b) = unbounded_channel();
        let (tx_c, _rx_c) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.register(2, 11, (0.5, -60.0, 0.5), "Bob".to_owned(), 1, 0, tx_b);
        reg.register(3, 12, (0.5, -60.0, 0.5), "Ghost".to_owned(), 3, 0, tx_c);
        reg.update_sent_chunks(1, &full_square((0, 0)));
        reg.update_sent_chunks(2, &full_square((0, 0)));
        reg.update_sent_chunks(3, &full_square((0, 0)));
        reg.activate(1);
        reg.activate(2);
        reg.activate(3);
        reg.tick(256);

        let change = reg.change_gamemode(2, 3);
        let bob_removed: Vec<i32> = change
            .actions
            .iter()
            .flat_map(|a| a.removed.iter().copied())
            .collect();
        assert_eq!(
            bob_removed,
            vec![11],
            "Bob devenu spectateur disparaît des observateurs non-spectateurs"
        );
        assert!(
            change
                .actions
                .iter()
                .all(|a| a.entered.iter().all(|e| e.entity_id != 11)),
            "Bob ne réapparaît chez personne en entrant spectateur"
        );

        let slots = reg.slots.lock().unwrap();
        assert!(
            slots.get(&2).unwrap().observers.contains(&12),
            "le spectateur Ghost continue de voir Bob"
        );
        assert!(
            !slots.get(&2).unwrap().observers.contains(&10),
            "Alice ne voit plus Bob"
        );
    }

    #[test]
    fn change_gamemode_back_to_survival_respawns_to_observers() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let (tx_b, _rx_b) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.register(2, 11, (0.5, -60.0, 0.5), "Bob".to_owned(), 1, 0, tx_b);
        reg.update_sent_chunks(1, &full_square((0, 0)));
        reg.update_sent_chunks(2, &full_square((0, 0)));
        reg.activate(1);
        reg.activate(2);
        reg.tick(256);

        reg.change_gamemode(2, 3);
        let change = reg.change_gamemode(2, 1);
        let spawned: Vec<i32> = change
            .actions
            .iter()
            .flat_map(|a| a.entered.iter().map(|e| e.entity_id))
            .collect();
        assert_eq!(
            spawned,
            vec![11],
            "le retour en survie réapparaît chez Alice"
        );
        assert!(
            change.actions.iter().all(|a| a.removed.is_empty()),
            "aucun retrait au retour en survie"
        );
    }

    #[test]
    fn change_gamemode_two_spectators_see_each_other() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let (tx_b, _rx_b) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.register(2, 11, (0.5, -60.0, 0.5), "Bob".to_owned(), 1, 0, tx_b);
        reg.update_sent_chunks(1, &full_square((0, 0)));
        reg.update_sent_chunks(2, &full_square((0, 0)));
        reg.activate(1);
        reg.activate(2);
        reg.tick(256);

        reg.change_gamemode(1, 3);
        reg.change_gamemode(2, 3);

        let slots = reg.slots.lock().unwrap();
        assert!(
            slots.get(&1).unwrap().observers.contains(&11),
            "Bob spectateur voit Alice spectatrice"
        );
        assert!(
            slots.get(&2).unwrap().observers.contains(&10),
            "Alice spectatrice voit Bob spectateur"
        );
    }

    #[test]
    fn tick_spectator_is_not_seen_by_others_but_sees_them() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let (tx_b, _rx_b) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.register(2, 11, (0.5, -60.0, 0.5), "Ghost".to_owned(), 3, 0, tx_b);
        reg.update_sent_chunks(1, &full_square((0, 0)));
        reg.update_sent_chunks(2, &full_square((0, 0)));
        reg.activate(1);
        reg.activate(2);

        reg.tick(256);

        let slots = reg.slots.lock().unwrap();
        let alice = slots.get(&1).unwrap();
        let spectator = slots.get(&2).unwrap();
        assert!(
            spectator.observers.is_empty(),
            "le spectateur n'est vu par personne"
        );
        assert_eq!(
            alice.observers.iter().copied().collect::<Vec<_>>(),
            vec![11],
            "le spectateur voit Alice, donc Alice est observée par lui"
        );
    }

    #[test]
    fn depart_reports_entity_and_current_observers() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let (tx_b, _rx_b) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.register(2, 11, (0.5, -60.0, 0.5), "Bob".to_owned(), 1, 0, tx_b);
        reg.update_sent_chunks(1, &full_square((0, 0)));
        reg.update_sent_chunks(2, &full_square((0, 0)));
        reg.activate(1);
        reg.activate(2);
        reg.tick(256);

        let departure = reg.depart(2);
        assert_eq!(departure.entity_id, 11);
        assert_eq!(departure.observers.len(), 1, "Alice observait Bob");
        assert_eq!(departure.remaining.len(), 1);
        assert!(
            !reg.slots
                .lock()
                .unwrap()
                .get(&1)
                .unwrap()
                .observers
                .contains(&11),
            "l'identifiant de Bob est retiré des observers d'Alice"
        );
        assert_eq!(reg.count(), 1);
    }

    #[test]
    fn set_latency_and_gamemode_update_slots() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.set_latency(1, 123);
        let senders = reg.set_gamemode(1, 3);
        assert_eq!(senders.len(), 1);
        let (entries, broadcast) = reg.latency_broadcast();
        assert_eq!(entries, vec![(1, 123)]);
        assert_eq!(broadcast.len(), 1);
    }

    #[test]
    fn tab_strings_report_motd_and_counts() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        let (header, footer) = reg.tab_strings();
        assert_eq!(header, "Verdigris");
        assert_eq!(footer, "1/20");
    }

    #[test]
    fn crouch_while_flying_sets_flag_but_keeps_standing_pose() {
        let reg = registry();
        let (tx, _rx) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Verdi".to_owned(), 1, 0, tx);
        reg.apply_crouch(1, true, true);
        let slots = reg.slots.lock().unwrap();
        let meta = slots.get(&1).unwrap().meta;
        assert_eq!(meta.flags & CROUCH_FLAG, CROUCH_FLAG);
        assert_eq!(meta.pose, Pose::Standing);
        assert_ne!(meta.dirty & (1 << META_FLAGS), 0);
        assert_eq!(meta.dirty & (1 << META_POSE), 0);
    }

    #[test]
    fn crouch_on_ground_sets_crouching_pose() {
        let reg = registry();
        let (tx, _rx) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Verdi".to_owned(), 1, 0, tx);
        reg.apply_crouch(1, true, false);
        let slots = reg.slots.lock().unwrap();
        let meta = slots.get(&1).unwrap().meta;
        assert_eq!(meta.pose, Pose::Crouching);
        assert_ne!(meta.dirty & (1 << META_POSE), 0);
    }

    #[test]
    fn sprint_toggles_flag_and_marks_dirty() {
        let reg = registry();
        let (tx, _rx) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Verdi".to_owned(), 1, 0, tx);
        reg.apply_sprint(1, true);
        {
            let slots = reg.slots.lock().unwrap();
            assert_eq!(slots.get(&1).unwrap().meta.flags & SPRINT_FLAG, SPRINT_FLAG);
        }
        reg.apply_sprint(1, false);
        let slots = reg.slots.lock().unwrap();
        assert_eq!(slots.get(&1).unwrap().meta.flags & SPRINT_FLAG, 0);
    }

    #[test]
    fn spawn_meta_defaults_to_health_and_skin() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let (tx_b, _rx_b) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.register(2, 11, (0.5, -60.0, 0.5), "Bob".to_owned(), 1, 0, tx_b);
        reg.update_sent_chunks(1, &full_square((0, 0)));
        reg.update_sent_chunks(2, &full_square((0, 0)));
        reg.activate(1);
        reg.activate(2);
        let actions = reg.tick(256).actions;
        let spawn = &actions
            .iter()
            .find(|a| a.entered.iter().any(|e| e.entity_id == 11))
            .unwrap()
            .entered[0];
        assert_eq!(
            spawn.meta_payload,
            vec![
                0x0B, 0x09, 0x03, 0x41, 0xA0, 0x00, 0x00, 0x10, 0x00, 0x00, 0xFF
            ]
        );
    }

    #[test]
    fn tick_emits_dirty_meta_to_self_and_observers_then_clears() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let (tx_b, _rx_b) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.register(2, 11, (0.5, -60.0, 0.5), "Bob".to_owned(), 1, 0, tx_b);
        reg.update_sent_chunks(1, &full_square((0, 0)));
        reg.update_sent_chunks(2, &full_square((0, 0)));
        reg.activate(1);
        reg.activate(2);
        reg.tick(256);

        reg.apply_sprint(2, true);
        let output = reg.tick(256);
        assert_eq!(output.meta.len(), 1, "une seule entité a changé");
        let update = &output.meta[0];
        assert_eq!(
            update.targets.len(),
            2,
            "Bob lui-même et Alice observatrice"
        );
        assert_eq!(update.payload, vec![0x0B, 0x00, 0x00, 0x08, 0xFF]);
        assert_eq!(reg.slots.lock().unwrap().get(&2).unwrap().meta.dirty, 0);

        let again = reg.tick(256);
        assert!(again.meta.is_empty(), "plus rien de sale au tick suivant");
    }

    #[test]
    fn swing_refused_inside_half_duration_accepted_after() {
        assert!(accepts_swing(None, 100, 6), "premier balancement accepté");
        assert!(
            !accepts_swing(Some(100), 102, 6),
            "second balancement à 2 ticks refusé"
        );
        assert!(
            accepts_swing(Some(100), 104, 6),
            "balancement à 4 ticks accepté"
        );
        assert!(
            accepts_swing(Some(100), 100, 6),
            "même tick (elapsed 0) accepté"
        );
    }

    #[test]
    fn try_swing_targets_observers_and_enforces_delay() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let (tx_b, _rx_b) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.register(2, 11, (0.5, -60.0, 0.5), "Bob".to_owned(), 1, 0, tx_b);
        reg.update_sent_chunks(1, &full_square((0, 0)));
        reg.update_sent_chunks(2, &full_square((0, 0)));
        reg.activate(1);
        reg.activate(2);
        reg.tick(256);

        let (entity_id, senders) = reg.try_swing(2).expect("premier balancement accepté");
        assert_eq!(entity_id, 11);
        assert_eq!(senders.len(), 1, "seul l'observateur Alice est visé");
        reg.tick(256);
        assert!(
            reg.try_swing(2).is_none(),
            "balancement un tick après le précédent refusé"
        );
    }

    #[test]
    fn update_mainhand_detects_change_and_targets_observers() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let (tx_b, _rx_b) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.register(2, 11, (0.5, -60.0, 0.5), "Bob".to_owned(), 1, 0, tx_b);
        reg.update_sent_chunks(1, &full_square((0, 0)));
        reg.update_sent_chunks(2, &full_square((0, 0)));
        reg.activate(1);
        reg.activate(2);
        reg.tick(256);

        let item = ItemStack::Item { count: 1, item: 55 };
        let (entity_id, senders) = reg.update_mainhand(2, item).expect("changement détecté");
        assert_eq!(entity_id, 11);
        assert_eq!(senders.len(), 1);
        assert!(
            reg.update_mainhand(2, item).is_none(),
            "aucun changement: pas de rediffusion"
        );
        let (_eid, _senders) = reg
            .update_mainhand(2, ItemStack::Empty)
            .expect("passage plein->vide détecté");
    }

    #[test]
    fn set_appearance_does_not_mark_dirty() {
        let reg = registry();
        let (tx, _rx) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Verdi".to_owned(), 1, 0, tx);
        reg.set_appearance(1, 0x7F, HumanoidArm::Left);
        let slots = reg.slots.lock().unwrap();
        let meta = slots.get(&1).unwrap().meta;
        assert_eq!(meta.skin, 0x7F);
        assert_eq!(meta.arm, HumanoidArm::Left);
        assert_eq!(meta.dirty, 0);
    }
}
