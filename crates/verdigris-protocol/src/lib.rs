pub mod error;
pub mod framing;
pub mod packets;
pub mod primitives;
pub mod status;
pub mod varint;

pub const GAME_VERSION: &str = "26.3";
pub const PROTOCOL_VERSION: i32 = 777;
pub const DATA_VERSION: i32 = 5023;

pub const MAX_PACKET_LEN: usize = 2 * 1024 * 1024;
pub const MAX_STRING_LEN: usize = 32767;
pub const MAX_ADDRESS_LEN: usize = 255;
pub const MAX_STATUS_JSON_LEN: usize = 32767;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Handshaking,
    Status,
}

pub use error::ProtocolError;
pub use framing::{decode_frame, encode_frame};
pub use packets::{Handshake, Intent, Serverbound, decode_serverbound};
pub use primitives::{read_i64, read_string, read_u16, write_i64, write_string, write_u16};
pub use status::{StatusInfo, encode_pong, encode_status_response, status_json};
pub use varint::{read_var_int, read_var_long, write_var_int, write_var_long};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cible_26_3() {
        assert_eq!(
            (GAME_VERSION, PROTOCOL_VERSION, DATA_VERSION),
            ("26.3", 777, 5023)
        );
    }
}
