use crate::{CurrentMemoryProjectionV1, MemoryRuntimeError};
use hashbrown::HashMap;
use phoenix_memory_contract::{
    CandidateEndpointBindingRecordV3, PageKindV3, SemanticCandidateRecordV3,
    VerifiedGraphGenerationV3,
};

pub const MAX_RECURSIVE_DEPTH: u8 = 8;
pub const MAX_RECURSIVE_NODES: u32 = 4_096;
pub const MAX_RECURSIVE_EDGES: u32 = 65_536;
pub const MAX_WORKING_SET_BUILD_NODES: usize = 8_000_000;
pub const MAX_WORKING_SET_BUILD_EDGES: usize = 64_000_000;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct WorkingNodeId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum WorkingNodeKeyV1 {
    Entity(u64),
    Candidate([u8; 32]),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum WorkingNodeKindV1 {
    Entity = 1,
    Candidate = 2,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum WorkingEdgeKindV1 {
    CandidateEndpoint = 1,
    EndpointCandidate = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkingNodeRecordV1 {
    pub candidate_id: [u8; 32],
    pub entity_id: u64,
    pub kind: WorkingNodeKindV1,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WorkingEdgeRecordV1 {
    pub target: WorkingNodeId,
    pub kind: WorkingEdgeKindV1,
    pub endpoint_role: u16,
    pub flags: u16,
}

#[derive(Clone, Debug)]
pub struct WorkingSetGraphV1 {
    nodes: Box<[WorkingNodeRecordV1]>,
    edge_offsets: Box<[u32]>,
    edges: Box<[WorkingEdgeRecordV1]>,
    entity_count: u32,
}

impl WorkingSetGraphV1 {
    pub fn build(
        generation: &VerifiedGraphGenerationV3,
        projection: &CurrentMemoryProjectionV1,
        valid_at_millis: i64,
        ledger_sequence: u64,
    ) -> Result<Self, MemoryRuntimeError> {
        let candidates =
            generation.typed_page::<SemanticCandidateRecordV3>(PageKindV3::SemanticCandidates)?;
        let endpoints = generation.typed_page::<CandidateEndpointBindingRecordV3>(
            PageKindV3::CandidateEndpointBindings,
        )?;
        let active = projection
            .active_at(valid_at_millis, ledger_sequence)
            .collect::<Vec<_>>();
        if active.len() > u32::MAX as usize {
            return Err(MemoryRuntimeError::OversizedWorkingSet);
        }
        let mut entity_ids = Vec::new();
        let mut upper_node_count = active.len();
        let mut edge_count = 0_usize;
        for memory in &active {
            let candidate = candidates
                .get(memory.candidate_row_index as usize)
                .filter(|candidate| candidate.candidate_id == memory.candidate_id)
                .ok_or(MemoryRuntimeError::InvalidProjection(
                    "projected candidate row no longer matches the generation",
                ))?;
            let range = &endpoints[candidate.endpoint_start as usize
                ..(candidate.endpoint_start + candidate.endpoint_count) as usize];
            upper_node_count = upper_node_count
                .checked_add(range.len())
                .ok_or(MemoryRuntimeError::OversizedWorkingSet)?;
            edge_count = edge_count
                .checked_add(range.len().saturating_mul(2))
                .ok_or(MemoryRuntimeError::OversizedWorkingSet)?;
            if upper_node_count > MAX_WORKING_SET_BUILD_NODES
                || edge_count > MAX_WORKING_SET_BUILD_EDGES
            {
                return Err(MemoryRuntimeError::OversizedWorkingSet);
            }
            entity_ids.extend(range.iter().map(|endpoint| endpoint.endpoint_id));
        }
        entity_ids.sort_unstable();
        entity_ids.dedup();
        let node_count = entity_ids.len().saturating_add(active.len());
        if node_count > u32::MAX as usize {
            return Err(MemoryRuntimeError::OversizedWorkingSet);
        }
        let entity_count = entity_ids.len() as u32;
        let mut nodes = Vec::with_capacity(node_count);
        let mut entity_nodes = HashMap::with_capacity(entity_ids.len());
        for entity_id in entity_ids {
            let id = WorkingNodeId(nodes.len() as u32);
            entity_nodes.insert(entity_id, id);
            nodes.push(WorkingNodeRecordV1 {
                candidate_id: [0; 32],
                entity_id,
                kind: WorkingNodeKindV1::Entity,
            });
        }
        let mut active = active;
        active.sort_unstable_by_key(|memory| memory.candidate_id);
        let mut candidate_nodes = HashMap::with_capacity(active.len());
        for memory in &active {
            let id = WorkingNodeId(nodes.len() as u32);
            candidate_nodes.insert(memory.candidate_id, id);
            nodes.push(WorkingNodeRecordV1 {
                candidate_id: memory.candidate_id,
                entity_id: 0,
                kind: WorkingNodeKindV1::Candidate,
            });
        }
        let mut adjacency = vec![Vec::<WorkingEdgeRecordV1>::new(); nodes.len()];
        for memory in active {
            let candidate = &candidates[memory.candidate_row_index as usize];
            let candidate_node = candidate_nodes[&memory.candidate_id];
            let range = &endpoints[candidate.endpoint_start as usize
                ..(candidate.endpoint_start + candidate.endpoint_count) as usize];
            for endpoint in range {
                let entity_node = entity_nodes[&endpoint.endpoint_id];
                adjacency[candidate_node.0 as usize].push(WorkingEdgeRecordV1 {
                    target: entity_node,
                    kind: WorkingEdgeKindV1::CandidateEndpoint,
                    endpoint_role: endpoint.role,
                    flags: endpoint.flags,
                });
                adjacency[entity_node.0 as usize].push(WorkingEdgeRecordV1 {
                    target: candidate_node,
                    kind: WorkingEdgeKindV1::EndpointCandidate,
                    endpoint_role: endpoint.role,
                    flags: endpoint.flags,
                });
            }
        }
        let mut edge_offsets = Vec::with_capacity(nodes.len() + 1);
        let mut edges = Vec::new();
        edge_offsets.push(0);
        for neighbors in &mut adjacency {
            neighbors.sort_unstable();
            neighbors.dedup();
            edges.extend_from_slice(neighbors);
            edge_offsets.push(
                u32::try_from(edges.len()).map_err(|_| MemoryRuntimeError::OversizedWorkingSet)?,
            );
        }
        Ok(Self {
            nodes: nodes.into_boxed_slice(),
            edge_offsets: edge_offsets.into_boxed_slice(),
            edges: edges.into_boxed_slice(),
            entity_count,
        })
    }

    pub fn nodes(&self) -> &[WorkingNodeRecordV1] {
        &self.nodes
    }

    pub fn edges(&self) -> &[WorkingEdgeRecordV1] {
        &self.edges
    }

    pub fn node_id(&self, key: WorkingNodeKeyV1) -> Option<WorkingNodeId> {
        match key {
            WorkingNodeKeyV1::Entity(entity_id) => self.nodes[..self.entity_count as usize]
                .binary_search_by_key(&entity_id, |node| node.entity_id)
                .ok()
                .map(|index| WorkingNodeId(index as u32)),
            WorkingNodeKeyV1::Candidate(candidate_id) => self.nodes[self.entity_count as usize..]
                .binary_search_by_key(&candidate_id, |node| node.candidate_id)
                .ok()
                .map(|index| WorkingNodeId(self.entity_count + index as u32)),
        }
    }

    pub fn node(&self, id: WorkingNodeId) -> Option<&WorkingNodeRecordV1> {
        self.nodes.get(id.0 as usize)
    }

    pub fn neighbors(&self, id: WorkingNodeId) -> &[WorkingEdgeRecordV1] {
        let index = id.0 as usize;
        let Some((&start, &end)) = self
            .edge_offsets
            .get(index)
            .zip(self.edge_offsets.get(index + 1))
        else {
            return &[];
        };
        &self.edges[start as usize..end as usize]
    }

    pub fn traverse(
        &self,
        query: RecursiveQueryV1<'_>,
        scratch: &mut RecursiveScratchV1,
    ) -> Result<RecursiveTraversalReceiptV1, MemoryRuntimeError> {
        if query.seeds.is_empty()
            || query.seeds.len() > query.max_nodes as usize
            || query.max_depth > MAX_RECURSIVE_DEPTH
            || query.max_nodes == 0
            || query.max_nodes > MAX_RECURSIVE_NODES
            || query.max_edges == 0
            || query.max_edges > MAX_RECURSIVE_EDGES
            || scratch.marks.len() != self.nodes.len()
            || query.max_nodes > scratch.maximum_nodes
        {
            return Err(MemoryRuntimeError::RecursiveBounds);
        }
        let visited_capacity = scratch.visited.capacity();
        let queue_capacity = scratch.queue.capacity();
        scratch.begin();
        for seed in query.seeds {
            if let Some(id) = self.node_id(*seed) {
                scratch.push_if_new(id, 0);
            }
        }
        scratch.queue.sort_unstable_by_key(|item| item.0);
        scratch.visited.sort_unstable();
        let mut head = 0_usize;
        let mut edges_scanned = 0_u32;
        let mut truncated = false;
        while let Some(&(node, depth)) = scratch.queue.get(head) {
            head += 1;
            if depth >= query.max_depth {
                continue;
            }
            for edge in self.neighbors(node) {
                if edges_scanned == query.max_edges {
                    truncated = true;
                    break;
                }
                edges_scanned += 1;
                if scratch.visited.len() == query.max_nodes as usize {
                    truncated = true;
                    break;
                }
                scratch.push_if_new(edge.target, depth + 1);
            }
            if truncated {
                break;
            }
        }
        Ok(RecursiveTraversalReceiptV1 {
            visited_nodes: scratch.visited.len() as u32,
            edges_scanned,
            reached_depth: scratch.queue.iter().map(|item| item.1).max().unwrap_or(0),
            truncated,
            allocations_grew: scratch.visited.capacity() != visited_capacity
                || scratch.queue.capacity() != queue_capacity,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RecursiveQueryV1<'a> {
    pub seeds: &'a [WorkingNodeKeyV1],
    pub max_depth: u8,
    pub max_nodes: u32,
    pub max_edges: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecursiveTraversalReceiptV1 {
    pub visited_nodes: u32,
    pub edges_scanned: u32,
    pub reached_depth: u8,
    pub truncated: bool,
    pub allocations_grew: bool,
}

#[derive(Clone, Debug)]
pub struct RecursiveScratchV1 {
    marks: Vec<u32>,
    epoch: u32,
    queue: Vec<(WorkingNodeId, u8)>,
    visited: Vec<WorkingNodeId>,
    maximum_nodes: u32,
}

impl RecursiveScratchV1 {
    pub fn new(node_count: usize, maximum_nodes: u32) -> Result<Self, MemoryRuntimeError> {
        if maximum_nodes == 0 || maximum_nodes > MAX_RECURSIVE_NODES {
            return Err(MemoryRuntimeError::RecursiveBounds);
        }
        Ok(Self {
            marks: vec![0; node_count],
            epoch: 0,
            queue: Vec::with_capacity(maximum_nodes as usize),
            visited: Vec::with_capacity(maximum_nodes as usize),
            maximum_nodes,
        })
    }

    pub fn visited(&self) -> &[WorkingNodeId] {
        &self.visited
    }

    fn begin(&mut self) {
        self.queue.clear();
        self.visited.clear();
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.marks.fill(0);
            self.epoch = 1;
        }
    }

    fn push_if_new(&mut self, node: WorkingNodeId, depth: u8) {
        let mark = &mut self.marks[node.0 as usize];
        if *mark == self.epoch {
            return;
        }
        *mark = self.epoch;
        self.queue.push((node, depth));
        self.visited.push(node);
    }
}
