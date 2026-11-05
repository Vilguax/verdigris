#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weather {
    Clear,
    Rain,
    Thunder,
}

pub const RAIN_START: u8 = 1;
pub const RAIN_STOP: u8 = 2;
pub const RAIN_LEVEL: u8 = 7;
pub const THUNDER_LEVEL: u8 = 8;

pub fn weather_transition(from: Weather, to: Weather) -> Vec<(u8, f32)> {
    use Weather::{Clear, Rain, Thunder};
    match (from, to) {
        (Clear, Rain) => vec![(RAIN_START, 0.0), (RAIN_LEVEL, 1.0)],
        (Clear, Thunder) => vec![(RAIN_START, 0.0), (RAIN_LEVEL, 1.0), (THUNDER_LEVEL, 1.0)],
        (Rain, Thunder) => vec![(THUNDER_LEVEL, 1.0)],
        (Thunder, Rain) => vec![(THUNDER_LEVEL, 0.0)],
        (Rain, Clear) => vec![(RAIN_STOP, 0.0), (RAIN_LEVEL, 0.0)],
        (Thunder, Clear) => vec![(RAIN_STOP, 0.0), (RAIN_LEVEL, 0.0), (THUNDER_LEVEL, 0.0)],
        _ => Vec::new(),
    }
}

pub fn weather_on_join(state: Weather) -> Vec<(u8, f32)> {
    use Weather::{Clear, Rain, Thunder};
    match state {
        Clear => Vec::new(),
        Rain => vec![(RAIN_START, 0.0), (RAIN_LEVEL, 1.0)],
        Thunder => vec![(RAIN_START, 0.0), (RAIN_LEVEL, 1.0), (THUNDER_LEVEL, 1.0)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verdigris_protocol::game_event_payload;

    #[test]
    fn clear_to_rain_then_thunder() {
        assert_eq!(
            weather_transition(Weather::Clear, Weather::Rain),
            vec![(1, 0.0), (7, 1.0)]
        );
        assert_eq!(
            weather_transition(Weather::Clear, Weather::Thunder),
            vec![(1, 0.0), (7, 1.0), (8, 1.0)]
        );
    }

    #[test]
    fn rain_and_thunder_swaps() {
        assert_eq!(
            weather_transition(Weather::Rain, Weather::Thunder),
            vec![(8, 1.0)]
        );
        assert_eq!(
            weather_transition(Weather::Thunder, Weather::Rain),
            vec![(8, 0.0)]
        );
    }

    #[test]
    fn transitions_back_to_clear() {
        assert_eq!(
            weather_transition(Weather::Rain, Weather::Clear),
            vec![(2, 0.0), (7, 0.0)]
        );
        assert_eq!(
            weather_transition(Weather::Thunder, Weather::Clear),
            vec![(2, 0.0), (7, 0.0), (8, 0.0)]
        );
    }

    #[test]
    fn same_state_emits_nothing() {
        assert!(weather_transition(Weather::Clear, Weather::Clear).is_empty());
        assert!(weather_transition(Weather::Rain, Weather::Rain).is_empty());
        assert!(weather_transition(Weather::Thunder, Weather::Thunder).is_empty());
    }

    #[test]
    fn join_matches_current_state() {
        assert!(weather_on_join(Weather::Clear).is_empty());
        assert_eq!(weather_on_join(Weather::Rain), vec![(1, 0.0), (7, 1.0)]);
        assert_eq!(
            weather_on_join(Weather::Thunder),
            vec![(1, 0.0), (7, 1.0), (8, 1.0)]
        );
    }

    #[test]
    fn game_event_bytes_for_transition() {
        assert_eq!(
            game_event_payload(1, 0.0),
            vec![0x01, 0x00, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            game_event_payload(7, 1.0),
            vec![0x07, 0x3f, 0x80, 0x00, 0x00]
        );
        assert_eq!(
            game_event_payload(2, 0.0),
            vec![0x02, 0x00, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            game_event_payload(8, 1.0),
            vec![0x08, 0x3f, 0x80, 0x00, 0x00]
        );
    }
}
