use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::{Compression, Decompress, FlushDecompress, Status};

use crate::MAX_PACKET_LEN;
use crate::error::ProtocolError;
use crate::varint::{read_var_int, write_var_int};

pub const MAX_UNCOMPRESSED_LEN: usize = 1 << 23;

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

pub fn decode_compressed_frame(
    buf: &[u8],
    threshold: usize,
) -> Result<Option<(Vec<u8>, usize)>, ProtocolError> {
    let (packet_len, header) = match read_var_int(buf)? {
        Some(v) => v,
        None => return Ok(None),
    };
    if packet_len < 0 {
        return Err(ProtocolError::FrameLengthNegative);
    }
    let packet_len = packet_len as usize;
    if packet_len > MAX_PACKET_LEN {
        return Err(ProtocolError::FrameLengthTooLarge);
    }
    let total = header + packet_len;
    if buf.len() < total {
        return Ok(None);
    }
    let frame = &buf[header..total];

    let (data_len, data_len_bytes) = match read_var_int(frame)? {
        Some(v) => v,
        None => return Err(ProtocolError::CompressedPacketLengthTooSmall),
    };
    if data_len < 0 {
        return Err(ProtocolError::FrameLengthNegative);
    }
    let data_len = data_len as usize;
    let body = &frame[data_len_bytes..];

    if data_len == 0 {
        return Ok(Some((body.to_vec(), total)));
    }
    if data_len < threshold {
        return Err(ProtocolError::CompressedDataLengthBelowThreshold);
    }
    if data_len > MAX_UNCOMPRESSED_LEN {
        return Err(ProtocolError::CompressedDataLengthTooLarge);
    }

    let packet = inflate_bounded(body, data_len)?;
    Ok(Some((packet, total)))
}

pub fn encode_compressed_frame(packet_bytes: &[u8], threshold: usize) -> Vec<u8> {
    let mut frame = Vec::new();
    if packet_bytes.len() >= threshold {
        write_var_int(packet_bytes.len() as i32, &mut frame);
        frame.extend_from_slice(&deflate(packet_bytes));
    } else {
        write_var_int(0, &mut frame);
        frame.extend_from_slice(packet_bytes);
    }
    let mut out = Vec::with_capacity(frame.len() + 5);
    write_var_int(frame.len() as i32, &mut out);
    out.extend_from_slice(&frame);
    out
}

fn inflate_bounded(input: &[u8], expected: usize) -> Result<Vec<u8>, ProtocolError> {
    let mut out = vec![0u8; expected + 1];
    let mut decoder = Decompress::new(true);
    let status = decoder
        .decompress(input, &mut out, FlushDecompress::Finish)
        .map_err(|_| ProtocolError::CompressedStreamInvalid)?;
    match status {
        Status::StreamEnd => {}
        Status::Ok | Status::BufError => {
            if decoder.total_out() as usize > expected {
                return Err(ProtocolError::CompressedSizeMismatch);
            }
            return Err(ProtocolError::CompressedStreamInvalid);
        }
    }
    let produced = decoder.total_out() as usize;
    if produced != expected {
        return Err(ProtocolError::CompressedSizeMismatch);
    }
    if decoder.total_in() as usize != input.len() {
        return Err(ProtocolError::CompressedTrailingBytes);
    }
    out.truncate(produced);
    Ok(out)
}

