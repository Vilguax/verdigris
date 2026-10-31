use crate::biome::boxes::ParamBox;
use crate::biome::data::DIMENSIONS;

pub fn largest_pow19_below(n: usize) -> usize {
    let mut p = 1usize;
    while p * 19 < n {
        p *= 19;
    }
    p
}

const STEP_THRESHOLD: usize = 19;

#[derive(Clone)]
pub enum TreeNode {
    Leaf {
        entry: usize,
        space: [[i64; 2]; DIMENSIONS],
    },
    Sub {
        children: Vec<TreeNode>,
        space: [[i64; 2]; DIMENSIONS],
        axis: Option<usize>,
    },
}

impl TreeNode {
    fn space(&self) -> &[[i64; 2]; DIMENSIONS] {
        match self {
            TreeNode::Leaf { space, .. } => space,
            TreeNode::Sub { space, .. } => space,
        }
    }

    pub fn axis(&self) -> Option<usize> {
        match self {
            TreeNode::Leaf { .. } => None,
            TreeNode::Sub { axis, .. } => *axis,
        }
    }

    pub fn children(&self) -> &[TreeNode] {
        match self {
            TreeNode::Leaf { .. } => &[],
            TreeNode::Sub { children, .. } => children,
        }
    }

    fn center(&self, d: usize) -> i64 {
        let s = self.space();
        (s[d][0] + s[d][1]) / 2
    }

    fn magnitude(&self) -> i64 {
        let mut sum = 0i64;
        for d in 0..DIMENSIONS {
            sum += self.center(d).abs();
        }
        sum
    }

    fn leaf_space(a: &[i64; DIMENSIONS], b: &[i64; DIMENSIONS]) -> [[i64; 2]; DIMENSIONS] {
        let mut space = [[0i64; 2]; DIMENSIONS];
        for (d, slot) in space.iter_mut().enumerate() {
            *slot = [a[d], b[d]];
        }
        space
    }
}

fn bounding(nodes: &[TreeNode]) -> [[i64; 2]; DIMENSIONS] {
    let mut space = [[i64::MAX, i64::MIN]; DIMENSIONS];
    for node in nodes {
        for (slot, sd) in space.iter_mut().zip(node.space().iter()) {
            if sd[0] < slot[0] {
                slot[0] = sd[0];
            }
            if sd[1] > slot[1] {
                slot[1] = sd[1];
            }
        }
    }
    space
}

fn cost(space: &[[i64; 2]; DIMENSIONS]) -> i64 {
    let mut sum = 0i64;
    for pair in space {
        sum += (pair[1] - pair[0]).abs();
    }
    sum
}

fn sort_nodes(nodes: &mut [TreeNode], axis: usize, abs: bool) {
    let key = |node: &TreeNode, d: usize| -> i64 {
        let c = node.center(d);
        if abs { c.abs() } else { c }
    };
    nodes.sort_by(|p, q| {
        for i in 0..DIMENSIONS {
            let d = (axis + i) % DIMENSIONS;
            let kp = key(p, d);
            let kq = key(q, d);
            match kp.cmp(&kq) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        }
        std::cmp::Ordering::Equal
    });
}

