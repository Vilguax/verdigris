#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMode {
    Survival,
    Creative,
    Adventure,
    Spectator,
}

impl GameMode {
    pub fn parse(s: &str) -> Option<GameMode> {
        match s {
            "survival" => Some(GameMode::Survival),
            "creative" => Some(GameMode::Creative),
            "adventure" => Some(GameMode::Adventure),
            "spectator" => Some(GameMode::Spectator),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            GameMode::Survival => "survival",
            GameMode::Creative => "creative",
            GameMode::Adventure => "adventure",
            GameMode::Spectator => "spectator",
        }
    }

    pub fn game_type(self) -> i32 {
        match self {
            GameMode::Survival => 0,
            GameMode::Creative => 1,
            GameMode::Adventure => 2,
            GameMode::Spectator => 3,
        }
    }

    pub fn from_game_type(value: i32) -> Option<GameMode> {
        match value {
            0 => Some(GameMode::Survival),
            1 => Some(GameMode::Creative),
            2 => Some(GameMode::Adventure),
            3 => Some(GameMode::Spectator),
            _ => None,
        }
    }

    pub fn abilities_flags(self) -> u8 {
        match self {
            GameMode::Creative => 0x0D,
            GameMode::Spectator => 0x07,
            GameMode::Survival | GameMode::Adventure => 0x00,
        }
    }

    pub fn can_fly(self) -> bool {
        matches!(self, GameMode::Creative | GameMode::Spectator)
    }
}

impl core::fmt::Display for GameMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_type_per_mode() {
        assert_eq!(GameMode::Survival.game_type(), 0);
        assert_eq!(GameMode::Creative.game_type(), 1);
        assert_eq!(GameMode::Adventure.game_type(), 2);
        assert_eq!(GameMode::Spectator.game_type(), 3);
    }

    #[test]
    fn abilities_flags_per_mode() {
        assert_eq!(GameMode::Survival.abilities_flags(), 0x00);
        assert_eq!(GameMode::Creative.abilities_flags(), 0x0D);
        assert_eq!(GameMode::Adventure.abilities_flags(), 0x00);
        assert_eq!(GameMode::Spectator.abilities_flags(), 0x07);
    }

    #[test]
    fn can_fly_only_creative_and_spectator() {
        assert!(!GameMode::Survival.can_fly());
        assert!(GameMode::Creative.can_fly());
        assert!(!GameMode::Adventure.can_fly());
        assert!(GameMode::Spectator.can_fly());
    }

    #[test]
    fn parse_each_value() {
        assert_eq!(GameMode::parse("survival"), Some(GameMode::Survival));
        assert_eq!(GameMode::parse("creative"), Some(GameMode::Creative));
        assert_eq!(GameMode::parse("adventure"), Some(GameMode::Adventure));
        assert_eq!(GameMode::parse("spectator"), Some(GameMode::Spectator));
    }

    #[test]
    fn parse_unknown_is_none() {
        assert_eq!(GameMode::parse("foo"), None);
        assert_eq!(GameMode::parse(""), None);
        assert_eq!(GameMode::parse("Creative"), None);
    }

    #[test]
    fn from_game_type_round_trips_known_values() {
        for mode in [
            GameMode::Survival,
            GameMode::Creative,
            GameMode::Adventure,
            GameMode::Spectator,
        ] {
            assert_eq!(GameMode::from_game_type(mode.game_type()), Some(mode));
        }
        assert_eq!(GameMode::from_game_type(4), None);
        assert_eq!(GameMode::from_game_type(-1), None);
    }

    #[test]
    fn display_is_lowercase_name() {
        assert_eq!(GameMode::Spectator.to_string(), "spectator");
    }
}
