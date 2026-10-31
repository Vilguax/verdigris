use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;

use serde_json::Value;

use crate::biome::{biome_entries, biome_names};
use crate::feature::error::{FeatureError, full_key};

pub const DECORATION_STEPS: usize = 11;

type NodeKey = (usize, usize);

pub struct GlobalOrder {
    steps: Vec<Vec<String>>,
}

impl GlobalOrder {
    pub fn steps(&self) -> &[Vec<String>] {
        &self.steps
    }

    pub fn step(&self, i: usize) -> &[String] {
        &self.steps[i]
    }

    pub fn total(&self) -> usize {
        self.steps.iter().map(Vec::len).sum()
    }
}

pub fn build_order(biomes: &[(String, Vec<Vec<String>>)]) -> Result<GlobalOrder, FeatureError> {
    let mut feature_index: HashMap<String, usize> = HashMap::new();
    let mut counter = 0usize;
    let mut name_of: HashMap<NodeKey, String> = HashMap::new();
    let mut adjacency: BTreeMap<NodeKey, BTreeSet<NodeKey>> = BTreeMap::new();
    let mut max_steps = 0usize;

    for (_, steps) in biomes {
        max_steps = max_steps.max(steps.len());
        let mut flat: Vec<NodeKey> = Vec::new();
        for (step, features) in steps.iter().enumerate() {
            for name in features {
                let index = *feature_index.entry(name.clone()).or_insert_with(|| {
                    let i = counter;
                    counter += 1;
                    i
                });
                let node = (step, index);
                name_of.entry(node).or_insert_with(|| name.clone());
                flat.push(node);
            }
        }
        for k in 0..flat.len() {
            let node = flat[k];
            let entry = adjacency.entry(node).or_default();
            if k + 1 < flat.len() {
                entry.insert(flat[k + 1]);
            }
        }
    }

    let nodes: Vec<NodeKey> = adjacency.keys().copied().collect();
    let mut visited: BTreeSet<NodeKey> = BTreeSet::new();
    let mut in_progress: BTreeSet<NodeKey> = BTreeSet::new();
    let mut output: Vec<NodeKey> = Vec::new();

    for node in nodes {
        if !visited.contains(&node)
            && dfs(
                &adjacency,
                &mut visited,
                &mut in_progress,
                &mut output,
                node,
            )
        {
            let nm = name_of.get(&node).cloned().unwrap_or_default();
            return Err(FeatureError::Cycle(nm));
        }
    }

    output.reverse();

    let steps_count = max_steps.max(DECORATION_STEPS);
    let mut steps: Vec<Vec<String>> = vec![Vec::new(); steps_count];
    for node in output {
        let (step, _) = node;
        if let Some(name) = name_of.get(&node) {
            steps[step].push(name.clone());
        }
    }

    Ok(GlobalOrder { steps })
}

pub type BiomeStepLists = Vec<(String, Vec<Vec<String>>)>;

fn parse_steps(steps: &Value) -> Result<Vec<Vec<String>>, FeatureError> {
    let arr = steps
        .as_array()
        .ok_or_else(|| FeatureError::BadField("biome_features entry".to_string()))?;
    let mut out: Vec<Vec<String>> = Vec::with_capacity(arr.len());
    for step in arr {
        let list = step
            .as_array()
            .ok_or_else(|| FeatureError::BadField("biome_features step".to_string()))?;
        let mut names = Vec::with_capacity(list.len());
        for f in list {
            let s = f
                .as_str()
                .ok_or_else(|| FeatureError::BadField("biome_features feature".to_string()))?;
            names.push(full_key(s));
        }
        out.push(names);
    }
    Ok(out)
}

pub fn overworld_biome_order() -> Vec<String> {
    let names = biome_names();
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();
    for entry in biome_entries() {
        if seen.insert(entry.biome_id)
            && let Some(name) = names.name_of(entry.biome_id)
        {
            ordered.push(name.to_string());
        }
    }
    ordered
}

