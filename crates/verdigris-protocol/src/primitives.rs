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

pub fn read_i16(buf: &[u8]) -> Result<Option<(i16, usize)>, ProtocolError> {
    if buf.len() < 2 {
        return Ok(None);
    }
    Ok(Some((i16::from_be_bytes([buf[0], buf[1]]), 2)))
}

pub fn write_i16(value: i16, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub fn pack_degrees(deg: f32) -> u8 {
    ((deg as f64 * 256.0 / 360.0).floor() as i32) as u8
}

pub fn quantize(v: f64) -> i64 {
    (v * 4096.0 + 0.5).floor() as i64
}

pub fn fits_i16(d: i64) -> bool {
    (-32768..=32767).contains(&d)
}

pub fn read_i32(buf: &[u8]) -> Result<Option<(i32, usize)>, ProtocolError> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&buf[..4]);
    Ok(Some((i32::from_be_bytes(bytes), 4)))
}

pub fn write_i32(value: i32, out: &mut Vec<u8>) {
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

pub fn read_f32(buf: &[u8]) -> Result<Option<(f32, usize)>, ProtocolError> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&buf[..4]);
    Ok(Some((f32::from_be_bytes(bytes), 4)))
}

pub fn write_f32(value: f32, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub fn read_f64(buf: &[u8]) -> Result<Option<(f64, usize)>, ProtocolError> {
    if buf.len() < 8 {
        return Ok(None);
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&buf[..8]);
    Ok(Some((f64::from_be_bytes(bytes), 8)))
}

pub fn write_f64(value: f64, out: &mut Vec<u8>) {
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
    fn i16_round_trip() {
        let mut buf = Vec::new();
        write_i16(4096, &mut buf);
        assert_eq!(buf, vec![0x10, 0x00]);
        assert_eq!(read_i16(&buf).unwrap().unwrap(), (4096, 2));
    }

    #[test]
    fn pack_degrees_vectors() {
        assert_eq!(pack_degrees(0.0), 0x00);
        assert_eq!(pack_degrees(90.0), 0x40);
        assert_eq!(pack_degrees(-90.0), 0xC0);
        assert_eq!(pack_degrees(180.0), 0x80);
        assert_eq!(pack_degrees(-180.0), 0x80);
        assert_eq!(pack_degrees(45.5), 0x20);
        assert_eq!(pack_degrees(-0.1), 0xFF);
        assert_eq!(pack_degrees(359.9), 0xFF);
        assert_eq!(pack_degrees(370.0), 0x07);
    }

    #[test]
    fn quantize_negative_halves_floor_not_round() {
        assert_eq!(quantize(-0.5 / 4096.0), 0);
        assert_eq!(quantize(-1.5 / 4096.0), -1);
        assert_eq!(quantize(0.5 / 4096.0), 1);
        assert_eq!(quantize(1.0), 4096);
        assert_eq!(quantize(-1.0), -4096);
    }

    #[test]
    fn fits_i16_bounds() {
        assert!(fits_i16(-32768));
        assert!(fits_i16(32767));
        assert!(!fits_i16(-32769));
        assert!(!fits_i16(32768));
    }

    #[test]
    fn i32_round_trip() {
        let mut buf = Vec::new();
        write_i32(-559038737, &mut buf);
        assert_eq!(buf, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(read_i32(&buf).unwrap().unwrap(), (-559038737, 4));
    }

    #[test]
    fn i32_incomplete() {
        assert_eq!(read_i32(&[0, 0, 0]).unwrap(), None);
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
    fn f32_round_trip() {
        let mut buf = Vec::new();
        write_f32(1.5, &mut buf);
        assert_eq!(buf, vec![0x3F, 0xC0, 0x00, 0x00]);
        let (value, consumed) = read_f32(&buf).unwrap().unwrap();
        assert_eq!(value, 1.5);
        assert_eq!(consumed, 4);
    }

    #[test]
    fn f32_incomplete() {
        assert_eq!(read_f32(&[0, 0, 0]).unwrap(), None);
    }

    #[test]
    fn f64_round_trip() {
        let mut buf = Vec::new();
        write_f64(100.0, &mut buf);
        assert_eq!(buf, vec![0x40, 0x59, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        let (value, consumed) = read_f64(&buf).unwrap().unwrap();
        assert_eq!(value, 100.0);
        assert_eq!(consumed, 8);
    }

    #[test]
    fn f64_incomplete() {
        assert_eq!(read_f64(&[0, 0, 0, 0, 0, 0, 0]).unwrap(), None);
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
