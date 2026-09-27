//! Document flow: a breadth-first cascade from every document node through
//! the visible graph, ported from the Tauri atlas walk.
//!
//! Each visible edge is oriented from structure toward detail (document,
//! chapter, paragraph, sentence or chunk, evidence and facts, entities) and
//! walked at most once. Branches carry the depth of the wave that walks them;
//! every branch at one depth travels together. Documents are interleaved
//! round-robin per depth so the branch cap never starves a document.

use phoenix_scene_contract::FamilyMask;
use std::collections::VecDeque;

/// Deepest wave walked from a document.
pub(crate) const MAX_FLOW_DEPTH: u32 = 8;
/// Nodes one document's cascade may reach.
pub(crate) const MAX_FLOW_VISITED: usize = 16_384;
/// Branches in one flow across all documents.
pub(crate) const MAX_FLOW_BRANCHES: usize = 16_384;
/// Rank for inactive or unknown slots; they never join a flow.
pub(crate) const RANK_NONE: u8 = u8::MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FlowBranch {
    pub edge: u32,
    pub source: u32,
    pub target: u32,
    /// Wave that walks this branch; 0 leaves a document.
    pub depth: u32,
}

/// Hierarchy rank from a published family mask, structure before detail.
pub(crate) fn rank_for_family(mask: u64) -> u8 {
    let has = |family: FamilyMask| mask & family.0 != 0;
    if has(FamilyMask::DOCUMENTS) {
        0
    } else if has(FamilyMask::CHAPTERS) || has(FamilyMask::EPISODES) {
        1
    } else if has(FamilyMask::PARAGRAPHS) {
        2
    } else if has(FamilyMask::SENTENCES) || has(FamilyMask::CHUNKS) {
        3
    } else if has(FamilyMask::EVIDENCE)
        || has(FamilyMask::EVENT_FACTS)
        || has(FamilyMask::RELATIONSHIP_FACTS)
        || has(FamilyMask::TEMPORAL_FACTS)
        || has(FamilyMask::CAUSAL_FACTS)
        || has(FamilyMask::MEMORY_STATE_FACTS)
        || has(FamilyMask::IDENTITY_DISCOURSE)
        || has(FamilyMask::CONTEXTUAL_DISCOURSE)
    {
        4
    } else if mask & FamilyMask::ENTITY_LANES.0 != 0 {
        5
    } else {
        6
    }
}

/// `ranks[slot]` is `RANK_NONE` for hidden nodes; `edges[slot]` is `None`
/// for hidden edges. `ids` break ties so the flow is deterministic.
pub(crate) fn build_flow(ranks: &[u8], ids: &[u64], edges: &[Option<(u32, u32)>]) -> Vec<FlowBranch> {
    let node_count = ranks.len();
    let mut adjacency: Vec<Vec<(u32, u32)>> = vec![Vec::new(); node_count];
    for (edge, endpoints) in edges.iter().enumerate() {
        let Some((a, b)) = *endpoints else { continue };
        let (a_index, b_index) = (a as usize, b as usize);
        if a == b
            || a_index >= node_count
            || b_index >= node_count
            || ranks[a_index] == RANK_NONE
            || ranks[b_index] == RANK_NONE
        {
            continue;
        }
        let (source, target) = if ranks[a_index] > ranks[b_index] { (b, a) } else { (a, b) };
        adjacency[source as usize].push((edge as u32, target));
    }
    for arcs in &mut adjacency {
        arcs.sort_unstable_by_key(|&(edge, target)| (ranks[target as usize], ids[target as usize], edge));
    }

    let mut roots: Vec<u32> = (0..node_count as u32)
        .filter(|&slot| ranks[slot as usize] == 0)
        .collect();
    roots.sort_unstable_by_key(|&slot| ids[slot as usize]);
    let forests: Vec<Vec<FlowBranch>> = roots
        .iter()
        .map(|&root| branches_for_root(ranks, &adjacency, root))
        .collect();

    let max_depth = forests
        .iter()
        .flat_map(|forest| forest.iter().map(|branch| branch.depth + 1))
        .max()
        .unwrap_or(0);
    let mut emitted = vec![false; edges.len()];
    let mut branches = Vec::new();
    for depth in 0..max_depth {
        let buckets: Vec<Vec<FlowBranch>> = forests
            .iter()
            .map(|forest| forest.iter().copied().filter(|b| b.depth == depth).collect())
            .collect();
        let width = buckets.iter().map(Vec::len).max().unwrap_or(0);
        for index in 0..width {
            for bucket in &buckets {
                let Some(branch) = bucket.get(index) else { continue };
                if std::mem::replace(&mut emitted[branch.edge as usize], true) {
                    continue;
                }
                branches.push(*branch);
                if branches.len() >= MAX_FLOW_BRANCHES {
                    return branches;
                }
            }
        }
    }
    branches
}

