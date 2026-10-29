use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::chunk::{
    BIOMES_PER_SECTION, BLOCKS_PER_SECTION, ChunkColumn, SECTION_COUNT, Section, bits_for,
    pack_longs, unpack_longs,
};
use crate::primitives::write_i64;
use crate::varint::write_var_int;

pub const MAGIC: [u8; 4] = *b"VRDG";
pub const FORMAT_VERSION: u16 = 1;

const MAX_BODY_LEN: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkFileError {
    BadMagic,
    WrongFormatVersion { found: u16 },
    WrongGameVersion { found: String },
    Truncated,
    Corrupt(String),
}

impl core::fmt::Display for ChunkFileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ChunkFileError::BadMagic => write!(f, "nombre magique invalide"),
            ChunkFileError::WrongFormatVersion { found } => write!(
                f,
                "version de format inattendue: {found} (attendu {FORMAT_VERSION})"
            ),
            ChunkFileError::WrongGameVersion { found } => write!(
                f,
                "version de jeu inattendue: {found} (attendu {})",
                crate::GAME_VERSION
            ),
            ChunkFileError::Truncated => write!(f, "données tronquées"),
            ChunkFileError::Corrupt(reason) => write!(f, "données corrompues: {reason}"),
        }
    }
}

impl std::error::Error for ChunkFileError {}

