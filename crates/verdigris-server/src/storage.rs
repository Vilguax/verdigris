use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tracing::error;
use verdigris_protocol::{ChunkColumn, ChunkLoader, decode_chunk, encode_chunk};

pub const WORLD_ENV: &str = "VERDIGRIS_WORLD";
pub const DEFAULT_WORLD_DIR: &str = "world";
pub const CHUNKS_SUBDIR: &str = "chunks";
pub const REJECTED_SUFFIX: &str = ".rejete";

static TMP_NONCE: AtomicU64 = AtomicU64::new(0);

pub fn world_dir() -> PathBuf {
    std::env::var_os(WORLD_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_WORLD_DIR))
}

pub enum LoadOutcome {
    Loaded(ChunkColumn),
    Absent,
    Rejected,
}

pub struct ChunkStore {
    base: PathBuf,
}

impl ChunkStore {
    pub fn from_env() -> Self {
        ChunkStore { base: world_dir() }
    }

    pub fn new(base: impl Into<PathBuf>) -> Self {
        ChunkStore { base: base.into() }
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    pub fn chunks_dir(&self) -> PathBuf {
        self.base.join(CHUNKS_SUBDIR)
    }

    pub fn chunk_path(&self, x: i32, z: i32) -> PathBuf {
        self.chunks_dir().join(format!("{x}.{z}.bin"))
    }

    pub fn save_chunk(&self, x: i32, z: i32, column: &ChunkColumn) -> io::Result<()> {
        let dir = self.chunks_dir();
        fs::create_dir_all(&dir)?;
        let bytes = encode_chunk(column);
        let nonce = TMP_NONCE.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!("{x}.{z}.bin.tmp.{}-{}", std::process::id(), nonce));

        if let Err(err) = write_all_synced(&tmp, &bytes) {
            let _ = fs::remove_file(&tmp);
            return Err(err);
        }

        let final_path = dir.join(format!("{x}.{z}.bin"));
        if let Err(err) = fs::rename(&tmp, &final_path) {
            let _ = fs::remove_file(&tmp);
            return Err(err);
        }
        Ok(())
    }

    pub fn load_chunk(&self, x: i32, z: i32) -> LoadOutcome {
        let path = self.chunk_path(x, z);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return LoadOutcome::Absent,
            Err(err) => {
                error!(x, z, erreur = %err, "lecture du chunk impossible, rejet");
                self.reject(&path);
                return LoadOutcome::Rejected;
            }
        };
        match decode_chunk(&bytes) {
            Ok(column) => LoadOutcome::Loaded(column),
            Err(err) => {
                error!(x, z, cause = %err, "chunk illisible, rejet");
                self.reject(&path);
                LoadOutcome::Rejected
            }
        }
    }

    fn reject(&self, path: &Path) {
        let rejected = rejected_path(path);
        if let Err(err) = fs::rename(path, &rejected) {
            error!(erreur = %err, chemin = %path.display(), "renommage en .rejete impossible");
        }
    }
}

fn write_all_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn rejected_path(path: &Path) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(REJECTED_SUFFIX);
    PathBuf::from(name)
}

pub struct DiskLoader {
    store: Arc<ChunkStore>,
}

impl DiskLoader {
    pub fn new(store: Arc<ChunkStore>) -> Self {
        DiskLoader { store }
    }
}