fn branches_for_root(ranks: &[u8], adjacency: &[Vec<(u32, u32)>], root: u32) -> Vec<FlowBranch> {
    let mut queue = VecDeque::from([(root, 0u32)]);
    let mut node_depth = vec![u32::MAX; ranks.len()];
    node_depth[root as usize] = 0;
    let mut visited = 1usize;
    let mut seen_edges = std::collections::HashSet::new();
    let mut branches = Vec::new();
    while let Some((node, depth)) = queue.pop_front() {
        if visited >= MAX_FLOW_VISITED {
            break;
        }
        if depth >= MAX_FLOW_DEPTH {
            continue;
        }
        for &(edge, target) in &adjacency[node as usize] {
            if seen_edges.contains(&edge) {
                continue;
            }
            // Other documents start their own cascades.
            if target != root && ranks[target as usize] == 0 {
                continue;
            }
            let next = depth + 1;
            let known = node_depth[target as usize];
            if known != u32::MAX && known < next {
                continue;
            }
            seen_edges.insert(edge);
            branches.push(FlowBranch {
                edge,
                source: node,
                target,
                depth,
            });
            if known == u32::MAX {
                node_depth[target as usize] = next;
                visited += 1;
                queue.push_back((target, next));
            }
        }
    }
    branches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_follow_structure_before_detail() {
        assert_eq!(rank_for_family(FamilyMask::DOCUMENTS.0), 0);
        assert_eq!(rank_for_family(FamilyMask::CHAPTERS.0), 1);
        assert_eq!(rank_for_family(FamilyMask::PARAGRAPHS.0), 2);
        assert_eq!(rank_for_family(FamilyMask::CHUNKS.0), 3);
        assert_eq!(
            rank_for_family(FamilyMask::EVIDENCE.0 | FamilyMask::CHARACTERS.0),
            4
        );
        assert_eq!(rank_for_family(FamilyMask::CHARACTERS.0), 5);
    }

    #[test]
    fn cascade_walks_waves_from_each_document_and_orients_edges() {
        // 0 doc A, 1 doc B, 2 chapter, 3 paragraph, 4 entity, 5 hidden.
        let ranks = [0, 0, 1, 2, 5, RANK_NONE];
        let ids = [10, 11, 12, 13, 14, 15];
        let edges = [
            Some((2, 0)), // chapter-doc, stored backwards
            Some((2, 3)),
            Some((3, 4)),
            Some((1, 4)), // doc B reaches the entity directly
            Some((4, 5)), // into a hidden node
            None,         // hidden edge
        ];
        let flow = build_flow(&ranks, &ids, &edges);
        assert_eq!(
            flow,
            vec![
                FlowBranch { edge: 0, source: 0, target: 2, depth: 0 },
                FlowBranch { edge: 3, source: 1, target: 4, depth: 0 },
                FlowBranch { edge: 1, source: 2, target: 3, depth: 1 },
                FlowBranch { edge: 2, source: 3, target: 4, depth: 2 },
            ]
        );
        assert_eq!(build_flow(&ranks, &ids, &edges), flow);
    }

    #[test]
    fn documents_never_walk_into_each_other_and_empty_graphs_are_quiet() {
        let flow = build_flow(&[0, 0], &[1, 2], &[Some((0, 1))]);
        assert!(flow.is_empty());
        assert!(build_flow(&[], &[], &[]).is_empty());
    }
}
