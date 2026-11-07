use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const MENU_REGISTRY: &str = "minecraft:menu";
pub const BLOCK_ENTITY_TYPE_REGISTRY: &str = "minecraft:block_entity_type";
pub const SOUND_EVENT_REGISTRY: &str = "minecraft:sound_event";
pub const ENTITY_TYPE_REGISTRY: &str = "minecraft:entity_type";

#[derive(Debug, Clone, Deserialize)]
struct RawData {
    game_version: String,
    server_jar_sha1: String,
    registries: HashMap<String, HashMap<String, i32>>,
}

struct Table {
    names: Vec<String>,
    ids: HashMap<String, i32>,
}

struct StaticRegistries {
    game_version: String,
    server_jar_sha1: String,
    tables: HashMap<String, Table>,
}

static DATA: OnceLock<StaticRegistries> = OnceLock::new();

fn build_table(registry: &str, entries: &HashMap<String, i32>) -> Table {
    let count = entries.len();
    let mut slots: Vec<Option<String>> = vec![None; count];
    for (name, &id) in entries {
        let index = usize::try_from(id)
            .ok()
            .filter(|&index| index < count)
            .unwrap_or_else(|| panic!("registre {registry}: id {id} hors de 0..{count}"));
        if slots[index].is_some() {
            panic!("registre {registry}: id {id} duplique");
        }
        slots[index] = Some(name.clone());
    }
    let names: Vec<String> = slots
        .into_iter()
        .enumerate()
        .map(|(index, slot)| {
            slot.unwrap_or_else(|| panic!("registre {registry}: id {index} manquant"))
        })
        .collect();
    Table {
        names,
        ids: entries.clone(),
    }
}

fn data() -> &'static StaticRegistries {
    DATA.get_or_init(|| {
        let raw: RawData =
            serde_json::from_str(include_str!("../data/26.3/static_registries.json"))
                .expect("embedded 26.3/static_registries.json is valid");
        let tables = raw
            .registries
            .iter()
            .map(|(registry, entries)| (registry.clone(), build_table(registry, entries)))
            .collect();
        StaticRegistries {
            game_version: raw.game_version,
            server_jar_sha1: raw.server_jar_sha1,
            tables,
        }
    })
}

pub fn static_registries_game_version() -> &'static str {
    data().game_version.as_str()
}

pub fn static_registries_sha1() -> &'static str {
    data().server_jar_sha1.as_str()
}

pub fn static_registry_id(registry: &str, entry: &str) -> Option<i32> {
    data()
        .tables
        .get(registry)
        .and_then(|table| table.ids.get(entry).copied())
}

pub fn static_registry_name(registry: &str, id: i32) -> Option<&'static str> {
    let table = data().tables.get(registry)?;
    let index = usize::try_from(id).ok()?;
    table.names.get(index).map(String::as_str)
}

pub fn static_registry_len(registry: &str) -> Option<usize> {
    data().tables.get(registry).map(|table| table.names.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_cible_26_3() {
        assert_eq!(static_registries_game_version(), "26.3");
        assert_eq!(
            static_registries_sha1(),
            "33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c"
        );
    }

    #[test]
    fn longueurs_attendues() {
        assert_eq!(static_registry_len(MENU_REGISTRY), Some(25));
        assert_eq!(static_registry_len(BLOCK_ENTITY_TYPE_REGISTRY), Some(49));
        assert_eq!(static_registry_len(SOUND_EVENT_REGISTRY), Some(1991));
        assert_eq!(static_registry_len(ENTITY_TYPE_REGISTRY), Some(161));
        assert_eq!(static_registry_len("minecraft:absent"), None);
    }

    #[test]
    fn ids_contigus() {
        for registry in [
            MENU_REGISTRY,
            BLOCK_ENTITY_TYPE_REGISTRY,
            SOUND_EVENT_REGISTRY,
            ENTITY_TYPE_REGISTRY,
        ] {
            let len = static_registry_len(registry).unwrap();
            for id in 0..len as i32 {
                let name = static_registry_name(registry, id).unwrap();
                assert_eq!(static_registry_id(registry, name), Some(id));
            }
            assert_eq!(static_registry_name(registry, len as i32), None);
        }
    }

    #[test]
    fn recherches_par_nom() {
        let id = static_registry_id;
        assert_eq!(id(MENU_REGISTRY, "minecraft:generic_9x1"), Some(0));
        assert_eq!(id(MENU_REGISTRY, "minecraft:anvil"), Some(8));
        assert_eq!(id(MENU_REGISTRY, "minecraft:stonecutter"), Some(24));
        assert_eq!(id(BLOCK_ENTITY_TYPE_REGISTRY, "minecraft:furnace"), Some(0));
        assert_eq!(id(BLOCK_ENTITY_TYPE_REGISTRY, "minecraft:chest"), Some(1));
        assert_eq!(id(ENTITY_TYPE_REGISTRY, "minecraft:allay"), Some(2));
        assert_eq!(id(ENTITY_TYPE_REGISTRY, "minecraft:player"), Some(159));
        assert_eq!(
            id(SOUND_EVENT_REGISTRY, "minecraft:entity.player.levelup"),
            Some(1321)
        );
    }

    #[test]
    fn recherches_par_id() {
        let name = static_registry_name;
        assert_eq!(name(MENU_REGISTRY, 0), Some("minecraft:generic_9x1"));
        assert_eq!(
            name(ENTITY_TYPE_REGISTRY, 160),
            Some("minecraft:fishing_bobber")
        );
        assert_eq!(
            name(SOUND_EVENT_REGISTRY, 1990),
            Some("minecraft:block.red_shrub.place")
        );
    }

    #[test]
    fn inconnu_rend_none_sans_paniquer() {
        assert_eq!(static_registry_id(MENU_REGISTRY, "minecraft:absent"), None);
        assert_eq!(static_registry_id("minecraft:absent", "x"), None);
        assert_eq!(static_registry_name(MENU_REGISTRY, -1), None);
        assert_eq!(static_registry_name(MENU_REGISTRY, 100000), None);
        assert_eq!(static_registry_name("minecraft:absent", 0), None);
    }
}
