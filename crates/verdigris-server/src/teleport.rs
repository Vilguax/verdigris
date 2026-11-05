use crate::registry::PlayerHandle;
use verdigris_protocol::{Arg, Component};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectError {
    Syntax,
    NotFound,
    TooMany,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordError {
    Caret,
    Malformed,
}

pub fn alloc(next: &mut i32, awaiting: Option<i32>) -> i32 {
    let mut id = *next;
    if Some(id) == awaiting {
        id = id.wrapping_add(1) & 0x7fff_ffff;
    }
    *next = id.wrapping_add(1) & 0x7fff_ffff;
    id
}

pub fn next_rand(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

pub fn seed_rng() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seed = nanos ^ 0x9e37_79b9_7f4a_7c15;
    if seed == 0 {
        0x1234_5678_9abc_def0
    } else {
        seed
    }
}

pub fn eval_coord(token: &str, base: f64, center: bool) -> Result<f64, CoordError> {
    if token.starts_with('^') {
        return Err(CoordError::Caret);
    }
    let value = if let Some(rest) = token.strip_prefix('~') {
        let delta = if rest.is_empty() {
            0.0
        } else {
            rest.parse::<f64>().map_err(|_| CoordError::Malformed)?
        };
        base + delta
    } else {
        let parsed: f64 = token.parse().map_err(|_| CoordError::Malformed)?;
        if center && !token.contains('.') {
            parsed.floor() + 0.5
        } else {
            parsed
        }
    };
    if !value.is_finite() {
        return Err(CoordError::Malformed);
    }
    Ok(value)
}

pub fn eval_vec3(tokens: [&str; 3], base: (f64, f64, f64)) -> Result<[f64; 3], CoordError> {
    let x = eval_coord(tokens[0], base.0, true)?;
    let y = eval_coord(tokens[1], base.1, false)?;
    let z = eval_coord(tokens[2], base.2, true)?;
    Ok([x, y, z])
}

fn nearest(players: Vec<PlayerHandle>, origin: (f64, f64, f64)) -> Vec<PlayerHandle> {
    let (sx, sy, sz) = origin;
    players
        .into_iter()
        .min_by(|a, b| {
            let da = (a.x - sx).powi(2) + (a.y - sy).powi(2) + (a.z - sz).powi(2);
            let db = (b.x - sx).powi(2) + (b.y - sy).powi(2) + (b.z - sz).powi(2);
            da.total_cmp(&db).then(a.uuid.cmp(&b.uuid))
        })
        .into_iter()
        .collect()
}

fn random_one(players: Vec<PlayerHandle>, rng: &mut u64) -> Vec<PlayerHandle> {
    if players.is_empty() {
        return Vec::new();
    }
    let index = (next_rand(rng) as usize) % players.len();
    players.into_iter().nth(index).into_iter().collect()
}

pub fn select(
    players: Vec<PlayerHandle>,
    author_uuid: u128,
    author_pos: (f64, f64, f64),
    token: &str,
    rng: &mut u64,
) -> Result<Vec<PlayerHandle>, SelectError> {
    if token.contains('[') || token.contains(']') {
        return Err(SelectError::Syntax);
    }
    Ok(match token {
        "@a" | "@e" => players,
        "@s" => players
            .into_iter()
            .filter(|handle| handle.uuid == author_uuid)
            .collect(),
        "@p" => nearest(players, author_pos),
        "@r" => random_one(players, rng),
        name => players
            .into_iter()
            .filter(|handle| handle.name == name)
            .collect(),
    })
}

pub fn resolve_targets(
    players: Vec<PlayerHandle>,
    author_uuid: u128,
    author_pos: (f64, f64, f64),
    token: &str,
    rng: &mut u64,
) -> Result<Vec<PlayerHandle>, SelectError> {
    let selected = select(players, author_uuid, author_pos, token, rng)?;
    if selected.is_empty() {
        Err(SelectError::NotFound)
    } else {
        Ok(selected)
    }
}

pub fn resolve_destination(
    players: Vec<PlayerHandle>,
    author_uuid: u128,
    author_pos: (f64, f64, f64),
    token: &str,
    rng: &mut u64,
) -> Result<PlayerHandle, SelectError> {
    let mut selected = select(players, author_uuid, author_pos, token, rng)?;
    match selected.len() {
        0 => Err(SelectError::NotFound),
        1 => Ok(selected.pop().unwrap()),
        _ => Err(SelectError::TooMany),
    }
}

fn fmt_coord(value: f64) -> String {
    format!("{value:.6}")
}

pub fn location_feedback(target_name: &str, count: usize, pos: [f64; 3]) -> Component {
    let (key, first) = if count == 1 {
        (
            "commands.teleport.success.location.single",
            Arg::Str(target_name.to_owned()),
        )
    } else {
        (
            "commands.teleport.success.location.multiple",
            Arg::Int(count as i32),
        )
    };
    Component::translate(key)
        .with(first)
        .with(Arg::Str(fmt_coord(pos[0])))
        .with(Arg::Str(fmt_coord(pos[1])))
        .with(Arg::Str(fmt_coord(pos[2])))
}

pub fn entity_feedback(target_name: &str, count: usize, dest_name: &str) -> Component {
    let (key, first) = if count == 1 {
        (
            "commands.teleport.success.entity.single",
            Arg::Str(target_name.to_owned()),
        )
    } else {
        (
            "commands.teleport.success.entity.multiple",
            Arg::Int(count as i32),
        )
    };
    Component::translate(key)
        .with(first)
        .with(Arg::Str(dest_name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;
    use verdigris_protocol::write_nbt;

    fn handle(uuid: u128, name: &str, pos: (f64, f64, f64)) -> PlayerHandle {
        let (tx, _rx) = unbounded_channel();
        PlayerHandle {
            uuid,
            name: name.to_owned(),
            x: pos.0,
            y: pos.1,
            z: pos.2,
            sender: tx,
        }
    }

    #[test]
    fn alloc_nominal_sequence() {
        let mut next = 2;
        assert_eq!(alloc(&mut next, None), 2);
        assert_eq!(alloc(&mut next, None), 3);
        assert_eq!(alloc(&mut next, None), 4);
        assert_eq!(next, 5);
    }

    #[test]
    fn alloc_wraps_past_max() {
        let mut next = i32::MAX;
        assert_eq!(alloc(&mut next, None), i32::MAX);
        assert_eq!(next, 0);
    }

    #[test]
    fn alloc_skips_awaiting_id() {
        let mut next = 2;
        assert_eq!(alloc(&mut next, Some(2)), 3);
        assert_eq!(next, 4);
    }

    #[test]
    fn centering_only_on_integer_x_and_z() {
        assert_eq!(
            eval_vec3(["0", "100", "0"], (9.0, 9.0, 9.0)).unwrap(),
            [0.5, 100.0, 0.5]
        );
        assert_eq!(
            eval_vec3(["0.0", "100", "0.0"], (9.0, 9.0, 9.0)).unwrap(),
            [0.0, 100.0, 0.0]
        );
        assert_eq!(
            eval_vec3(["-5", "64", "-5"], (0.0, 0.0, 0.0)).unwrap(),
            [-4.5, 64.0, -4.5]
        );
    }

    #[test]
    fn relative_coords_use_author_without_centering() {
        assert_eq!(
            eval_vec3(["~", "~1", "~"], (3.25, 70.0, -8.75)).unwrap(),
            [3.25, 71.0, -8.75]
        );
    }

    #[test]
    fn caret_and_malformed_are_errors() {
        assert_eq!(eval_coord("^2", 0.0, true), Err(CoordError::Caret));
        assert_eq!(eval_coord("abc", 0.0, true), Err(CoordError::Malformed));
    }

    #[test]
    fn non_finite_coords_are_rejected() {
        for token in ["NaN", "nan", "inf", "infinity", "-inf", "+inf", "Infinity"] {
            assert_eq!(
                eval_coord(token, 0.0, true),
                Err(CoordError::Malformed),
                "coordonnée absolue non finie rejetée: {token}"
            );
            assert_eq!(
                eval_coord(token, 0.0, false),
                Err(CoordError::Malformed),
                "coordonnée y non finie rejetée: {token}"
            );
            let relative = format!("~{token}");
            assert_eq!(
                eval_coord(&relative, 0.0, true),
                Err(CoordError::Malformed),
                "delta relatif non fini rejeté: {relative}"
            );
        }
        assert_eq!(
            eval_vec3(["NaN", "100", "0"], (0.0, 0.0, 0.0)),
            Err(CoordError::Malformed),
            "un vec3 avec une composante non finie échoue en entier"
        );
        assert_eq!(
            eval_vec3(["0", "inf", "0"], (0.0, 0.0, 0.0)),
            Err(CoordError::Malformed)
        );
    }

    #[test]
    fn destination_cardinality_errors() {
        let mut rng: u64 = 1;
        let players = vec![handle(1, "Alice", (0.0, 0.0, 0.0))];
        assert!(matches!(
            resolve_destination(players.clone(), 1, (0.0, 0.0, 0.0), "Ghost", &mut rng),
            Err(SelectError::NotFound)
        ));
        assert_eq!(
            resolve_destination(players.clone(), 1, (0.0, 0.0, 0.0), "@a", &mut rng)
                .map(|h| h.uuid),
            Ok(1u128)
        );
        let two = vec![
            handle(1, "Alice", (0.0, 0.0, 0.0)),
            handle(2, "Bob", (1.0, 0.0, 0.0)),
        ];
        assert!(matches!(
            resolve_destination(two, 1, (0.0, 0.0, 0.0), "@a", &mut rng),
            Err(SelectError::TooMany)
        ));
    }

    #[test]
    fn targets_zero_is_notfound_and_brackets_are_syntax() {
        let mut rng: u64 = 1;
        let players = vec![handle(1, "Alice", (0.0, 0.0, 0.0))];
        assert!(matches!(
            resolve_targets(players.clone(), 1, (0.0, 0.0, 0.0), "Ghost", &mut rng),
            Err(SelectError::NotFound)
        ));
        assert!(matches!(
            resolve_targets(players.clone(), 1, (0.0, 0.0, 0.0), "@a[x=1]", &mut rng),
            Err(SelectError::Syntax)
        ));
        assert_eq!(
            select(players, 1, (0.0, 0.0, 0.0), "@s", &mut rng)
                .unwrap()
                .into_iter()
                .map(|h| h.uuid)
                .collect::<Vec<_>>(),
            vec![1u128]
        );
    }

    #[test]
    fn random_target_is_a_player_and_unique() {
        let mut rng = seed_rng();
        let players = vec![
            handle(1, "Alice", (0.0, 0.0, 0.0)),
            handle(2, "Bob", (1.0, 0.0, 0.0)),
            handle(3, "Carol", (2.0, 0.0, 0.0)),
        ];
        let uuids = [1u128, 2, 3];
        for _ in 0..32 {
            let picked =
                resolve_targets(players.clone(), 1, (0.0, 0.0, 0.0), "@r", &mut rng).unwrap();
            assert_eq!(picked.len(), 1);
            assert!(uuids.contains(&picked[0].uuid));
        }
    }

    #[test]
    fn nearest_breaks_ties_with_smallest_uuid() {
        let mut rng: u64 = 1;
        let players = vec![
            handle(7, "Far", (10.0, 0.0, 0.0)),
            handle(5, "A", (1.0, 0.0, 0.0)),
            handle(3, "B", (1.0, 0.0, 0.0)),
        ];
        let picked = resolve_destination(players, 99, (0.0, 0.0, 0.0), "@p", &mut rng).unwrap();
        assert_eq!(picked.uuid, 3);
    }

    #[test]
    fn location_single_feedback_has_key_and_formatted_coords() {
        let component = location_feedback("Verdi", 1, [0.5, 100.0, 0.5]);
        let mut buf = Vec::new();
        write_nbt(&mut buf, &component);
        assert!(contains(&buf, b"commands.teleport.success.location.single"));
        assert!(contains(&buf, b"0.500000"));
        assert!(contains(&buf, b"100.000000"));
        assert!(contains(&buf, b"Verdi"));
    }

    #[test]
    fn entity_single_feedback_has_both_names() {
        let component = entity_feedback("Alice", 1, "Bob");
        let mut buf = Vec::new();
        write_nbt(&mut buf, &component);
        assert!(contains(&buf, b"commands.teleport.success.entity.single"));
        assert!(contains(&buf, b"Alice"));
        assert!(contains(&buf, b"Bob"));
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }
}
