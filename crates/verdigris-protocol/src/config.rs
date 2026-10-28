use crate::MAX_STRING_LEN;
use crate::error::ProtocolError;
use crate::network::{KnownPack, Registry, TagRegistry, network_data};
use crate::primitives::{read_string, write_string};
use crate::varint::{read_var_int, write_var_int};

pub const MAX_KNOWN_PACKS: usize = 1024;

pub mod packet_ids {
    pub mod clientbound {
        pub const REGISTRY_DATA: i32 = 0x07;
        pub const UPDATE_TAGS: i32 = 0x0E;
        pub const SELECT_KNOWN_PACKS: i32 = 0x0F;
        pub const FINISH_CONFIGURATION: i32 = 0x03;
        pub const KEEP_ALIVE: i32 = 0x04;
        pub const DISCONNECT: i32 = 0x02;
    }

    pub mod serverbound {
        pub const CLIENT_INFORMATION: i32 = 0x00;
        pub const CUSTOM_PAYLOAD: i32 = 0x02;
        pub const FINISH_CONFIGURATION: i32 = 0x03;
        pub const KEEP_ALIVE: i32 = 0x04;
        pub const SELECT_KNOWN_PACKS: i32 = 0x07;
    }

    pub mod play {
        pub const DISCONNECT: i32 = 0x20;
    }
}

pub fn known_packs_payload(packs: &[KnownPack]) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(packs.len() as i32, &mut payload);
    for pack in packs {
        write_string(&pack.namespace, &mut payload);
        write_string(&pack.id, &mut payload);
        write_string(&pack.version, &mut payload);
    }
    payload
}

pub fn server_known_pack() -> KnownPack {
    network_data().known_pack.clone()
}

pub fn registry_data_payload(registry: &Registry) -> Vec<u8> {
    let mut payload = Vec::new();
    write_string(&registry.id, &mut payload);
    write_var_int(registry.entries.len() as i32, &mut payload);
    for entry in &registry.entries {
        write_string(entry, &mut payload);
        payload.push(0);
    }
    payload
}

pub fn update_tags_payload(tags: &[TagRegistry]) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(tags.len() as i32, &mut payload);
    for tag_registry in tags {
        write_string(&tag_registry.registry, &mut payload);
        write_var_int(tag_registry.tags.len() as i32, &mut payload);
        for tag in &tag_registry.tags {
            write_string(&tag.name, &mut payload);
            write_var_int(tag.ids.len() as i32, &mut payload);
            for &id in &tag.ids {
                write_var_int(id, &mut payload);
            }
        }
    }
    payload
}

pub fn decode_known_packs(body: &[u8]) -> Result<Vec<KnownPack>, ProtocolError> {
    let mut cursor = body;
    let count = take_var_int(&mut cursor)?;
    if count < 0 {
        return Err(ProtocolError::PrefixedArrayLengthNegative);
    }
    let count = count as usize;
    if count > MAX_KNOWN_PACKS {
        return Err(ProtocolError::PrefixedArrayTooLong);
    }
    let mut packs = Vec::with_capacity(count);
    for _ in 0..count {
        let namespace = take_string(&mut cursor, MAX_STRING_LEN)?;
        let id = take_string(&mut cursor, MAX_STRING_LEN)?;
        let version = take_string(&mut cursor, MAX_STRING_LEN)?;
        packs.push(KnownPack {
            namespace,
            id,
            version,
        });
    }
    if !cursor.is_empty() {
        return Err(ProtocolError::TrailingBytes);
    }
    Ok(packs)
}

pub fn client_has_core(packs: &[KnownPack]) -> bool {
    let expected = &network_data().known_pack;
    packs.iter().any(|pack| pack == expected)
}

fn take_var_int(cursor: &mut &[u8]) -> Result<i32, ProtocolError> {
    match read_var_int(cursor)? {
        Some((value, consumed)) => {
            *cursor = &cursor[consumed..];
            Ok(value)
        }
        None => Err(ProtocolError::UnexpectedEof),
    }
}