fn bucketize(nodes: Vec<TreeNode>) -> Vec<Vec<TreeNode>> {
    let g = largest_pow19_below(nodes.len());
    let mut buckets: Vec<Vec<TreeNode>> = Vec::new();
    let mut current: Vec<TreeNode> = Vec::new();
    for node in nodes {
        current.push(node);
        if current.len() >= g {
            buckets.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        buckets.push(current);
    }
    buckets
}

fn build(nodes: Vec<TreeNode>) -> TreeNode {
    let n = nodes.len();
    if n == 1 {
        return nodes.into_iter().next().unwrap();
    }
    if n <= STEP_THRESHOLD {
        let mut children = nodes;
        children.sort_by_key(|a| a.magnitude());
        let space = bounding(&children);
        return TreeNode::Sub {
            children,
            space,
            axis: None,
        };
    }

    let mut best_cost = i64::MAX;
    let mut best_axis = 0usize;
    let mut best_buckets: Option<Vec<Vec<TreeNode>>> = None;
    for axis in 0..DIMENSIONS {
        let mut sorted = nodes.clone();
        sort_nodes(&mut sorted, axis, false);
        let buckets = bucketize(sorted);
        let mut cost_sum = 0i64;
        for bucket in &buckets {
            cost_sum += cost(&bounding(bucket));
        }
        if best_cost > cost_sum {
            best_cost = cost_sum;
            best_axis = axis;
            best_buckets = Some(buckets);
        }
    }

    let mut subs: Vec<TreeNode> = best_buckets
        .unwrap()
        .into_iter()
        .map(|bucket| {
            let space = bounding(&bucket);
            TreeNode::Sub {
                children: bucket,
                space,
                axis: None,
            }
        })
        .collect();
    sort_nodes(&mut subs, best_axis, true);

    let children: Vec<TreeNode> = subs
        .into_iter()
        .map(|sub| match sub {
            TreeNode::Sub { children, .. } => build(children),
            leaf => leaf,
        })
        .collect();
    let space = bounding(&children);
    TreeNode::Sub {
        children,
        space,
        axis: Some(best_axis),
    }
}

pub struct BuiltTree {
    pub root: TreeNode,
    pub order: Vec<usize>,
    pub rank: Vec<u16>,
}

impl BuiltTree {
    pub fn root_axis(&self) -> Option<usize> {
        self.root.axis()
    }

    pub fn height(&self) -> usize {
        node_height(&self.root)
    }

    pub fn leaf_count(&self) -> usize {
        self.order.len()
    }

    pub fn child_sizes(node: &TreeNode) -> Vec<usize> {
        node.children().iter().map(leaf_count_of).collect()
    }

    pub fn axes_at_depth(&self, depth: usize) -> Vec<usize> {
        let mut out = Vec::new();
        collect_axes(&self.root, 0, depth, &mut out);
        out
    }

    pub fn distinct_axes_at_depth(&self, depth: usize) -> usize {
        let mut seen = [false; DIMENSIONS];
        for a in self.axes_at_depth(depth) {
            seen[a] = true;
        }
        seen.iter().filter(|&&b| b).count()
    }

    pub fn order_fingerprint(&self) -> u64 {
        let mut h: u64 = 1125899906842597;
        for &e in &self.order {
            h = h.wrapping_mul(31).wrapping_add(e as u64);
        }
        h
    }
}

fn collect_axes(node: &TreeNode, depth: usize, want: usize, out: &mut Vec<usize>) {
    if depth == want {
        if let Some(a) = node.axis() {
            out.push(a);
        }
        return;
    }
    for child in node.children() {
        collect_axes(child, depth + 1, want, out);
    }
}

fn node_height(node: &TreeNode) -> usize {
    match node {
        TreeNode::Leaf { .. } => 0,
        TreeNode::Sub { children, .. } => 1 + children.iter().map(node_height).max().unwrap_or(0),
    }
}

fn leaf_count_of(node: &TreeNode) -> usize {
    match node {
        TreeNode::Leaf { .. } => 1,
        TreeNode::Sub { children, .. } => children.iter().map(leaf_count_of).sum(),
    }
}

fn collect(node: &TreeNode, order: &mut Vec<usize>) {
    match node {
        TreeNode::Leaf { entry, .. } => order.push(*entry),
        TreeNode::Sub { children, .. } => {
            for child in children {
                collect(child, order);
            }
        }
    }
}

pub fn build_tree(boxes: &[ParamBox]) -> BuiltTree {
    let leaves: Vec<TreeNode> = boxes
        .iter()
        .enumerate()
        .map(|(entry, bx)| {
            let space = TreeNode::leaf_space(&bx.a, &bx.b);
            TreeNode::Leaf { entry, space }
        })
        .collect();
    let root = build(leaves);
    let mut order = Vec::with_capacity(boxes.len());
    collect(&root, &mut order);
    let mut rank = vec![0u16; boxes.len()];
    for (pos, &entry) in order.iter().enumerate() {
        rank[entry] = pos as u16;
    }
    BuiltTree { root, order, rank }
}

#[cfg(test)]
mod tests {
    use super::largest_pow19_below;

    #[test]
    fn pow19_reference_values() {
        assert_eq!(largest_pow19_below(7594), 6859);
        assert_eq!(largest_pow19_below(6859), 361);
        assert_eq!(largest_pow19_below(735), 361);
        assert_eq!(largest_pow19_below(361), 19);
        assert_eq!(largest_pow19_below(19), 1);
        assert_eq!(largest_pow19_below(20), 19);
    }
}
