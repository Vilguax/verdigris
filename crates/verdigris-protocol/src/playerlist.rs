use crate::MAX_USERNAME_LEN;
use crate::clientbound::{EncodeError, text_component_nbt};
use crate::primitives::write_string;
use crate::varint::write_var_int;

pub const ACTION_ADD_PLAYER: u8 = 0x01;
pub const ACTION_INITIALIZE_CHAT: u8 = 0x02;
pub const ACTION_UPDATE_GAME_MODE: u8 = 0x04;
pub const ACTION_UPDATE_LISTED: u8 = 0x08;
pub const ACTION_UPDATE_LATENCY: u8 = 0x10;
pub const ACTION_UPDATE_DISPLAY_NAME: u8 = 0x20;
pub const ACTION_UPDATE_LIST_ORDER: u8 = 0x40;
pub const ACTION_UPDATE_HAT: u8 = 0x80;

pub const ACTIONS_INIT: u8 = ACTION_ADD_PLAYER
    | ACTION_INITIALIZE_CHAT
    | ACTION_UPDATE_GAME_MODE
    | ACTION_UPDATE_LISTED
    | ACTION_UPDATE_LATENCY
    | ACTION_UPDATE_DISPLAY_NAME
    | ACTION_UPDATE_LIST_ORDER
    | ACTION_UPDATE_HAT;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedPlayer {
    pub uuid: u128,
    pub name: String,
    pub game_mode: i32,
    pub latency_ms: i32,
    pub listed: bool,
}

fn write_uuid(uuid: u128, out: &mut Vec<u8>) {
    out.extend_from_slice(&uuid.to_be_bytes());
}

fn write_bool(value: bool, out: &mut Vec<u8>) {
    out.push(u8::from(value));
}

pub fn player_info_update_init_payload(players: &[ListedPlayer]) -> Result<Vec<u8>, EncodeError> {
    for player in players {
        if player.name.chars().count() > MAX_USERNAME_LEN {
            return Err(EncodeError::NameTooLong);
        }
    }

    let mut payload = Vec::new();
    payload.push(ACTIONS_INIT);
    write_var_int(players.len() as i32, &mut payload);
    for player in players {
        write_uuid(player.uuid, &mut payload);
        write_string(&player.name, &mut payload);
        write_var_int(0, &mut payload);
        write_bool(false, &mut payload);
        write_var_int(player.game_mode, &mut payload);
        write_bool(player.listed, &mut payload);
        write_var_int(player.latency_ms, &mut payload);
        write_bool(false, &mut payload);
        write_var_int(0, &mut payload);
        write_bool(true, &mut payload);
    }
    Ok(payload)
}

pub fn player_info_update_latency_payload(entries: &[(u128, i32)]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(ACTION_UPDATE_LATENCY);
    write_var_int(entries.len() as i32, &mut payload);
    for &(uuid, latency_ms) in entries {
        write_uuid(uuid, &mut payload);
        write_var_int(latency_ms, &mut payload);
    }
    payload
}

pub fn player_info_update_gamemode_payload(uuid: u128, mode: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(ACTION_UPDATE_GAME_MODE);
    write_var_int(1, &mut payload);
    write_uuid(uuid, &mut payload);
    write_var_int(mode, &mut payload);
    payload
}

pub fn player_info_remove_payload(uuids: &[u128]) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(uuids.len() as i32, &mut payload);
    for &uuid in uuids {
        write_uuid(uuid, &mut payload);
    }
    payload
}

pub fn tab_list_payload(header: &str, footer: &str) -> Vec<u8> {
    let mut payload = text_component_nbt(header);
    payload.extend_from_slice(&text_component_nbt(footer));
    payload
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::login::offline_uuid;

    fn sample_uuid() -> u128 {
        u128::from_be_bytes([
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
            0x0E, 0x0F,
        ])
    }

    #[test]
    fn init_single_creative_player_exact_bytes() {
        let uuid = offline_uuid("Verdi");
        let players = [ListedPlayer {
            uuid,
            name: "Verdi".to_owned(),
            game_mode: 1,
            latency_ms: 0,
            listed: true,
        }];
        let payload = player_info_update_init_payload(&players).unwrap();

        let mut expected = Vec::new();
        expected.extend_from_slice(&[0xFF, 0x01]);
        expected.extend_from_slice(&uuid.to_be_bytes());
        expected.extend_from_slice(&[0x05]);
        expected.extend_from_slice(b"Verdi");
        expected.extend_from_slice(&[0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x01]);

        assert_eq!(payload, expected);
    }

    #[test]
    fn init_rejects_name_over_sixteen() {
        let players = [ListedPlayer {
            uuid: sample_uuid(),
            name: "a".repeat(17),
            game_mode: 0,
            latency_ms: 0,
            listed: true,
        }];
        assert_eq!(
            player_info_update_init_payload(&players),
            Err(EncodeError::NameTooLong)
        );
    }

    #[test]
    fn latency_payload_exact_bytes() {
        let payload = player_info_update_latency_payload(&[(sample_uuid(), 42)]);
        assert_eq!(
            payload,
            vec![
                0x10, 0x01, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
                0x0C, 0x0D, 0x0E, 0x0F, 0x2A,
            ]
        );
    }

    #[test]
    fn gamemode_payload_exact_bytes() {
        let payload = player_info_update_gamemode_payload(sample_uuid(), 3);
        assert_eq!(
            payload,
            vec![
                0x04, 0x01, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
                0x0C, 0x0D, 0x0E, 0x0F, 0x03,
            ]
        );
    }

    #[test]
    fn remove_payload_exact_bytes() {
        let payload = player_info_remove_payload(&[sample_uuid()]);
        assert_eq!(
            payload,
            vec![
                0x01, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
                0x0D, 0x0E, 0x0F,
            ]
        );
    }

    #[test]
    fn tab_list_payload_exact_bytes() {
        let payload = tab_list_payload("A", "B");
        assert_eq!(
            payload,
            vec![0x08, 0x00, 0x01, b'A', 0x08, 0x00, 0x01, b'B']
        );
    }
}
