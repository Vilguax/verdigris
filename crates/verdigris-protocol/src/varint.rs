use crate::error::ProtocolError;

pub fn read_var_int(buf: &[u8]) -> Result<Option<(i32, usize)>, ProtocolError> {
    let mut value: u32 = 0;
    let mut position: u32 = 0;
    let mut index: usize = 0;
    loop {
        let byte = match buf.get(index) {
            Some(b) => *b,
            None => return Ok(None),
        };
        index += 1;
        if position == 28 && (byte & 0x7F) > 0x0F {
            return Err(ProtocolError::VarIntOverflow);
        }
        value |= ((byte & 0x7F) as u32) << position;
        if byte & 0x80 == 0 {
            return Ok(Some((value as i32, index)));
        }
        position += 7;
        if position >= 35 {
            return Err(ProtocolError::VarIntTooLong);
        }
    }
}

pub fn write_var_int(value: i32, out: &mut Vec<u8>) {
    let mut v = value as u32;
    loop {
        if v & !0x7F == 0 {
            out.push(v as u8);
            return;
        }
        out.push(((v & 0x7F) | 0x80) as u8);
        v >>= 7;
    }
}

pub fn read_var_long(buf: &[u8]) -> Result<Option<(i64, usize)>, ProtocolError> {
    let mut value: u64 = 0;
    let mut position: u32 = 0;
    let mut index: usize = 0;
    loop {
        let byte = match buf.get(index) {
            Some(b) => *b,
            None => return Ok(None),
        };
        index += 1;
        if position == 63 && (byte & 0x7F) > 0x01 {
            return Err(ProtocolError::VarIntOverflow);
        }
        value |= ((byte & 0x7F) as u64) << position;
        if byte & 0x80 == 0 {
            return Ok(Some((value as i64, index)));
        }
        position += 7;
        if position >= 70 {
            return Err(ProtocolError::VarIntTooLong);
        }
    }
}

pub fn write_var_long(value: i64, out: &mut Vec<u8>) {
    let mut v = value as u64;
    loop {
        if v & !0x7F == 0 {
            out.push(v as u8);
            return;
        }
        out.push(((v & 0x7F) | 0x80) as u8);
        v >>= 7;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip_var_int(value: i32) {
        let mut buf = Vec::new();
        write_var_int(value, &mut buf);
        let (decoded, consumed) = read_var_int(&buf).unwrap().unwrap();
        assert_eq!(decoded, value);
        assert_eq!(consumed, buf.len());
    }

    #[test]
    fn var_int_round_trip() {
        for value in [0, 1, 2, 127, 128, 255, 2147483647, -1, -2147483648] {
            roundtrip_var_int(value);
        }
    }

    #[test]
    fn var_int_five_bytes_valid() {
        let buf = [0xFF, 0xFF, 0xFF, 0xFF, 0x0F];
        let (decoded, consumed) = read_var_int(&buf).unwrap().unwrap();
        assert_eq!(decoded, -1);
        assert_eq!(consumed, 5);
    }

    #[test]
    fn var_int_six_bytes_rejected() {
        let buf = [0x80, 0x80, 0x80, 0x80, 0x80, 0x00];
        assert_eq!(read_var_int(&buf), Err(ProtocolError::VarIntTooLong));
    }

    #[test]
    fn var_int_last_byte_overflow_rejected() {
        let buf = [0xFF, 0xFF, 0xFF, 0xFF, 0x7F];
        assert_eq!(read_var_int(&buf), Err(ProtocolError::VarIntOverflow));
    }

    #[test]
    fn var_int_truncated_is_incomplete() {
        let buf = [0xFF, 0xFF];
        assert_eq!(read_var_int(&buf), Ok(None));
    }

    #[test]
    fn var_int_empty_is_incomplete() {
        assert_eq!(read_var_int(&[]), Ok(None));
    }

    fn roundtrip_var_long(value: i64) {
        let mut buf = Vec::new();
        write_var_long(value, &mut buf);
        let (decoded, consumed) = read_var_long(&buf).unwrap().unwrap();
        assert_eq!(decoded, value);
        assert_eq!(consumed, buf.len());
    }

    #[test]
    fn var_long_round_trip() {
        for value in [
            0,
            1,
            127,
            128,
            9223372036854775807,
            -1,
            -9223372036854775808,
        ] {
            roundtrip_var_long(value);
        }
    }

    #[test]
    fn var_long_ten_bytes_valid() {
        let buf = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01];
        let (decoded, _) = read_var_long(&buf).unwrap().unwrap();
        assert_eq!(decoded, -1);
    }

    #[test]
    fn var_long_eleven_bytes_rejected() {
        let buf = [
            0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00,
        ];
        assert_eq!(read_var_long(&buf), Err(ProtocolError::VarIntTooLong));
    }

    #[test]
    fn var_long_last_byte_overflow_rejected() {
        let buf = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F];
        assert_eq!(read_var_long(&buf), Err(ProtocolError::VarIntOverflow));
    }

    #[test]
    fn var_long_truncated_is_incomplete() {
        let buf = [0xFF, 0xFF, 0xFF];
        assert_eq!(read_var_long(&buf), Ok(None));
    }
}
