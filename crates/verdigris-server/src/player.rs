use tracing::warn;
use verdigris_protocol::GameMode;

pub const GAMEMODE_ENV: &str = "VERDIGRIS_GAMEMODE";

pub fn resolve_gamemode(value: Option<&str>) -> GameMode {
    match value {
        None => GameMode::Creative,
        Some(raw) => match GameMode::parse(raw) {
            Some(mode) => mode,
            None => {
                warn!(
                    value = raw,
                    "VERDIGRIS_GAMEMODE inconnu, repli sur creative"
                );
                GameMode::Creative
            }
        },
    }
}

pub fn read_gamemode() -> GameMode {
    resolve_gamemode(std::env::var(GAMEMODE_ENV).ok().as_deref())
}

pub struct PlayerState {
    pub name: String,
    pub mode: GameMode,
    pub flying: bool,
}

impl PlayerState {
    pub fn connect(name: &str) -> Self {
        PlayerState::with_mode(name, read_gamemode())
    }

    pub fn with_mode(name: &str, mode: GameMode) -> Self {
        let flying = (mode.abilities_flags() & 0x02) != 0;
        PlayerState {
            name: name.to_owned(),
            mode,
            flying,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_env_is_creative() {
        assert_eq!(resolve_gamemode(None), GameMode::Creative);
    }

    #[test]
    fn known_values_are_mapped() {
        assert_eq!(resolve_gamemode(Some("survival")), GameMode::Survival);
        assert_eq!(resolve_gamemode(Some("creative")), GameMode::Creative);
        assert_eq!(resolve_gamemode(Some("adventure")), GameMode::Adventure);
        assert_eq!(resolve_gamemode(Some("spectator")), GameMode::Spectator);
    }

    #[test]
    fn unknown_value_falls_back_to_creative() {
        assert_eq!(resolve_gamemode(Some("xxx")), GameMode::Creative);
        assert_eq!(resolve_gamemode(Some("")), GameMode::Creative);
    }

    #[test]
    fn flying_tracks_the_abilities_flag() {
        assert!(!PlayerState::with_mode("Verdi", GameMode::Survival).flying);
        assert!(!PlayerState::with_mode("Verdi", GameMode::Creative).flying);
        assert!(!PlayerState::with_mode("Verdi", GameMode::Adventure).flying);
        assert!(PlayerState::with_mode("Verdi", GameMode::Spectator).flying);
    }
}
