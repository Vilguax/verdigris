use verdigris_worldgen::SearchState;
use verdigris_worldgen::biome::boxes::ParamBox;
use verdigris_worldgen::biome::search::BiomeTree;

fn point(v: i64, biome: u16) -> ParamBox {
    ParamBox {
        a: [v, 0, 0, 0, 0, 0, 0],
        b: [v, 0, 0, 0, 0, 0, 0],
        biome,
    }
}

fn t(v: i64) -> [i64; 7] {
    [v, 0, 0, 0, 0, 0, 0]
}

fn tree() -> BiomeTree {
    let a = point(0, 10);
    let b = point(20, 20);
    let c = point(1000, 30);
    BiomeTree::from_parts(vec![a, b, c], vec![1, 0, 2])
}

#[test]
fn unique_minimum_ignores_last_result() {
    let tree = tree();
    for seed in [None, Some(0usize), Some(1), Some(2)] {
        let mut state = SearchState { last: seed };
        let biome = tree.search(&mut state, &t(0));
        assert_eq!(
            biome, 10,
            "cible stricte -> boîte A quel que soit last={seed:?}"
        );
        assert_eq!(state.last, Some(0), "last mis à jour sur le résultat");
    }
}

#[test]
fn tie_without_last_returns_smallest_rank() {
    let tree = tree();
    let mut state = SearchState::new();
    let biome = tree.search(&mut state, &t(10));
    assert_eq!(biome, 20, "égalité A/B -> rang minimal (B, rang 0)");
    assert_eq!(state.last, Some(1));
}

#[test]
fn tie_keeps_last_result_when_it_is_minimal() {
    let tree = tree();
    let mut state = SearchState { last: Some(0) };
    let biome = tree.search(&mut state, &t(10));
    assert_eq!(biome, 10, "A est à distance minimale -> collant conservé");
    assert_eq!(state.last, Some(0));
}

#[test]
fn tie_ignores_last_result_when_it_is_not_minimal() {
    let tree = tree();
    let mut state = SearchState { last: Some(2) };
    let biome = tree.search(&mut state, &t(10));
    assert_eq!(biome, 20, "C hors minimum -> ignoré, rang minimal B");
    assert_eq!(state.last, Some(1));
}

#[test]
fn state_threads_across_successive_queries() {
    let tree = tree();
    let mut state = SearchState::new();
    assert_eq!(tree.search(&mut state, &t(10)), 20);
    assert_eq!(state.last, Some(1));
    assert_eq!(tree.search(&mut state, &t(0)), 10);
    assert_eq!(state.last, Some(0));
    assert_eq!(
        tree.search(&mut state, &t(10)),
        10,
        "A devenu collant -> conservé sur l'égalité suivante"
    );
    assert_eq!(state.last, Some(0));
}

#[test]
fn min_info_reports_the_tie_set() {
    let tree = tree();
    let (dist, mut biomes) = tree.min_info(&t(10));
    assert_eq!(dist, 100);
    biomes.sort_unstable();
    assert_eq!(biomes, vec![10, 20]);
}
