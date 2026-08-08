use phoenix_memory_contract::VerifiedGraphGenerationV3;
use phoenix_memory_runtime::{
    CurrentMemoryProjectionV1, MemoryCatalogV1, MemoryRuntimeError, PolicyDecisionLedgerV1,
    VerifiedCurrentMemoryProjectionV1, WorkingSetGraphV1, CURRENT_MEMORY_PROJECTION_EXTENSION,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResidentMemoryRuntimeSnapshotV1 {
    pub registered: bool,
    pub ledger_sequence: u64,
    pub projection_hash: Option<[u8; 32]>,
    pub projected_records: u32,
    pub active_records: u32,
    pub working_nodes: u32,
    pub working_edges: u32,
}

pub(super) struct RegisteredMemoryRuntimeV1 {
    root: PathBuf,
    ledger: PolicyDecisionLedgerV1,
    verified_projection: Option<VerifiedCurrentMemoryProjectionV1>,
    working_set: Option<WorkingSetGraphV1>,
}

impl RegisteredMemoryRuntimeV1 {
    pub(super) fn open(root: impl AsRef<Path>) -> Result<Self, MemoryRuntimeError> {
        let root = root.as_ref().to_path_buf();
        let ledger = PolicyDecisionLedgerV1::open(root.join("policy-ledger"))?;
        Ok(Self {
            root,
            ledger,
            verified_projection: None,
            working_set: None,
        })
    }

    pub(super) fn install_generation(
        &mut self,
        generation: &VerifiedGraphGenerationV3,
    ) -> Result<(), MemoryRuntimeError> {
        let catalog = MemoryCatalogV1::from_generation(generation)?;
        let projection = CurrentMemoryProjectionV1::materialize(&catalog, &self.ledger)?;
        let valid_at_millis = self
            .ledger
            .receipts()
            .iter()
            .map(|receipt| receipt.header().effective_at_unix_millis)
            .max()
            .unwrap_or(0);
        let working_set = WorkingSetGraphV1::build(
            generation,
            &projection,
            valid_at_millis,
            self.ledger.sequence(),
        )?;
        let projection_path = projection_path(
            &self.root,
            catalog.source_generation_hash(),
            self.ledger.sequence(),
        );
        let verified_projection = projection.publish(projection_path)?;

        self.verified_projection = Some(verified_projection);
        self.working_set = Some(working_set);
        Ok(())
    }

    pub(super) fn snapshot(&self) -> ResidentMemoryRuntimeSnapshotV1 {
        let projection = self.verified_projection.as_ref();
        ResidentMemoryRuntimeSnapshotV1 {
            registered: true,
            ledger_sequence: self.ledger.sequence(),
            projection_hash: projection.map(|value| value.header().projection_hash),
            projected_records: projection.map_or(0, |value| {
                u32::try_from(value.header().record_count).unwrap_or(u32::MAX)
            }),
            active_records: projection.map_or(0, |value| value.header().active_count),
            working_nodes: self.working_set.as_ref().map_or(0, |graph| {
                u32::try_from(graph.nodes().len()).unwrap_or(u32::MAX)
            }),
            working_edges: self.working_set.as_ref().map_or(0, |graph| {
                u32::try_from(graph.edges().len()).unwrap_or(u32::MAX)
            }),
        }
    }
}

fn projection_path(root: &Path, generation_hash: [u8; 32], ledger_sequence: u64) -> PathBuf {
    let mut name =
        String::with_capacity(64 + 1 + 20 + 1 + CURRENT_MEMORY_PROJECTION_EXTENSION.len());
    for byte in generation_hash {
        write!(&mut name, "{byte:02x}").expect("writing to String cannot fail");
    }
    write!(
        &mut name,
        "-{ledger_sequence:020}.{CURRENT_MEMORY_PROJECTION_EXTENSION}"
    )
    .expect("writing to String cannot fail");
    root.join("projections").join(name)
}

#[cfg(test)]
mod tests {
    use super::projection_path;
    use std::path::Path;

    #[test]
    fn projection_names_bind_generation_and_ledger_sequence() {
        let path = projection_path(Path::new("runtime"), [0xab; 32], 17);
        assert_eq!(
            path.file_name().and_then(|value| value.to_str()),
            Some(
                "abababababababababababababababababababababababababababababababab-00000000000000000017.phxmemory"
            )
        );
    }
}
