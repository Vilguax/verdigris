use crate::MAX_PACKET_LEN;
use crate::error::ProtocolError;
use crate::varint::{read_var_int, write_var_int};

pub fn decode_frame(buf: &[u8]) -> Result<Option<(&[u8], usize)>, ProtocolError> {
    let (len, header) = match read_var_int(buf)? {
        Some(v) => v,
        None => return Ok(None),
    };
    if len < 0 {
        return Err(ProtocolError::FrameLengthNegative);
    }
    let len = len as usize;
    if len > MAX_PACKET_LEN {
        return Err(ProtocolError::FrameLengthTooLarge);
    }
    let total = header + len;
    if buf.len() < total {
        return Ok(None);
    }
    Ok(Some((&buf[header..total], total)))
}

pub fn encode_frame(packet_bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(packet_bytes.len() + 5);
    write_var_int(packet_bytes.len() as i32, &mut out);
    out.extend_from_slice(packet_bytes);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip() {
        let payload = [0x00, 0xDE, 0xAD, 0xBE, 0xEF];
        let framed = encode_frame(&payload);
        let (body, consumed) = decode_frame(&framed).unwrap().unwrap();
        assert_eq!(body, &payload);
        assert_eq!(consumed, framed.len());
    }

    #[test]
    fn frame_partial_is_incomplete() {
        let payload = [0x00, 0x01, 0x02, 0x03];
        let framed = encode_frame(&payload);
        assert_eq!(decode_frame(&framed[..3]).unwrap(), None);
    }

    #[test]
    fn frame_length_only_is_incomplete() {
        let framed = encode_frame(&[0u8; 10]);
        assert_eq!(decode_frame(&framed[..1]).unwrap(), None);
    }

    #[test]
    fn frame_negative_length_rejected() {
        let mut buf = Vec::new();
        write_var_int(-5, &mut buf);
        assert_eq!(decode_frame(&buf), Err(ProtocolError::FrameLengthNegative));
    }

    #[test]
    fn frame_too_large_rejected() {
        let mut buf = Vec::new();
        write_var_int((MAX_PACKET_LEN + 1) as i32, &mut buf);
        assert_eq!(decode_frame(&buf), Err(ProtocolError::FrameLengthTooLarge));
    }

    #[test]
    fn frame_boundary_2097151_accepted() {
        let payload = vec![0u8; 2_097_151];
        let framed = encode_frame(&payload);
        let (body, consumed) = decode_frame(&framed).unwrap().unwrap();
        assert_eq!(body.len(), 2_097_151);
        assert_eq!(consumed, framed.len());
    }

    #[test]
    fn frame_boundary_2097152_rejected() {
        let mut buf = Vec::new();
        write_var_int(2_097_152, &mut buf);
        assert_eq!(decode_frame(&buf), Err(ProtocolError::FrameLengthTooLarge));
    }

    #[test]
    fn frame_length_four_byte_varint_rejected() {
        let buf = [0x80, 0x80, 0x80, 0x01];
        assert_eq!(decode_frame(&buf), Err(ProtocolError::FrameLengthTooLarge));
    }

    #[test]
    fn frame_trailing_bytes_not_consumed() {
        let payload = [0x00, 0x01];
        let mut framed = encode_frame(&payload);
        framed.extend_from_slice(&[0xAA, 0xBB]);
        let (body, consumed) = decode_frame(&framed).unwrap().unwrap();
        assert_eq!(body, &payload);
        assert_eq!(consumed, payload.len() + 1);
    }
}
