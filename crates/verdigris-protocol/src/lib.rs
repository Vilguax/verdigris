pub const GAME_VERSION: &str = "26.3";
pub const PROTOCOL_VERSION: i32 = 777;
pub const DATA_VERSION: i32 = 5023;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cible_26_3() {
        assert_eq!(
            (GAME_VERSION, PROTOCOL_VERSION, DATA_VERSION),
            ("26.3", 777, 5023)
        );
    }
}
