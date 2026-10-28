use crate::error::ProtocolError;
use crate::varint::{read_var_int, write_var_int};

pub fn read_u16(buf: &[u8]) -> Result<Option<(u16, usize)>, ProtocolError> {
    if buf.len() < 2 {
        return Ok(None);
    }
    Ok(Some((u16::from_be_bytes([buf[0], buf[1]]), 2)))
}

pub fn write_u16(value: u16, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub fn read_i64(buf: &[u8]) -> Result<Option<(i64, usize)>, ProtocolError> {
    if buf.len() < 8 {
        return Ok(None);
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&buf[..8]);
    Ok(Some((i64::from_be_bytes(bytes), 8)))
}

pub fn write_i64(value: i64, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub fn read_string(buf: &[u8], max_len: usize) -> Result<Option<(String, usize)>, ProtocolError> {
    let (len, header) = match read_var_int(buf)? {
        Some(v) => v,
        None => return Ok(None),
    };
    if len < 0 {
        return Err(ProtocolError::StringLengthNegative);
    }
    let len = len as usize;
    if len > max_len.saturating_mul(3) {
        return Err(ProtocolError::StringTooLong);
    }
    let total = header + len;
    if buf.len() < total {
        return Ok(None);
    }
    let slice = &buf[header..total];
    let text = core::str::from_utf8(slice).map_err(|_| ProtocolError::InvalidUtf8)?;
    if text.chars().count() > max_len {
        return Err(ProtocolError::StringTooLong);
    }
    Ok(Some((text.to_owned(), total)))
}

pub fn write_string(value: &str, out: &mut Vec<u8>) {
    write_var_int(value.len() as i32, out);
    out.extend_from_slice(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_STRING_LEN;

    #[test]
    fn u16_round_trip() {
        let mut buf = Vec::new();
        write_u16(25565, &mut buf);
        assert_eq!(read_u16(&buf).unwrap().unwrap(), (25565, 2));
    }

    #[test]
    fn u16_incomplete() {
        assert_eq!(read_u16(&[0x01]).unwrap(), None);
    }

    #[test]
    fn i64_round_trip() {
        let mut buf = Vec::new();
        write_i64(-123456789, &mut buf);
        assert_eq!(read_i64(&buf).unwrap().unwrap(), (-123456789, 8));
    }

    #[test]
    fn i64_incomplete() {
        assert_eq!(read_i64(&[0, 0, 0]).unwrap(), None);
    }

    #[test]
    fn string_round_trip() {
        let mut buf = Vec::new();
        write_string("localhost", &mut buf);
        let (decoded, consumed) = read_string(&buf, MAX_STRING_LEN).unwrap().unwrap();
        assert_eq!(decoded, "localhost");
        assert_eq!(consumed, buf.len());
    }

    #[test]
    fn string_incomplete() {
        let mut buf = Vec::new();
        write_string("localhost", &mut buf);
        buf.truncate(4);
        assert_eq!(read_string(&buf, MAX_STRING_LEN).unwrap(), None);
    }

    #[test]
    fn string_exceeds_generic_limit() {
        let text = "x".repeat(10);
        let mut buf = Vec::new();
        write_string(&text, &mut buf);
        assert_eq!(read_string(&buf, 5), Err(ProtocolError::StringTooLong));
    }

    #[test]
    fn string_exceeds_address_limit() {
        let text = "a".repeat(256);
        let mut buf = Vec::new();
        write_string(&text, &mut buf);
        assert_eq!(read_string(&buf, 255), Err(ProtocolError::StringTooLong));
    }

    #[test]
    fn string_invalid_utf8() {
        let mut buf = Vec::new();
        write_var_int(2, &mut buf);
        buf.extend_from_slice(&[0xFF, 0xFE]);
        assert_eq!(
            read_string(&buf, MAX_STRING_LEN),
            Err(ProtocolError::InvalidUtf8)
        );
    }

    #[test]
    fn string_negative_length() {
        let mut buf = Vec::new();
        write_var_int(-1, &mut buf);
        assert_eq!(
            read_string(&buf, MAX_STRING_LEN),
            Err(ProtocolError::StringLengthNegative)
        );
    }
}
