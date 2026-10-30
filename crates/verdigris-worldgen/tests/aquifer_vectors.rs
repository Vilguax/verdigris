use verdigris_worldgen::aquifer::{anchor_cell, y_saut_from_max};
use verdigris_worldgen::sources::{aquifer_factory, positional_factory};
use verdigris_worldgen::xoroshiro::md5_words;

fn center(seed: i64, gx: i32, gy: i32, gz: i32) -> [i32; 3] {
    let factory = aquifer_factory(&positional_factory(seed, false));
    let mut r = factory.at(gx, gy, gz);
    let a = r.next_int_bounded(10);
    let b = r.next_int_bounded(9);
    let e = r.next_int_bounded(10);
    [16 * gx + a, 12 * gy + b, 16 * gz + e]
}

#[test]
fn md5_of_aquifer_name() {
    assert_eq!(
        md5_words("minecraft:aquifer"),
        (8_913_007_134_489_619_686, 854_934_872_360_429_201)
    );
}

#[test]
fn centers_seed_0() {
    assert_eq!(center(0, 0, 0, 0), [4, 6, 9]);
    assert_eq!(center(0, -1, -7, -1), [-9, -80, -16]);
    assert_eq!(center(0, 1, 2, 3), [22, 32, 49]);
    assert_eq!(center(0, 0, 5, 0), [2, 68, 8]);
    assert_eq!(center(0, -1, 0, 0), [-15, 6, 4]);
    assert_eq!(center(0, 3, -2, -5), [57, -17, -72]);
}

#[test]
fn centers_seed_12345() {
    assert_eq!(center(12345, 0, 0, 0), [9, 3, 1]);
    assert_eq!(center(12345, -1, -7, -1), [-13, -78, -12]);
    assert_eq!(center(12345, 1, 2, 3), [22, 24, 54]);
    assert_eq!(center(12345, 0, 5, 0), [0, 67, 2]);
    assert_eq!(center(12345, -1, 0, 0), [-15, 3, 9]);
    assert_eq!(center(12345, 3, -2, -5), [52, -24, -75]);
}

#[test]
fn anchor_and_y_saut_8_1() {
    assert_eq!(anchor_cell(0, 0, 0), [-1, 0, -1]);
    assert_eq!(anchor_cell(5, -1, 21), [0, 0, 1]);
    assert_eq!(y_saut_from_max(64), 106);
    assert_eq!(y_saut_from_max(40), 82);
    assert_eq!(y_saut_from_max(-64), -26);
}