impl ChunkLoader for DiskLoader {
    fn load(&self, chunk_x: i32, chunk_z: i32) -> Option<ChunkColumn> {
        match self.store.load_chunk(chunk_x, chunk_z) {
            LoadOutcome::Loaded(column) => Some(column),
            LoadOutcome::Absent | LoadOutcome::Rejected => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verdigris_protocol::{FlatGenerator, World};

    static DIR_SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_base(tag: &str) -> PathBuf {
        let seq = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "verdigris-store-{}-{tag}-{seq}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn sample_column() -> ChunkColumn {
        let mut column = ChunkColumn::empty(4);
        column.sections[0].states[0] = 1;
        column.sections[0].states[10] = 89;
        column.sections[1].biomes[0] = 7;
        column
    }

    fn flat_world(store: &Arc<ChunkStore>) -> World {
        World::with_loader(
            Box::new(FlatGenerator::new().unwrap()),
            Box::new(DiskLoader::new(Arc::clone(store))),
        )
    }

    fn dir_entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn save_then_load_round_trips() {
        let store = ChunkStore::new(temp_base("rt"));
        let column = sample_column();
        store.save_chunk(3, -5, &column).unwrap();
        match store.load_chunk(3, -5) {
            LoadOutcome::Loaded(loaded) => assert_eq!(loaded, column),
            _ => panic!("chunk attendu rechargé"),
        }
    }

    #[test]
    fn absent_chunk_reports_absent() {
        let store = ChunkStore::new(temp_base("absent"));
        assert!(matches!(store.load_chunk(0, 0), LoadOutcome::Absent));
    }

    #[test]
    fn save_is_atomic_and_leaves_no_tmp() {
        let store = ChunkStore::new(temp_base("atomic"));
        store.save_chunk(0, 0, &sample_column()).unwrap();
        assert_eq!(
            dir_entries(&store.chunks_dir()),
            vec!["0.0.bin".to_string()]
        );

        store.save_chunk(0, 0, &sample_column()).unwrap();
        let entries = dir_entries(&store.chunks_dir());
        assert_eq!(entries, vec!["0.0.bin".to_string()]);
        assert!(entries.iter().all(|name| !name.contains(".tmp")));

        match store.load_chunk(0, 0) {
            LoadOutcome::Loaded(loaded) => assert_eq!(loaded, sample_column()),
            _ => panic!("re-sauvegarde illisible"),
        }
    }

    #[test]
    fn wrong_version_is_rejected_and_regenerated() {
        let store = ChunkStore::new(temp_base("version"));
        store.save_chunk(1, 1, &sample_column()).unwrap();
        let path = store.chunk_path(1, 1);
        let mut bytes = fs::read(&path).unwrap();
        bytes[4] = 0xFF;
        bytes[5] = 0xFF;
        fs::write(&path, &bytes).unwrap();

        assert!(matches!(store.load_chunk(1, 1), LoadOutcome::Rejected));
        assert!(!path.exists(), "le .bin corrompu est renommé");
        assert!(rejected_path(&path).exists(), "le .rejete est présent");
    }

    #[test]
    fn truncated_file_is_rejected() {
        let store = ChunkStore::new(temp_base("trunc"));
        store.save_chunk(2, 2, &sample_column()).unwrap();
        let path = store.chunk_path(2, 2);
        fs::write(&path, b"VR").unwrap();

        assert!(matches!(store.load_chunk(2, 2), LoadOutcome::Rejected));
        assert!(!path.exists());
        assert!(rejected_path(&path).exists());
    }

    #[test]
    fn reject_overwrites_existing_rejete() {
        let store = ChunkStore::new(temp_base("twice"));
        let path = store.chunk_path(0, 0);

        store.save_chunk(0, 0, &sample_column()).unwrap();
        fs::write(&path, b"VR").unwrap();
        assert!(matches!(store.load_chunk(0, 0), LoadOutcome::Rejected));
        assert!(rejected_path(&path).exists());

        store.save_chunk(0, 0, &sample_column()).unwrap();
        fs::write(&path, b"xyz").unwrap();
        assert!(matches!(store.load_chunk(0, 0), LoadOutcome::Rejected));
        assert!(rejected_path(&path).exists());
        assert!(!path.exists());
    }

    #[test]
    fn flush_skips_untouched_chunks() {
        let store = Arc::new(ChunkStore::new(temp_base("clean")));
        let world = flat_world(&store);
        let _ = world.chunk(0, 0);
        let _ = world.chunk(1, -1);

        let written = crate::flush_dirty(&world, &store);
        assert_eq!(written, 0);
        let chunks = store.chunks_dir();
        assert!(!chunks.exists() || dir_entries(&chunks).is_empty());
    }

    #[test]
    fn flush_persists_modified_chunk() {
        let store = Arc::new(ChunkStore::new(temp_base("dirty")));
        let world = flat_world(&store);
        world.set_state(0, -60, 0, 10);

        let written = crate::flush_dirty(&world, &store);
        assert_eq!(written, 1);
        assert!(store.chunk_path(0, 0).exists());
        assert_eq!(world.dirty_len(), 0);

        let reopened = Arc::new(ChunkStore::new(store.base()));
        let restarted = flat_world(&reopened);
        assert_eq!(restarted.state_at(0, -60, 0), 10);
        assert_eq!(restarted.state_at(32, -60, 32), 0);
    }
}
