pub const TIME_PERIOD: i64 = 24_000;

const MARKERS: [(&str, i64, &str); 4] = [
    ("day", 1_000, "minecraft:day"),
    ("noon", 6_000, "minecraft:noon"),
    ("night", 13_000, "minecraft:night"),
    ("midnight", 18_000, "minecraft:midnight"),
];

fn strip_namespace(token: &str) -> &str {
    token.strip_prefix("minecraft:").unwrap_or(token)
}

pub fn parse_marker(token: &str) -> Option<i64> {
    let key = strip_namespace(token);
    MARKERS
        .iter()
        .find(|(name, _, _)| *name == key)
        .map(|(_, ticks, _)| *ticks)
}

pub fn canonical_marker(token: &str) -> Option<&'static str> {
    let key = strip_namespace(token);
    MARKERS
        .iter()
        .find(|(name, _, _)| *name == key)
        .map(|(_, _, canonical)| *canonical)
}

pub fn move_to_marker(total: i64, marker: i64) -> Option<i64> {
    let offset = (marker - total.rem_euclid(TIME_PERIOD)).rem_euclid(TIME_PERIOD);
    if offset == 0 {
        None
    } else {
        Some(total + offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_marker_accepts_bare_and_namespaced() {
        assert_eq!(parse_marker("minecraft:day"), Some(1_000));
        assert_eq!(parse_marker("day"), Some(1_000));
        assert_eq!(parse_marker("noon"), Some(6_000));
        assert_eq!(parse_marker("night"), Some(13_000));
        assert_eq!(parse_marker("minecraft:midnight"), Some(18_000));
    }

    #[test]
    fn parse_marker_unknown_is_none() {
        assert_eq!(parse_marker("foo"), None);
        assert_eq!(parse_marker("minecraft:foo"), None);
    }

    #[test]
    fn canonical_marker_normalises_prefix() {
        assert_eq!(canonical_marker("day"), Some("minecraft:day"));
        assert_eq!(canonical_marker("minecraft:night"), Some("minecraft:night"));
        assert_eq!(canonical_marker("foo"), None);
    }

    #[test]
    fn move_to_marker_advances_to_next_occurrence() {
        assert_eq!(move_to_marker(2_650, 1_000), Some(25_000));
    }

    #[test]
    fn move_to_marker_zero_gap_is_none() {
        assert_eq!(move_to_marker(1_000, 1_000), None);
        assert_eq!(move_to_marker(25_000, 1_000), None);
    }

    #[test]
    fn move_to_marker_always_lands_on_period_offset() {
        for total in [0_i64, 500, 13_000, 23_999, 48_123] {
            if let Some(target) = move_to_marker(total, 6_000) {
                assert_eq!(target.rem_euclid(TIME_PERIOD), 6_000);
                assert!(target > total);
            }
        }
    }
}