pub fn overworld_biomes_from_features(root: &Value) -> Result<BiomeStepLists, FeatureError> {
    let obj = root
        .get("biome_features")
        .and_then(Value::as_object)
        .ok_or_else(|| FeatureError::MissingField("biome_features".to_string()))?;
    let mut biomes = Vec::new();
    for name in overworld_biome_order() {
        if let Some(steps) = obj.get(&name) {
            biomes.push((name, parse_steps(steps)?));
        }
    }
    Ok(biomes)
}

pub fn global_order_from_features(root: &Value) -> Result<GlobalOrder, FeatureError> {
    let biomes = overworld_biomes_from_features(root)?;
    build_order(&biomes)
}

fn dfs(
    adjacency: &BTreeMap<NodeKey, BTreeSet<NodeKey>>,
    visited: &mut BTreeSet<NodeKey>,
    in_progress: &mut BTreeSet<NodeKey>,
    output: &mut Vec<NodeKey>,
    node: NodeKey,
) -> bool {
    if visited.contains(&node) {
        return false;
    }
    if in_progress.contains(&node) {
        return true;
    }
    in_progress.insert(node);
    if let Some(successors) = adjacency.get(&node) {
        for succ in successors {
            if dfs(adjacency, visited, in_progress, output, *succ) {
                return true;
            }
        }
    }
    in_progress.remove(&node);
    visited.insert(node);
    output.push(node);
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn biome(name: &str, steps: &[&[&str]]) -> (String, Vec<Vec<String>>) {
        (
            name.to_string(),
            steps
                .iter()
                .map(|s| s.iter().map(|f| f.to_string()).collect())
                .collect(),
        )
    }

    #[test]
    fn respects_within_step_order() {
        let biomes = vec![biome("a", &[&["x", "y", "z"]]), biome("b", &[&["x", "z"]])];
        let order = build_order(&biomes).unwrap();
        let step0 = order.step(0);
        let px = step0.iter().position(|f| f == "x").unwrap();
        let py = step0.iter().position(|f| f == "y").unwrap();
        let pz = step0.iter().position(|f| f == "z").unwrap();
        assert!(px < py && py < pz);
    }

    #[test]
    fn detects_cycle() {
        let biomes = vec![biome("a", &[&["x", "y"]]), biome("b", &[&["y", "x"]])];
        assert!(matches!(build_order(&biomes), Err(FeatureError::Cycle(_))));
    }

    #[test]
    fn always_eleven_steps() {
        let biomes = vec![biome("a", &[&["x"]])];
        let order = build_order(&biomes).unwrap();
        assert_eq!(order.steps().len(), DECORATION_STEPS);
    }

    #[test]
    fn embedded_global_order_matches_2_5_counts() {
        let root: Value = serde_json::from_str(crate::feature::registry::FEATURES_JSON).unwrap();
        let biomes = overworld_biomes_from_features(&root).unwrap();
        assert_eq!(biomes.len(), 56, "56 overworld biomes expected");
        let order = global_order_from_features(&root).unwrap();
        assert_eq!(order.steps().len(), DECORATION_STEPS);
        assert_eq!(
            order.total(),
            171,
            "171 placed feature entries expected (2.5)"
        );
    }

    #[test]
    fn embedded_global_order_is_consistent() {
        let root: Value = serde_json::from_str(crate::feature::registry::FEATURES_JSON).unwrap();
        let order = global_order_from_features(&root).unwrap();
        assert_eq!(order.steps().len(), DECORATION_STEPS);
        assert!(order.total() > 0);
        let biomes = overworld_biomes_from_features(&root).unwrap();
        for (_, steps) in &biomes {
            for (step, features) in steps.iter().enumerate() {
                let global = order.step(step);
                let mut last = None;
                for name in features {
                    let pos = global.iter().position(|f| f == name);
                    assert!(pos.is_some(), "feature {name} absent from step {step}");
                    if let (Some(prev), Some(cur)) = (last, pos) {
                        assert!(prev < cur, "order violated for {name} in step {step}");
                    }
                    last = pos;
                }
            }
        }
    }
}
