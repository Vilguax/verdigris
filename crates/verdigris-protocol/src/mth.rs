use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Direction {
    pub fn opposite(self) -> Direction {
        match self {
            Direction::Down => Direction::Up,
            Direction::Up => Direction::Down,
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::West => Direction::East,
            Direction::East => Direction::West,
        }
    }
}

const SIN_SCALE: f32 = 10430.378;

fn sin_table() -> &'static [f32; 65536] {
    static SIN: OnceLock<Box<[f32; 65536]>> = OnceLock::new();
    SIN.get_or_init(|| {
        let mut values = vec![0.0f32; 65536];
        for (index, value) in values.iter_mut().enumerate() {
            *value = ((index as f64) * 2.0 * std::f64::consts::PI / 65536.0).sin() as f32;
        }
        match values.into_boxed_slice().try_into() {
            Ok(array) => array,
            Err(_) => unreachable!(),
        }
    })
}

pub fn sin(x: f32) -> f32 {
    sin_table()[((x * SIN_SCALE) as i32 & 65535) as usize]
}

pub fn cos(x: f32) -> f32 {
    sin_table()[((x * SIN_SCALE + 16384.0) as i32 & 65535) as usize]
}

pub fn horizontal_direction(yaw: f32) -> Direction {
    const TABLE: [Direction; 4] = [
        Direction::South,
        Direction::West,
        Direction::North,
        Direction::East,
    ];
    TABLE[(((yaw as f64) / 90.0 + 0.5).floor() as i32 & 3) as usize]
}

pub fn nearest_looking_directions(yaw: f32, pitch: f32) -> [Direction; 6] {
    let radians = (std::f64::consts::PI / 180.0) as f32;
    let f = pitch * radians;
    let g = -yaw * radians;
    let h = sin(f);
    let i = cos(f);
    let j = sin(g);
    let k = cos(g);

    let x_pos = j > 0.0;
    let y_neg = h < 0.0;
    let z_pos = k > 0.0;
    let l = if x_pos { j } else { -j };
    let m = if y_neg { -h } else { h };
    let n = if z_pos { k } else { -k };
    let o = l * i;
    let p = n * i;

    let dx = if x_pos {
        Direction::East
    } else {
        Direction::West
    };
    let dy = if y_neg {
        Direction::Up
    } else {
        Direction::Down
    };
    let dz = if z_pos {
        Direction::South
    } else {
        Direction::North
    };

    let (a, b, c) = if l > n {
        if m > o {
            (dy, dx, dz)
        } else if p > m {
            (dx, dz, dy)
        } else {
            (dx, dy, dz)
        }
    } else if m > p {
        (dy, dz, dx)
    } else if o > m {
        (dz, dx, dy)
    } else {
        (dz, dy, dx)
    };

    [a, b, c, c.opposite(), b.opposite(), a.opposite()]
}

pub fn nearest_looking_direction(yaw: f32, pitch: f32) -> Direction {
    nearest_looking_directions(yaw, pitch)[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_table_anchors() {
        let table = sin_table();
        assert_eq!(table[0], 0.0);
        assert_eq!(table[16384], 1.0);
    }

    #[test]
    fn horizontal_direction_cardinals() {
        assert_eq!(horizontal_direction(0.0), Direction::South);
        assert_eq!(horizontal_direction(90.0), Direction::West);
        assert_eq!(horizontal_direction(180.0), Direction::North);
        assert_eq!(horizontal_direction(-90.0), Direction::East);
    }

    #[test]
    fn horizontal_direction_rounding_and_wrap() {
        assert_eq!(horizontal_direction(45.0), Direction::West);
        assert_eq!(horizontal_direction(44.9), Direction::South);
        assert_eq!(horizontal_direction(360.0), Direction::South);
        assert_eq!(horizontal_direction(-720.0), Direction::South);
    }

    #[test]
    fn looking_straight_down_is_down_first() {
        assert_eq!(nearest_looking_direction(0.0, 90.0), Direction::Down);
    }

    #[test]
    fn looking_straight_up_is_up_first() {
        assert_eq!(nearest_looking_direction(0.0, -90.0), Direction::Up);
    }

    #[test]
    fn looking_level_south_is_south_first() {
        assert_eq!(nearest_looking_direction(0.0, 0.0), Direction::South);
    }

    #[test]
    fn looking_level_east_is_east_first() {
        assert_eq!(nearest_looking_direction(-90.0, 0.0), Direction::East);
    }

    #[test]
    fn level_south_full_order() {
        assert_eq!(
            nearest_looking_directions(0.0, 0.0),
            [
                Direction::South,
                Direction::Down,
                Direction::West,
                Direction::East,
                Direction::Up,
                Direction::North,
            ]
        );
    }

    #[test]
    fn steep_upward_glance_is_up_first() {
        assert_eq!(nearest_looking_direction(20.0, -80.0), Direction::Up);
    }
}
