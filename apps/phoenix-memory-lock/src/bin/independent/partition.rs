use hashbrown::HashMap;
use serde::Serialize;

use super::source::{near_duplicate_key, SourceDataset};

/// Owns negative documents by the same positive-evidence component used by the
/// leakage split. A document can be claimed once, preventing mined pairs from
/// joining otherwise independent query/source components after the fact.
pub(super) struct MiningPartitions {
    query_components: HashMap<String, usize>,
    document_owners: Vec<Option<usize>>,
    document_sources: Vec<usize>,
    source_owners: Vec<Option<usize>>,
}

impl MiningPartitions {
    pub(super) fn new(dataset: &SourceDataset) -> Self {
        let mut sets = DisjointSets::new(dataset.queries.len());
        let documents = dataset
            .documents
            .iter()
            .enumerate()
            .map(|(index, document)| (document.id.as_str(), index))
            .collect::<HashMap<_, _>>();
        let mut families = HashMap::<&str, usize>::new();
        let mut entities = HashMap::<&str, usize>::new();
        let mut cohorts = HashMap::<&str, usize>::new();
        let mut positive_documents = HashMap::<usize, usize>::new();
        let mut positive_sources = HashMap::<&str, usize>::new();
        let mut positive_near_duplicates = HashMap::<String, usize>::new();

        for (query_index, query) in dataset.queries.iter().enumerate() {
            union_text_group(&mut sets, &mut families, &query.family, query_index);
            union_text_group(&mut sets, &mut entities, &query.entity_family, query_index);
            union_text_group(
                &mut sets,
                &mut cohorts,
                &query.collection_cohort,
                query_index,
            );
            for document_id in query.relevant.keys() {
                let Some(&document_index) = documents.get(document_id.as_str()) else {
                    continue;
                };
                union_index_group(
                    &mut sets,
                    &mut positive_documents,
                    document_index,
                    query_index,
                );
                let document = &dataset.documents[document_index];
                union_text_group(
                    &mut sets,
                    &mut positive_sources,
                    &document.source_family,
                    query_index,
                );
                let duplicate = near_duplicate_key(document);
                union_owned_group(
                    &mut sets,
                    &mut positive_near_duplicates,
                    duplicate,
                    query_index,
                );
            }
        }

        let query_components = dataset
            .queries
            .iter()
            .enumerate()
            .map(|(index, query)| (query.id.clone(), sets.find(index)))
            .collect::<HashMap<_, _>>();
        let mut document_owners = vec![None; dataset.documents.len()];
        let mut source_indices = HashMap::<&str, usize>::new();
        let mut document_sources = Vec::with_capacity(dataset.documents.len());
        for document in &dataset.documents {
            let next = source_indices.len();
            let source = *source_indices
                .entry(document.source_family.as_str())
                .or_insert(next);
            document_sources.push(source);
        }
        let mut source_owners = vec![None; source_indices.len()];
        for (query_index, query) in dataset.queries.iter().enumerate() {
            let component = sets.find(query_index);
            for document_id in query.relevant.keys() {
                if let Some(&document_index) = documents.get(document_id.as_str()) {
                    document_owners[document_index] = Some(component);
                    source_owners[document_sources[document_index]] = Some(component);
                }
            }
        }
        Self {
            query_components,
            document_owners,
            document_sources,
            source_owners,
        }
    }

    pub(super) fn component(&self, query_id: &str) -> Option<usize> {
        self.query_components.get(query_id).copied()
    }

    pub(super) fn allows(&self, document_index: usize, component: usize) -> bool {
        (self.document_owners[document_index].is_none()
            || self.document_owners[document_index] == Some(component))
            && (self.source_owners[self.document_sources[document_index]].is_none()
                || self.source_owners[self.document_sources[document_index]] == Some(component))
    }

    pub(super) fn claim(&mut self, document_index: usize, component: usize) {
        debug_assert!(self.allows(document_index, component));
        self.document_owners[document_index] = Some(component);
        self.source_owners[self.document_sources[document_index]] = Some(component);
    }

