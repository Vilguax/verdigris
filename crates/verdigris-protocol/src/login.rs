use std::hash::{BuildHasher, Hasher, RandomState};

use crate::MAX_USERNAME_LEN;
use crate::primitives::write_string;
use crate::varint::write_var_int;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidName {
    Empty,
    TooLong,
    IllegalCharacter,
}

impl core::fmt::Display for InvalidName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            InvalidName::Empty => write!(f, "name is empty"),
            InvalidName::TooLong => write!(f, "name exceeds 16 characters"),
            InvalidName::IllegalCharacter => write!(f, "name contains an illegal character"),
        }
    }
}

impl std::error::Error for InvalidName {}

pub fn validate_name(name: &str) -> Result<(), InvalidName> {
    let count = name.chars().count();
    if count == 0 {
        return Err(InvalidName::Empty);
    }
    if count > MAX_USERNAME_LEN {
        return Err(InvalidName::TooLong);
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(InvalidName::IllegalCharacter);
    }
    Ok(())
}

pub fn offline_uuid(name: &str) -> u128 {
    let mut input = Vec::with_capacity(14 + name.len());
    input.extend_from_slice(b"OfflinePlayer:");
    input.extend_from_slice(name.as_bytes());
    let mut digest = md5(&input);
    digest[6] = (digest[6] & 0x0F) | 0x30;
    digest[8] = (digest[8] & 0x3F) | 0x80;
    u128::from_be_bytes(digest)
}

pub fn random_session_id() -> u128 {
    let hi = random_u64();
    let lo = random_u64();
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&hi.to_be_bytes());
    bytes[8..].copy_from_slice(&lo.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    u128::from_be_bytes(bytes)
}

pub fn login_success_payload(uuid: u128, username: &str, session_id: u128) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&uuid.to_be_bytes());
    write_string(username, &mut payload);
    write_var_int(0, &mut payload);
    payload.extend_from_slice(&session_id.to_be_bytes());
    payload
}

fn random_u64() -> u64 {
    let state = RandomState::new();
    let mut hasher = state.build_hasher();
    hasher.write_u8(0xA5);
    hasher.finish()
}

const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

fn md5(input: &[u8]) -> [u8; 16] {
    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    let mut msg = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in msg.as_chunks::<64>().0 {
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            let base = i * 4;
            *word = u32::from_le_bytes([
                chunk[base],
                chunk[base + 1],
                chunk[base + 2],
                chunk[base + 3],
            ]);
        }

        let mut a = a0;
        let mut b = b0;
        let mut c = c0;
        let mut d = d0;

        for i in 0..64 {
            let (f, g) = if i < 16 {
                ((b & c) | (!b & d), i)
            } else if i < 32 {
                ((d & b) | (!d & c), (5 * i + 1) % 16)
            } else if i < 48 {
                (b ^ c ^ d, (3 * i + 5) % 16)
            } else {
                (c ^ (b | !d), (7 * i) % 16)
            };
            let f = f.wrapping_add(a).wrapping_add(MD5_K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(MD5_S[i]));
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::read_string;
    use crate::varint::read_var_int;

    fn hex(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    #[test]
    fn md5_rfc_vectors() {
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            hex(&md5(b"message digest")),
            "f96b697d7cb7938d525a2f31aaf161d0"
        );
        assert_eq!(
            hex(&md5(b"The quick brown fox jumps over the lazy dog")),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
    }

    #[test]
    fn offline_uuid_notch_vector() {
        let expected = 0xb50ad385_829d_3141_a216_7e7d7539ba7fu128;
        assert_eq!(offline_uuid("Notch"), expected);
    }

    #[test]
    fn offline_uuid_jeb_vector() {
        let expected = 222494897209172451529644695220166637099u128;
        assert_eq!(offline_uuid("jeb_"), expected);
    }

    #[test]
    fn offline_uuid_sets_version_three_and_ietf_variant() {
        let bytes = offline_uuid("Verdi").to_be_bytes();
        assert_eq!(bytes[6] & 0xF0, 0x30);
        assert_eq!(bytes[8] & 0xC0, 0x80);
    }

    #[test]
    fn session_id_is_v4_ietf() {
        let bytes = random_session_id().to_be_bytes();
        assert_eq!(bytes[6] & 0xF0, 0x40);
        assert_eq!(bytes[8] & 0xC0, 0x80);
    }

    #[test]
    fn session_ids_are_distinct() {
        assert_ne!(random_session_id(), random_session_id());
    }

    #[test]
    fn validate_name_accepts_legal() {
        assert_eq!(validate_name("Verdi_42"), Ok(()));
        assert_eq!(validate_name("_"), Ok(()));
        assert_eq!(validate_name("a"), Ok(()));
        assert_eq!(validate_name("ABCDEFGHIJKLMNOP"), Ok(()));
    }

    #[test]
    fn validate_name_rejects_empty() {
        assert_eq!(validate_name(""), Err(InvalidName::Empty));
    }

    #[test]
    fn validate_name_rejects_too_long() {
        assert_eq!(validate_name(&"a".repeat(17)), Err(InvalidName::TooLong));
    }

    #[test]
    fn validate_name_rejects_illegal_character() {
        assert_eq!(
            validate_name("Bad Name"),
            Err(InvalidName::IllegalCharacter)
        );
        assert_eq!(validate_name("héros"), Err(InvalidName::IllegalCharacter));
        assert_eq!(validate_name("drop;"), Err(InvalidName::IllegalCharacter));
    }

    #[test]
    fn login_success_payload_layout() {
        let uuid = offline_uuid("Verdi");
        let session_id = random_session_id();
        let payload = login_success_payload(uuid, "Verdi", session_id);

        assert_eq!(&payload[..16], &uuid.to_be_bytes());

        let mut cursor = &payload[16..];
        let (name, consumed) = read_string(cursor, MAX_USERNAME_LEN).unwrap().unwrap();
        assert_eq!(name, "Verdi");
        cursor = &cursor[consumed..];

        let (properties, consumed) = read_var_int(cursor).unwrap().unwrap();
        assert_eq!(properties, 0);
        cursor = &cursor[consumed..];

        assert_eq!(cursor.len(), 16);
        assert_eq!(cursor, &session_id.to_be_bytes());
    }
}
