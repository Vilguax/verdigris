use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
pub struct NetworkData {
    pub game_version: String,
    pub protocol: i32,
    pub source: Source,
    pub known_pack: KnownPack,
    pub registries: Vec<Registry>,
    pub tags: Vec<TagRegistry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Source {
    pub server_jar_sha1: String,
    pub generator: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KnownPack {
    pub namespace: String,
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Registry {
    pub id: String,
    pub entries: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TagRegistry {
    pub registry: String,
    pub tags: Vec<TagEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TagEntry {
    pub name: String,
    pub ids: Vec<i32>,
}

static DATA: OnceLock<NetworkData> = OnceLock::new();

pub fn network_data() -> &'static NetworkData {
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../data/26.3/network.json"))
            .expect("embedded 26.3/network.json is valid NetworkData")
    })
}

impl Registry {
    pub fn entry_id(&self, identifier: &str) -> Option<i32> {
        self.entries
            .iter()
            .position(|entry| entry == identifier)
            .map(|index| index as i32)
    }

    pub fn entry_name(&self, id: i32) -> Option<&str> {
        usize::try_from(id)
            .ok()
            .and_then(|index| self.entries.get(index))
            .map(String::as_str)
    }
}

impl NetworkData {
    pub fn registry(&self, id: &str) -> Option<&Registry> {
        self.registries.iter().find(|registry| registry.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GAME_VERSION, PROTOCOL_VERSION};

    const REGISTRY_ORDER: [&str; 29] = [
        "minecraft:banner_pattern",
        "minecraft:chat_type",
        "minecraft:damage_type",
        "minecraft:dialog",
        "minecraft:dimension_type",
        "minecraft:enchantment",
        "minecraft:instrument",
        "minecraft:jukebox_song",
        "minecraft:painting_variant",
        "minecraft:sulfur_cube_archetype",
        "minecraft:test_environment",
        "minecraft:test_instance",
        "minecraft:timeline",
        "minecraft:trim_material",
        "minecraft:trim_pattern",
        "minecraft:world_clock",
        "minecraft:worldgen/biome",
        "minecraft:cat_variant",
        "minecraft:cat_sound_variant",
        "minecraft:chicken_variant",
        "minecraft:chicken_sound_variant",
        "minecraft:cow_variant",
        "minecraft:cow_sound_variant",
        "minecraft:frog_variant",
        "minecraft:pig_variant",
        "minecraft:pig_sound_variant",
        "minecraft:wolf_variant",
        "minecraft:wolf_sound_variant",
        "minecraft:zombie_nautilus_variant",
    ];

    #[test]
    fn header_matches_target() {
        let data = network_data();
        assert_eq!(data.game_version, GAME_VERSION);
        assert_eq!(data.protocol, PROTOCOL_VERSION);
        assert_eq!(data.known_pack.namespace, "minecraft");
        assert_eq!(data.known_pack.id, "core");
        assert_eq!(data.known_pack.version, GAME_VERSION);
    }

    #[test]
    fn loads_once() {
        let first = network_data() as *const NetworkData;
        let second = network_data() as *const NetworkData;
        assert_eq!(first, second);
    }

    #[test]
    fn twenty_nine_registries_in_file_order() {
        let data = network_data();
        assert_eq!(data.registries.len(), 29);
        let ids: Vec<&str> = data.registries.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, REGISTRY_ORDER.to_vec());
    }

    #[test]
    fn entry_ids_are_zero_based_positions() {
        let biome = network_data().registry("minecraft:worldgen/biome").unwrap();
        let id = biome.entry_id("minecraft:plains").unwrap();
        assert_eq!(biome.entries[id as usize], "minecraft:plains");
        assert_eq!(biome.entry_name(id), Some("minecraft:plains"));
        assert_eq!(biome.entry_id("minecraft:does_not_exist"), None);
    }

    #[test]
    fn required_entries_present() {
        let data = network_data();

        let biome = data.registry("minecraft:worldgen/biome").unwrap();
        assert!(biome.entry_id("minecraft:plains").is_some());

        let dimension = data.registry("minecraft:dimension_type").unwrap();
        assert!(!dimension.entries.is_empty());

        let damage = data.registry("minecraft:damage_type").unwrap();
        for entry in [
            "minecraft:generic",
            "minecraft:out_of_world",
            "minecraft:wither",
        ] {
            assert!(damage.entry_id(entry).is_some(), "missing {entry}");
        }

        let instrument = data.registry("minecraft:instrument").unwrap();
        assert!(instrument.entry_id("minecraft:ponder_goat_horn").is_some());

        let chicken = data.registry("minecraft:chicken_variant").unwrap();
        for entry in ["minecraft:cold", "minecraft:temperate", "minecraft:warm"] {
            assert!(chicken.entry_id(entry).is_some(), "missing {entry}");
        }
    }

    #[test]
    fn required_tags_present() {
        let data = network_data();

        let damage = data
            .tags
            .iter()
            .find(|t| t.registry == "minecraft:damage_type")
            .unwrap();
        for name in [
            "minecraft:bypasses_shield",
            "minecraft:is_explosion",
            "minecraft:is_fire",
        ] {
            assert!(
                damage.tags.iter().any(|tag| tag.name == name),
                "missing {name}"
            );
        }

        let banner = data
            .tags
            .iter()
            .find(|t| t.registry == "minecraft:banner_pattern")
            .unwrap();
        assert!(
            banner
                .tags
                .iter()
                .any(|tag| tag.name == "minecraft:pattern_item/creeper")
        );
    }

    #[test]
    fn synced_tag_ids_within_registry_bounds() {
        let data = network_data();
        for tag_registry in &data.tags {
            let Some(registry) = data.registry(&tag_registry.registry) else {
                continue;
            };
            let count = registry.entries.len() as i32;
            for tag in &tag_registry.tags {
                for &id in &tag.ids {
                    assert!(
                        id >= 0 && id < count,
                        "{}/{} id {id} out of 0..{count}",
                        tag_registry.registry,
                        tag.name
                    );
                }
            }
        }
    }
}