    pub(super) fn audit(&self) -> PartitionAudit {
        let mut sizes = HashMap::<usize, usize>::new();
        for component in self.query_components.values() {
            *sizes.entry(*component).or_default() += 1;
        }
        PartitionAudit {
            queries: self.query_components.len(),
            components: sizes.len(),
            largest_component_queries: sizes.values().copied().max().unwrap_or(0),
            positive_owned_documents: self.document_owners.iter().flatten().count(),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct PartitionAudit {
    queries: usize,
    components: usize,
    largest_component_queries: usize,
    positive_owned_documents: usize,
}

fn union_text_group<'a>(
    sets: &mut DisjointSets,
    groups: &mut HashMap<&'a str, usize>,
    identity: &'a str,
    query: usize,
) {
    if let Some(&other) = groups.get(identity) {
        sets.union(query, other);
    } else {
        groups.insert(identity, query);
    }
}

fn union_owned_group(
    sets: &mut DisjointSets,
    groups: &mut HashMap<String, usize>,
    identity: String,
    query: usize,
) {
    if let Some(&other) = groups.get(&identity) {
        sets.union(query, other);
    } else {
        groups.insert(identity, query);
    }
}

fn union_index_group(
    sets: &mut DisjointSets,
    groups: &mut HashMap<usize, usize>,
    identity: usize,
    query: usize,
) {
    if let Some(&other) = groups.get(&identity) {
        sets.union(query, other);
    } else {
        groups.insert(identity, query);
    }
}

struct DisjointSets {
    parents: Vec<usize>,
    ranks: Vec<u8>,
}

impl DisjointSets {
    fn new(len: usize) -> Self {
        Self {
            parents: (0..len).collect(),
            ranks: vec![0; len],
        }
    }

    fn find(&mut self, value: usize) -> usize {
        let parent = self.parents[value];
        if parent != value {
            self.parents[value] = self.find(parent);
        }
        self.parents[value]
    }

    fn union(&mut self, left: usize, right: usize) {
        let mut left = self.find(left);
        let mut right = self.find(right);
        if left == right {
            return;
        }
        if self.ranks[left] < self.ranks[right] {
            std::mem::swap(&mut left, &mut right);
        }
        self.parents[right] = left;
        if self.ranks[left] == self.ranks[right] {
            self.ranks[left] += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{QueryKind, SourceDocument, SourceQuery};

    #[test]
    fn a_negative_document_cannot_bridge_positive_components() {
        let dataset = SourceDataset {
            name: "test",
            documents: vec![
                document("a", "source-a"),
                document("b", "source-b"),
                document("negative", "source-negative"),
                document("negative-sibling", "source-negative"),
            ],
            queries: vec![query("qa", "a"), query("qb", "b")],
        };
        let mut partitions = MiningPartitions::new(&dataset);
        let a = partitions.component("qa").unwrap();
        let b = partitions.component("qb").unwrap();
        assert_ne!(a, b);
        assert!(partitions.allows(2, a));
        partitions.claim(2, a);
        assert!(!partitions.allows(2, b));
        assert!(!partitions.allows(3, b));
    }

    fn document(id: &str, source_family: &str) -> SourceDocument {
        SourceDocument {
            id: id.to_owned(),
            title: id.to_owned(),
            text: format!("evidence for {id}"),
            source_family: source_family.to_owned(),
            collected_at: 1,
            source_time_label: String::new(),
            reviewer_context: String::new(),
        }
    }

    fn query(id: &str, relevant: &str) -> SourceQuery {
        SourceQuery {
            id: id.to_owned(),
            text: format!("query {id}"),
            reference_answer: String::new(),
            relevant: [(relevant.to_owned(), 4)].into_iter().collect(),
            family: id.to_owned(),
            entity_family: id.to_owned(),
            collection_cohort: id.to_owned(),
            collected_at: 1,
            kind: QueryKind::ScientificClaim,
        }
    }
}
