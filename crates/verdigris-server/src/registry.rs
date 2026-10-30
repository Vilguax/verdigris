use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::UnboundedSender;
use verdigris_protocol::ListedPlayer;

pub type OutboundSender = UnboundedSender<Arc<[u8]>>;

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
    #[allow(dead_code)]
    on_ground: bool,
    ready: bool,
    sender: OutboundSender,
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
    pub yaw: f32,
    pub pitch: f32,
}

pub struct SpawnExchange {
    pub own: EntitySpawn,
    pub observers: Vec<(OutboundSender, EntitySpawn)>,
}

pub struct Departure {
    pub entity_id: i32,
    pub observers: Vec<OutboundSender>,
    pub remaining: Vec<OutboundSender>,
}

fn spawn_of(uuid: u128, slot: &Slot) -> EntitySpawn {
    EntitySpawn {
        entity_id: slot.entity_id,
        uuid,
        x: slot.x,
        y: slot.y,
        z: slot.z,
        yaw: slot.yaw,
        pitch: slot.pitch,
    }
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
                on_ground: false,
                ready: false,
                sender,
            },
        );
        RegisterSnapshot {
            existing,
            other_senders,
        }
    }

    pub fn activate(&self, uuid: u128) -> Option<SpawnExchange> {
        let mut slots = self.slots.lock().unwrap();
        let own = {
            let slot = slots.get_mut(&uuid)?;
            slot.ready = true;
            spawn_of(uuid, slot)
        };
        let observers = slots
            .iter()
            .filter(|&(&other, slot)| other != uuid && slot.ready)
            .map(|(&other, slot)| (slot.sender.clone(), spawn_of(other, slot)))
            .collect();
        Some(SpawnExchange { own, observers })
    }

    pub fn depart(&self, uuid: u128) -> Departure {
        let mut slots = self.slots.lock().unwrap();
        let entity_id = slots.get(&uuid).map(|slot| slot.entity_id).unwrap_or(0);
        let observers = slots
            .iter()
            .filter(|&(&other, slot)| other != uuid && slot.ready)
            .map(|(_, slot)| slot.sender.clone())
            .collect();
        slots.remove(&uuid);
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

    pub fn set_orientation(&self, uuid: u128, yaw: f32, pitch: f32, sneaking: bool) {
        let mut slots = self.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(&uuid) {
            slot.yaw = yaw;
            slot.pitch = pitch;
            slot.sneaking = sneaking;
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
    fn activate_exposes_only_ready_observers() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let (tx_b, _rx_b) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.register(2, 11, (1.5, -60.0, 2.5), "Bob".to_owned(), 0, 0, tx_b);

        let alice = reg.activate(1).unwrap();
        assert_eq!(alice.own.entity_id, 10);
        assert!(
            alice.observers.is_empty(),
            "Bob n'est pas encore prêt quand Alice s'active"
        );

        let bob = reg.activate(2).unwrap();
        assert_eq!(bob.own.entity_id, 11);
        assert_eq!(bob.observers.len(), 1, "Alice prête est observée");
        assert_eq!(bob.observers[0].1.entity_id, 10);
        assert_eq!(bob.observers[0].1.uuid, 1);
    }

    #[test]
    fn depart_reports_entity_and_ready_observers() {
        let reg = registry();
        let (tx_a, _rx_a) = unbounded_channel();
        let (tx_b, _rx_b) = unbounded_channel();
        reg.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        reg.register(2, 11, (1.5, -60.0, 2.5), "Bob".to_owned(), 0, 0, tx_b);
        reg.activate(1);
        reg.activate(2);

        let departure = reg.depart(2);
        assert_eq!(departure.entity_id, 11);
        assert_eq!(
            departure.observers.len(),
            1,
            "Alice observe le départ de Bob"
        );
        assert_eq!(departure.remaining.len(), 1);
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