pub fn encode_chunk(column: &ChunkColumn) -> Vec<u8> {
    let mut body = Vec::new();
    for section in &column.sections {
        let states: Vec<u32> = section.states.iter().map(|&state| state as u32).collect();
        write_disk_container(&states, &mut body);
        let biomes: Vec<u32> = section.biomes.iter().map(|&biome| biome as u32).collect();
        write_disk_container(&biomes, &mut body);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
    let game = crate::GAME_VERSION.as_bytes();
    write_var_int(game.len() as i32, &mut out);
    out.extend_from_slice(game);
    out.extend_from_slice(&deflate(&body));
    out
}

pub fn decode_chunk(bytes: &[u8]) -> Result<ChunkColumn, ChunkFileError> {
    let mut reader = Reader::new(bytes);

    let magic = reader.read_bytes(MAGIC.len())?;
    if magic != &MAGIC[..] {
        return Err(ChunkFileError::BadMagic);
    }

    let version = reader.read_u16()?;
    if version != FORMAT_VERSION {
        return Err(ChunkFileError::WrongFormatVersion { found: version });
    }

    let name_len = reader.read_var_int()?;
    if name_len < 0 {
        return Err(ChunkFileError::Corrupt(
            "longueur de version négative".to_owned(),
        ));
    }
    let name_bytes = reader.read_bytes(name_len as usize)?;
    let game = std::str::from_utf8(name_bytes)
        .map_err(|_| ChunkFileError::Corrupt("version de jeu non utf8".to_owned()))?;
    if game != crate::GAME_VERSION {
        return Err(ChunkFileError::WrongGameVersion {
            found: game.to_owned(),
        });
    }

    let rest = reader.remaining();
    let compressed = reader.read_bytes(rest)?;
    let body = inflate(compressed)?;

    let mut body_reader = Reader::new(&body);
    let mut sections = Vec::with_capacity(SECTION_COUNT);
    for _ in 0..SECTION_COUNT {
        let states = read_disk_container(&mut body_reader, BLOCKS_PER_SECTION)?;
        let biomes = read_disk_container(&mut body_reader, BIOMES_PER_SECTION)?;
        let mut state_cells = [0u16; BLOCKS_PER_SECTION];
        state_cells.copy_from_slice(&states);
        let mut biome_cells = [0u16; BIOMES_PER_SECTION];
        biome_cells.copy_from_slice(&biomes);
        sections.push(Section::from_cells(&state_cells, &biome_cells));
    }

    Ok(ChunkColumn::from_sections(sections))
}

fn write_disk_container(entries: &[u32], out: &mut Vec<u8>) {
    let mut palette: Vec<u32> = Vec::new();
    for &entry in entries {
        if !palette.contains(&entry) {
            palette.push(entry);
        }
    }

    write_var_int(palette.len() as i32, out);
    for &id in &palette {
        write_var_int(id as i32, out);
    }

    let bits = bits_for(palette.len());
    out.push(bits);

    let longs = if bits == 0 {
        Vec::new()
    } else {
        let indices: Vec<u32> = entries
            .iter()
            .map(|entry| palette.iter().position(|value| value == entry).unwrap() as u32)
            .collect();
        pack_longs(&indices, bits)
    };
    write_var_int(longs.len() as i32, out);
    for long in longs {
        write_i64(long, out);
    }
}

fn read_disk_container(reader: &mut Reader, count: usize) -> Result<Vec<u16>, ChunkFileError> {
    let palette_len = read_len(reader, count)?;
    let mut palette = Vec::with_capacity(palette_len);
    for _ in 0..palette_len {
        let id = reader.read_var_int()?;
        if id < 0 {
            return Err(ChunkFileError::Corrupt(
                "identifiant de palette négatif".to_owned(),
            ));
        }
        palette.push(id as u16);
    }

    let bits = reader.read_u8()?;
    if bits as usize > 32 {
        return Err(ChunkFileError::Corrupt(format!(
            "largeur de bits invalide: {bits}"
        )));
    }

    let longs_raw = reader.read_var_int()?;
    if longs_raw < 0 {
        return Err(ChunkFileError::Corrupt(
            "nombre de longs négatif".to_owned(),
        ));
    }
    let longs_len = longs_raw as usize;
    let expected = if bits == 0 {
        0
    } else {
        count.div_ceil(64 / bits as usize)
    };
    if longs_len != expected {
        return Err(ChunkFileError::Corrupt(format!(
            "nombre de longs {longs_len}, attendu {expected}"
        )));
    }

    let mut data = Vec::with_capacity(longs_len);
    for _ in 0..longs_len {
        data.push(reader.read_i64()?);
    }

    let indices = unpack_longs(&data, bits, count);
    let mut cells = Vec::with_capacity(count);
    for index in indices {
        let id = *palette.get(index as usize).ok_or_else(|| {
            ChunkFileError::Corrupt(format!("index de palette hors limites: {index}"))
        })?;
        cells.push(id);
    }
    Ok(cells)
}

fn read_len(reader: &mut Reader, max: usize) -> Result<usize, ChunkFileError> {
    let raw = reader.read_var_int()?;
    if raw < 0 || raw as usize > max {
        return Err(ChunkFileError::Corrupt(format!(
            "taille de palette invalide: {raw}"
        )));
    }
    Ok(raw as usize)
}

fn deflate(input: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(input)
        .expect("écriture en mémoire infaillible");
    encoder
        .finish()
        .expect("finalisation en mémoire infaillible")
}

fn inflate(input: &[u8]) -> Result<Vec<u8>, ChunkFileError> {
    use std::io::Read;
    let mut decoder = ZlibDecoder::new(input).take(MAX_BODY_LEN as u64 + 1);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|_| ChunkFileError::Corrupt("flux zlib invalide".to_owned()))?;
    if out.len() > MAX_BODY_LEN {
        return Err(ChunkFileError::Corrupt(
            "flux décompressé trop volumineux".to_owned(),
        ));
    }
    Ok(out)
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], ChunkFileError> {
        if self.remaining() < n {
            return Err(ChunkFileError::Truncated);
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, ChunkFileError> {
        Ok(self.read_bytes(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16, ChunkFileError> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn read_i64(&mut self) -> Result<i64, ChunkFileError> {
        let bytes = self.read_bytes(8)?;
        let mut array = [0u8; 8];
        array.copy_from_slice(bytes);
        Ok(i64::from_be_bytes(array))
    }

    fn read_var_int(&mut self) -> Result<i32, ChunkFileError> {
        let mut value: i32 = 0;
        let mut shift: u32 = 0;
        loop {
            if shift >= 35 {
                return Err(ChunkFileError::Corrupt("varint trop long".to_owned()));
            }
            let byte = self.read_u8()?;
            value |= ((byte & 0x7F) as i32) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::block_index;

    fn sample_column() -> ChunkColumn {
        let mut column = ChunkColumn::empty(4);
        column.sections[0].states[0] = 1;
        column.sections[0].states[1] = 89;
        column.sections[0].states[block_index(5, 3, 2)] = 1234;
        column.sections[0].biomes[0] = 7;
        column.sections[0].biomes[biome_index_local()] = 11;
        column.sections[2].states[block_index(15, 15, 15)] = 50;
        column.sections[2].biomes[10] = 3;
        for cell in 0..300 {
            column.sections[5].states[cell] = (cell as u16) + 1;
        }
        column.sections[23].states[42] = 35722;
        column
    }

    fn biome_index_local() -> usize {
        crate::chunk::biome_index(1, 2, 3)
    }

    #[test]
    fn round_trip_preserves_every_cell() {
        let column = sample_column();
        let encoded = encode_chunk(&column);
        let decoded = decode_chunk(&encoded).unwrap();
        assert_eq!(decoded, column);
    }

    #[test]
    fn empty_column_round_trips() {
        let column = ChunkColumn::empty(41);
        let decoded = decode_chunk(&encode_chunk(&column)).unwrap();
        assert_eq!(decoded, column);
    }

    #[test]
    fn wrong_format_version_is_reported() {
        let mut encoded = encode_chunk(&sample_column());
        encoded[4] = 0xFF;
        encoded[5] = 0xFF;
        assert_eq!(
            decode_chunk(&encoded),
            Err(ChunkFileError::WrongFormatVersion { found: 0xFFFF })
        );
    }

    #[test]
    fn wrong_game_version_is_reported() {
        let mut encoded = encode_chunk(&sample_column());
        encoded[7] = b'X';
        match decode_chunk(&encoded) {
            Err(ChunkFileError::WrongGameVersion { found }) => assert_eq!(found, "X6.3"),
            other => panic!("attendu WrongGameVersion, obtenu {other:?}"),
        }
    }

    #[test]
    fn bad_magic_is_reported() {
        let mut encoded = encode_chunk(&sample_column());
        encoded[0] = b'X';
        assert_eq!(decode_chunk(&encoded), Err(ChunkFileError::BadMagic));
    }

    #[test]
    fn truncated_header_does_not_panic() {
        let encoded = encode_chunk(&sample_column());
        assert_eq!(decode_chunk(&encoded[..3]), Err(ChunkFileError::Truncated));
    }

    #[test]
    fn truncated_body_does_not_panic() {
        let encoded = encode_chunk(&sample_column());
        let cut = &encoded[..encoded.len() - 1];
        match decode_chunk(cut) {
            Err(ChunkFileError::Corrupt(_)) | Err(ChunkFileError::Truncated) => {}
            other => panic!("attendu Corrupt ou Truncated, obtenu {other:?}"),
        }
    }

    #[test]
    fn empty_input_is_truncated() {
        assert_eq!(decode_chunk(&[]), Err(ChunkFileError::Truncated));
    }
}
