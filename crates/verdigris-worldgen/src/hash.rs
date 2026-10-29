pub fn hash_position(x: i32, y: i32, z: i32) -> i64 {
    let a = x.wrapping_mul(3_129_871) as i64;
    let b = (z as i64).wrapping_mul(116_129_781);
    let mut g = a ^ b ^ (y as i64);
    g = g
        .wrapping_mul(g)
        .wrapping_mul(42_317_861)
        .wrapping_add(g.wrapping_mul(11));
    g >> 16
}

pub fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for unit in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(unit as i32);
    }
    h
}