fn deflate(input: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(input)
        .expect("writing to an in-memory buffer is infallible");
    encoder
        .finish()
        .expect("finishing an in-memory buffer is infallible")
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

    const THRESHOLD: usize = 256;

    fn raw_zlib(data: &[u8]) -> Vec<u8> {
        deflate(data)
    }

    fn compressed_frame(data_len: i32, body: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        write_var_int(data_len, &mut frame);
        frame.extend_from_slice(body);
        let mut out = Vec::new();
        write_var_int(frame.len() as i32, &mut out);
        out.extend_from_slice(&frame);
        out
    }

    #[test]
    fn compressed_round_trip_uncompressed_body() {
        let packet = vec![0x11u8; 100];
        let framed = encode_compressed_frame(&packet, THRESHOLD);
        let (body, consumed) = decode_compressed_frame(&framed, THRESHOLD)
            .unwrap()
            .unwrap();
        assert_eq!(body, packet);
        assert_eq!(consumed, framed.len());
    }

    #[test]
    fn compressed_round_trip_compressed_body() {
        let packet = vec![0x22u8; 1000];
        let framed = encode_compressed_frame(&packet, THRESHOLD);
        let (body, consumed) = decode_compressed_frame(&framed, THRESHOLD)
            .unwrap()
            .unwrap();
        assert_eq!(body, packet);
        assert_eq!(consumed, framed.len());
    }

    #[test]
    fn compressed_boundary_255_stays_uncompressed() {
        let packet = vec![0x33u8; 255];
        let framed = encode_compressed_frame(&packet, THRESHOLD);
        let (_, header) = read_var_int(&framed).unwrap().unwrap();
        let (data_len, _) = read_var_int(&framed[header..]).unwrap().unwrap();
        assert_eq!(data_len, 0);
        let (body, consumed) = decode_compressed_frame(&framed, THRESHOLD)
            .unwrap()
            .unwrap();
        assert_eq!(body, packet);
        assert_eq!(consumed, framed.len());
    }

    #[test]
    fn compressed_boundary_256_is_compressed() {
        let packet = vec![0x44u8; 256];
        let framed = encode_compressed_frame(&packet, THRESHOLD);
        let (_, header) = read_var_int(&framed).unwrap().unwrap();
        let (data_len, _) = read_var_int(&framed[header..]).unwrap().unwrap();
        assert_eq!(data_len, 256);
        let (body, consumed) = decode_compressed_frame(&framed, THRESHOLD)
            .unwrap()
            .unwrap();
        assert_eq!(body, packet);
        assert_eq!(consumed, framed.len());
    }

    #[test]
    fn compressed_data_length_zero_accepted_above_threshold() {
        let body = vec![0x55u8; 300];
        let framed = compressed_frame(0, &body);
        let (packet, consumed) = decode_compressed_frame(&framed, THRESHOLD)
            .unwrap()
            .unwrap();
        assert_eq!(packet, body);
        assert_eq!(consumed, framed.len());
    }

    #[test]
    fn compressed_output_shorter_than_data_length_rejected() {
        let actual = vec![0x66u8; 256];
        let compressed = raw_zlib(&actual);
        let framed = compressed_frame(300, &compressed);
        assert_eq!(
            decode_compressed_frame(&framed, THRESHOLD),
            Err(ProtocolError::CompressedSizeMismatch)
        );
    }

    #[test]
    fn compressed_output_longer_than_data_length_rejected() {
        let actual = vec![0x77u8; 400];
        let compressed = raw_zlib(&actual);
        let framed = compressed_frame(256, &compressed);
        assert_eq!(
            decode_compressed_frame(&framed, THRESHOLD),
            Err(ProtocolError::CompressedSizeMismatch)
        );
    }

    #[test]
    fn compressed_truncated_stream_rejected() {
        let actual = vec![0x88u8; 300];
        let mut compressed = raw_zlib(&actual);
        compressed.truncate(compressed.len() - 1);
        let framed = compressed_frame(300, &compressed);
        assert_eq!(
            decode_compressed_frame(&framed, THRESHOLD),
            Err(ProtocolError::CompressedStreamInvalid)
        );
    }

    #[test]
    fn compressed_extra_bytes_after_stream_rejected() {
        let actual = vec![0x99u8; 300];
        let mut compressed = raw_zlib(&actual);
        compressed.push(0xFF);
        let framed = compressed_frame(300, &compressed);
        assert_eq!(
            decode_compressed_frame(&framed, THRESHOLD),
            Err(ProtocolError::CompressedTrailingBytes)
        );
    }

    #[test]
    fn compressed_zlib_bomb_rejected() {
        let actual = vec![0u8; 1_000_000];
        let compressed = raw_zlib(&actual);
        let framed = compressed_frame(256, &compressed);
        assert_eq!(
            decode_compressed_frame(&framed, THRESHOLD),
            Err(ProtocolError::CompressedSizeMismatch)
        );
    }

    #[test]
    fn compressed_packet_length_smaller_than_data_length_field() {
        let mut out = Vec::new();
        write_var_int(1, &mut out);
        out.push(0x80);
        assert_eq!(
            decode_compressed_frame(&out, THRESHOLD),
            Err(ProtocolError::CompressedPacketLengthTooSmall)
        );
    }

    #[test]
    fn compressed_below_threshold_rejected() {
        let actual = vec![0xABu8; 100];
        let compressed = raw_zlib(&actual);
        let framed = compressed_frame(100, &compressed);
        assert_eq!(
            decode_compressed_frame(&framed, THRESHOLD),
            Err(ProtocolError::CompressedDataLengthBelowThreshold)
        );
    }

    #[test]
    fn compressed_data_length_boundary_accepted() {
        let actual = vec![0u8; MAX_UNCOMPRESSED_LEN];
        let framed = compressed_frame(MAX_UNCOMPRESSED_LEN as i32, &raw_zlib(&actual));
        let (packet, consumed) = decode_compressed_frame(&framed, THRESHOLD)
            .unwrap()
            .unwrap();
        assert_eq!(packet.len(), MAX_UNCOMPRESSED_LEN);
        assert_eq!(consumed, framed.len());
    }

    #[test]
    fn compressed_data_length_over_boundary_rejected() {
        let framed = compressed_frame((MAX_UNCOMPRESSED_LEN + 1) as i32, &[0x01, 0x02, 0x03]);
        assert_eq!(
            decode_compressed_frame(&framed, THRESHOLD),
            Err(ProtocolError::CompressedDataLengthTooLarge)
        );
    }

    #[test]
    fn compressed_partial_then_complete() {
        let packet = vec![0xCDu8; 1000];
        let framed = encode_compressed_frame(&packet, THRESHOLD);
        assert_eq!(
            decode_compressed_frame(&framed[..framed.len() - 1], THRESHOLD).unwrap(),
            None
        );
        let (body, consumed) = decode_compressed_frame(&framed, THRESHOLD)
            .unwrap()
            .unwrap();
        assert_eq!(body, packet);
        assert_eq!(consumed, framed.len());
    }

    #[test]
    fn compressed_length_only_is_incomplete() {
        let packet = vec![0xEEu8; 1000];
        let framed = encode_compressed_frame(&packet, THRESHOLD);
        assert_eq!(
            decode_compressed_frame(&framed[..1], THRESHOLD).unwrap(),
            None
        );
    }
}
