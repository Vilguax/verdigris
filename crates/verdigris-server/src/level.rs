use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub const LEVEL_FILE: &str = "level.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelConfig {
    pub generator: String,
    pub seed: i64,
    pub game_version: String,
}

#[derive(Debug)]
pub enum LevelError {
    Io(io::Error),
    Parse(String),
    Mismatch {
        field: &'static str,
        stored: String,
        requested: String,
    },
}

impl fmt::Display for LevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LevelError::Io(err) => write!(f, "{LEVEL_FILE}: {err}"),
            LevelError::Parse(msg) => write!(f, "{LEVEL_FILE} illisible: {msg}"),
            LevelError::Mismatch {
                field,
                stored,
                requested,
            } => write!(
                f,
                "{LEVEL_FILE}: {field} du monde ({stored}) incompatible avec la valeur demandée ({requested}); \
                 démarrage refusé"
            ),
        }
    }
}

impl std::error::Error for LevelError {}

impl From<io::Error> for LevelError {
    fn from(err: io::Error) -> Self {
        LevelError::Io(err)
    }
}

pub fn level_path(base: &Path) -> PathBuf {
    base.join(LEVEL_FILE)
}

pub fn serialize(config: &LevelConfig) -> String {
    let body = serde_json::json!({
        "generator": config.generator,
        "seed": config.seed,
        "game_version": config.game_version,
    });
    let mut text = serde_json::to_string_pretty(&body).expect("level config serializes");
    text.push('\n');
    text
}

pub fn parse(text: &str) -> Result<LevelConfig, LevelError> {
    let value: Value =
        serde_json::from_str(text).map_err(|err| LevelError::Parse(err.to_string()))?;
    let generator = value
        .get("generator")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            LevelError::Parse("champ \"generator\" manquant ou non textuel".to_string())
        })?
        .to_string();
    let seed = value
        .get("seed")
        .and_then(Value::as_i64)
        .ok_or_else(|| LevelError::Parse("champ \"seed\" manquant ou non entier".to_string()))?;
    let game_version = value
        .get("game_version")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(LevelConfig {
        generator,
        seed,
        game_version,
    })
}

pub fn ensure(base: &Path, requested: &LevelConfig) -> Result<(), LevelError> {
    let path = level_path(base);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let stored = parse(&text)?;
            if stored.generator != requested.generator {
                return Err(LevelError::Mismatch {
                    field: "generator",
                    stored: stored.generator,
                    requested: requested.generator.clone(),
                });
            }
            if stored.seed != requested.seed {
                return Err(LevelError::Mismatch {
                    field: "seed",
                    stored: stored.seed.to_string(),
                    requested: requested.seed.to_string(),
                });
            }
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(base)?;
            fs::write(&path, serialize(requested))?;
            Ok(())
        }
        Err(err) => Err(LevelError::Io(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static DIR_SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_base(tag: &str) -> PathBuf {
        let seq = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "verdigris-level-{}-{tag}-{seq}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn config(generator: &str, seed: i64) -> LevelConfig {
        LevelConfig {
            generator: generator.to_string(),
            seed,
            game_version: "26.3".to_string(),
        }
    }

    #[test]
    fn writes_then_reads_identity() {
        let base = temp_base("rt");
        let cfg = config("noise", 20_261_029);
        ensure(&base, &cfg).expect("first start writes");
        assert!(level_path(&base).exists());
        ensure(&base, &cfg).expect("restart with identical config accepts");
        let text = fs::read_to_string(level_path(&base)).unwrap();
        assert_eq!(parse(&text).unwrap(), cfg);
    }

    #[test]
    fn different_generator_refuses_start() {
        let base = temp_base("gen");
        ensure(&base, &config("noise", 7)).unwrap();
        let err = ensure(&base, &config("flat", 7)).unwrap_err();
        assert!(matches!(
            err,
            LevelError::Mismatch {
                field: "generator",
                ..
            }
        ));
    }

    #[test]
    fn different_seed_refuses_start() {
        let base = temp_base("seed");
        ensure(&base, &config("noise", 7)).unwrap();
        let err = ensure(&base, &config("noise", 8)).unwrap_err();
        assert!(matches!(err, LevelError::Mismatch { field: "seed", .. }));
    }

    #[test]
    fn game_version_is_informational() {
        let base = temp_base("ver");
        ensure(&base, &config("noise", 7)).unwrap();
        let mut other = config("noise", 7);
        other.game_version = "99.9".to_string();
        ensure(&base, &other).expect("game_version mismatch does not block");
    }
}