fn take_string(cursor: &mut &[u8], max_len: usize) -> Result<String, ProtocolError> {
    match read_string(cursor, max_len)? {
        Some((value, consumed)) => {
            *cursor = &cursor[consumed..];
            Ok(value)
        }
        None => Err(ProtocolError::UnexpectedEof),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GAME_VERSION;

    #[test]
    fn registry_data_golden_two_entries() {
        let registry = Registry {
            id: "minecraft:test".to_owned(),
            entries: vec!["minecraft:a".to_owned(), "minecraft:b".to_owned()],
        };
        let mut expected = Vec::new();
        expected.push(0x0E);
        expected.extend_from_slice(b"minecraft:test");
        expected.push(0x02);
        expected.push(0x0B);
        expected.extend_from_slice(b"minecraft:a");
        expected.push(0x00);
        expected.push(0x0B);
        expected.extend_from_slice(b"minecraft:b");
        expected.push(0x00);
        assert_eq!(registry_data_payload(&registry), expected);
    }

    #[test]
    fn known_packs_golden_single() {
        let packs = vec![KnownPack {
            namespace: "minecraft".to_owned(),
            id: "core".to_owned(),
            version: "26.3".to_owned(),
        }];
        let mut expected = Vec::new();
        expected.push(0x01);
        expected.push(0x09);
        expected.extend_from_slice(b"minecraft");
        expected.push(0x04);
        expected.extend_from_slice(b"core");
        expected.push(0x04);
        expected.extend_from_slice(b"26.3");
        assert_eq!(known_packs_payload(&packs), expected);
    }

    #[test]
    fn update_tags_golden_single() {
        let tags = vec![TagRegistry {
            registry: "minecraft:x".to_owned(),
            tags: vec![crate::network::TagEntry {
                name: "minecraft:t".to_owned(),
                ids: vec![0, 1, 130],
            }],
        }];
        let mut expected = Vec::new();
        expected.push(0x01);
        expected.push(0x0B);
        expected.extend_from_slice(b"minecraft:x");
        expected.push(0x01);
        expected.push(0x0B);
        expected.extend_from_slice(b"minecraft:t");
        expected.push(0x03);
        expected.push(0x00);
        expected.push(0x01);
        expected.push(0x82);
        expected.push(0x01);
        assert_eq!(update_tags_payload(&tags), expected);
    }

    #[test]
    fn known_packs_round_trip() {
        let packs = vec![
            KnownPack {
                namespace: "minecraft".to_owned(),
                id: "core".to_owned(),
                version: "26.3".to_owned(),
            },
            KnownPack {
                namespace: "verdigris".to_owned(),
                id: "extra".to_owned(),
                version: "1".to_owned(),
            },
        ];
        let encoded = known_packs_payload(&packs);
        assert_eq!(decode_known_packs(&encoded).unwrap(), packs);
    }

    #[test]
    fn known_packs_empty_round_trip() {
        let encoded = known_packs_payload(&[]);
        assert_eq!(decode_known_packs(&encoded).unwrap(), Vec::new());
    }

    #[test]
    fn decode_known_packs_trailing_rejected() {
        let mut encoded = known_packs_payload(&[]);
        encoded.push(0xFF);
        assert_eq!(
            decode_known_packs(&encoded),
            Err(ProtocolError::TrailingBytes)
        );
    }

    #[test]
    fn decode_known_packs_truncated_is_eof() {
        let packs = vec![KnownPack {
            namespace: "minecraft".to_owned(),
            id: "core".to_owned(),
            version: "26.3".to_owned(),
        }];
        let mut encoded = known_packs_payload(&packs);
        encoded.truncate(encoded.len() - 1);
        assert_eq!(
            decode_known_packs(&encoded),
            Err(ProtocolError::UnexpectedEof)
        );
    }

    #[test]
    fn decode_known_packs_negative_count_rejected() {
        let mut body = Vec::new();
        write_var_int(-1, &mut body);
        assert_eq!(
            decode_known_packs(&body),
            Err(ProtocolError::PrefixedArrayLengthNegative)
        );
    }

    #[test]
    fn decode_known_packs_oversize_count_rejected() {
        let mut body = Vec::new();
        write_var_int((MAX_KNOWN_PACKS + 1) as i32, &mut body);
        assert_eq!(
            decode_known_packs(&body),
            Err(ProtocolError::PrefixedArrayTooLong)
        );
    }

    #[test]
    fn server_known_pack_is_core() {
        let pack = server_known_pack();
        assert_eq!(pack.namespace, "minecraft");
        assert_eq!(pack.id, "core");
        assert_eq!(pack.version, GAME_VERSION);
    }

    #[test]
    fn client_has_core_detects_match() {
        assert!(client_has_core(&[server_known_pack()]));
    }

    #[test]
    fn client_has_core_rejects_wrong_version() {
        assert!(!client_has_core(&[KnownPack {
            namespace: "minecraft".to_owned(),
            id: "core".to_owned(),
            version: "1.21".to_owned(),
        }]));
    }

    #[test]
    fn client_has_core_rejects_missing() {
        assert!(!client_has_core(&[]));
    }

    #[test]
    fn packet_ids_match_report() {
        assert_eq!(packet_ids::clientbound::REGISTRY_DATA, 0x07);
        assert_eq!(packet_ids::clientbound::UPDATE_TAGS, 0x0E);
        assert_eq!(packet_ids::clientbound::SELECT_KNOWN_PACKS, 0x0F);
        assert_eq!(packet_ids::clientbound::FINISH_CONFIGURATION, 0x03);
        assert_eq!(packet_ids::clientbound::KEEP_ALIVE, 0x04);
        assert_eq!(packet_ids::clientbound::DISCONNECT, 0x02);
        assert_eq!(packet_ids::serverbound::CLIENT_INFORMATION, 0x00);
        assert_eq!(packet_ids::serverbound::CUSTOM_PAYLOAD, 0x02);
        assert_eq!(packet_ids::serverbound::FINISH_CONFIGURATION, 0x03);
        assert_eq!(packet_ids::serverbound::KEEP_ALIVE, 0x04);
        assert_eq!(packet_ids::serverbound::SELECT_KNOWN_PACKS, 0x07);
        assert_eq!(packet_ids::play::DISCONNECT, 0x20);
    }

    #[test]
    fn real_registry_data_has_no_nbt() {
        let registry = network_data()
            .registry("minecraft:chicken_variant")
            .unwrap();
        let payload = registry_data_payload(registry);
        assert_eq!(*payload.last().unwrap(), 0x00);
    }
}
