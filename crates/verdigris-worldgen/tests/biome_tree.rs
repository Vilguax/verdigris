use verdigris_worldgen::biome::boxes::ParamBox;
use verdigris_worldgen::biome::data::biome_entries;
use verdigris_worldgen::biome::tree::{BuiltTree, build_tree, largest_pow19_below};

fn built() -> BuiltTree {
    let boxes: Vec<ParamBox> = biome_entries().iter().map(ParamBox::from_raw).collect();
    build_tree(&boxes)
}

#[test]
fn pow19_reference_values() {
    assert_eq!(largest_pow19_below(7594), 6859);
    assert_eq!(largest_pow19_below(6859), 361);
    assert_eq!(largest_pow19_below(735), 361);
    assert_eq!(largest_pow19_below(361), 19);
    assert_eq!(largest_pow19_below(19), 1);
    assert_eq!(largest_pow19_below(20), 19);
}

#[test]
fn tree_has_7594_leaves() {
    assert_eq!(built().leaf_count(), 7594);
}

#[test]
fn root_has_two_children_6859_and_735_split_on_axis_zero() {
    let tree = built();
    let mut sizes = BuiltTree::child_sizes(&tree.root);
    sizes.sort_unstable();
    assert_eq!(sizes, vec![735, 6859]);
    assert_eq!(tree.root_axis(), Some(0));
}

#[test]
fn tree_height_is_four() {
    assert_eq!(built().height(), 4);
}

#[test]
fn reference_ranks() {
    let tree = built();
    let rank = tree.rank.as_slice();
    assert_eq!(rank[0], 8, "rank[0]");
    assert_eq!(rank[1], 3965, "rank[1]");
    assert_eq!(rank[2], 15, "rank[2]");
    assert_eq!(rank[7590], 3421, "rank[7590]");
    assert_eq!(rank[7591], 3420, "rank[7591]");
    assert_eq!(rank[7592], 3428, "rank[7592]");
    assert_eq!(rank[7593], 6498, "rank[7593]");
}

#[test]
fn ranks_are_a_permutation() {
    let tree = built();
    let mut seen = vec![false; tree.rank.len()];
    for &r in &tree.rank {
        assert!(!seen[r as usize], "duplicate rank {r}");
        seen[r as usize] = true;
    }
    assert!(seen.iter().all(|&b| b));
}
