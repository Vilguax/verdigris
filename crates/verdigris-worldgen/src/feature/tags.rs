use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::OnceLock;

use serde_json::Value;

pub const NETWORK_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../verdigris-protocol/data/26.3/network.json"
));

pub const BLOCKS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../verdigris-protocol/data/26.3/blocks.json"
));

fn network_value() -> &'static Value {
    static V: OnceLock<Value> = OnceLock::new();
    V.get_or_init(|| serde_json::from_str(NETWORK_JSON).expect("network.json parses"))
}

fn blocks_value() -> &'static Value {
    static V: OnceLock<Value> = OnceLock::new();
    V.get_or_init(|| serde_json::from_str(BLOCKS_JSON).expect("blocks.json parses"))
}

fn block_id_to_name() -> &'static HashMap<i64, String> {
    static M: OnceLock<HashMap<i64, String>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m = HashMap::new();
        if let Some(arr) = blocks_value().get("blocks").and_then(Value::as_array) {
            for b in arr {
                if let (Some(name), Some(id)) = (
                    b.get("name").and_then(Value::as_str),
                    b.get("id").and_then(Value::as_i64),
                ) {
                    m.insert(id, name.to_string());
                }
            }
        }
        m
    })
}

fn fluid_id_to_name() -> &'static HashMap<i64, String> {
    static M: OnceLock<HashMap<i64, String>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m = HashMap::new();
        if let Some(regs) = network_value().get("registries").and_then(Value::as_array) {
            for r in regs {
                if r.get("id").and_then(Value::as_str) == Some("minecraft:fluid")
                    && let Some(entries) = r.get("entries").and_then(Value::as_array)
                {
                    for (i, e) in entries.iter().enumerate() {
                        if let Some(name) = e.as_str() {
                            m.insert(i as i64, name.to_string());
                        }
                    }
                }
            }
        }
        m
    })
}

fn tag_entries(registry: &str) -> Option<&'static Vec<Value>> {
    let tags = network_value().get("tags")?.as_array()?;
    let group = tags
        .iter()
        .find(|g| g.get("registry").and_then(Value::as_str) == Some(registry))?;
    group.get("tags")?.as_array()
}

fn resolve_into(
    registry: &str,
    name: &str,
    id_map: &HashMap<i64, String>,
    out: &mut Vec<String>,
    seen_members: &mut HashSet<String>,
    seen_tags: &mut HashSet<String>,
) {
    if !seen_tags.insert(name.to_string()) {
        return;
    }
    let entries = match tag_entries(registry) {
        Some(e) => e,
        None => return,
    };
    let entry = entries
        .iter()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(name));
    let entry = match entry {
        Some(e) => e,
        None => return,
    };
    let Some(ids) = entry.get("ids").and_then(Value::as_array) else {
        return;
    };
    for v in ids {
        match v {
            Value::Number(n) => {
                if let Some(id) = n.as_i64()
                    && let Some(bn) = id_map.get(&id)
                    && seen_members.insert(bn.clone())
                {
                    out.push(bn.clone());
                }
            }
            Value::String(s) => {
                if let Some(sub) = s.strip_prefix('#') {
                    resolve_into(registry, sub, id_map, out, seen_members, seen_tags);
                } else if seen_members.insert(s.clone()) {
                    out.push(s.clone());
                }
            }
            _ => {}
        }
    }
}

pub fn resolve_block_tag(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut m = HashSet::new();
    let mut t = HashSet::new();
    resolve_into(
        "minecraft:block",
        name,
        block_id_to_name(),
        &mut out,
        &mut m,
        &mut t,
    );
    out
}

pub fn resolve_fluid_tag(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut m = HashSet::new();
    let mut t = HashSet::new();
    resolve_into(
        "minecraft:fluid",
        name,
        fluid_id_to_name(),
        &mut out,
        &mut m,
        &mut t,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_block_tag_resolves_in_order() {
        let members = resolve_block_tag("minecraft:dirt");
        assert!(members.iter().any(|m| m == "minecraft:dirt"));
        let mut seen = HashSet::new();
        for m in &members {
            assert!(seen.insert(m.clone()), "duplicate member {m}");
        }
    }
}
