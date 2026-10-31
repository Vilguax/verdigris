use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::UnboundedSender;
use verdigris_protocol::{
    EntitySnapshot, ListedPlayer, TrackedState, chunk_coord, frame_entity_pass,
};

pub type OutboundSender = UnboundedSender<Arc<[u8]>>;

pub const TRACK_RANGE_BLOCKS: f64 = 128.0;
const SPECTATOR_GAME_MODE: i32 = 3;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntitySpawn {
    pub entity_id: i32,
    pub uuid: u128,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw_byte: u8,
    pub pitch_byte: u8,
}

pub struct ObserverActions {
    pub sender: OutboundSender,
    pub removed: Vec<i32>,
    pub entered: Vec<EntitySpawn>,
    pub moved: Vec<Arc<[u8]>>,
}

pub struct Departure {
    pub entity_id: i32,
    pub observers: Vec<OutboundSender>,
    pub remaining: Vec<OutboundSender>,
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

fn broadcast_to_player(observer_mode: i32, target_mode: i32) -> bool {
    is_spectator(observer_mode) || !is_spectator(target_mode)
}

fn within_range(a: &ReadyInfo, b: &ReadyInfo) -> bool {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    dx * dx + dz * dz <= TRACK_RANGE_BLOCKS * TRACK_RANGE_BLOCKS
}

pub struct PlayerRegistry {
    slots: Mutex<HashMap<u128, Slot>>,
    motd: String,
    max_players: i32,
}

impl PlayerRegistry {
    pub fn new(motd: &str, max_players: i32) -> Self {
        PlayerRegistry {
            slots: Mutex::new(HashMap::new()),
            motd: motd.to_owned(),
            max_players,
        }
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

    pub fn latency_broadcast(&self) -> (Vec<(u128, i32)>, Vec<OutboundSender>) {
        let slots = self.slots.lock().unwrap();
        let entries = slots
            .iter()
            .map(|(&uuid, slot)| (uuid, slot.latency_ms))
            .collect();
        let senders = slots.values().map(|slot| slot.sender.clone()).collect();
        (entries, senders)
    }

    pub fn tick(&self, threshold: usize) -> Vec<ObserverActions> {
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
                let visible = within_range(a, b)
                    && a_sent.contains(&(chunk_coord(b.x), chunk_coord(b.z)))
                    && broadcast_to_player(a.game_mode, b.game_mode);
                if visible {
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
                    let b_slot = slots.get(&b.uuid).unwrap();
                    let (bx, by, bz) = b_slot.tracked.base_position();
                    entered.push(EntitySpawn {
                        entity_id: b.entity_id,
                        uuid: b.uuid,
                        x: bx,
                        y: by,
                        z: bz,
                        yaw_byte: b_slot.tracked.base_yaw_byte(),
                        pitch_byte: b_slot.tracked.base_pitch_byte(),
                    });
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
        actions
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

        let actions = reg.tick(256);
        assert_eq!(actions.len(), 2, "les deux joueurs voient l'autre entrer");
        for action in &actions {
            assert_eq!(action.entered.len(), 1);
            assert!(action.removed.is_empty());
            assert!(action.moved.is_empty());
            let spawn = &action.entered[0];
            assert_eq!((spawn.x, spawn.y, spawn.z), (0.5, -60.0, 0.5));
        }

        let again = reg.tick(256);
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
        let actions = reg.tick(256);
        let alice = actions
            .iter()
            .find(|a| a.removed == vec![11])
            .expect("Alice perd Bob hors de portée");
        assert!(alice.entered.is_empty());
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
}
