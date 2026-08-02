use phoenix_scene_contract::{GraphGeneration, Manifold};
use serde::Serialize;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ManifoldSwitchReceipt {
    pub contract: &'static str,
    pub generation: GraphGeneration,
    pub from: Manifold,
    pub to: Manifold,
    pub node_count: usize,
    pub positions_bytes: usize,
    pub hot_page_count: u8,
    pub hot_page_bytes: u64,
    pub page_verifications: u64,
    pub cpu_us: u128,
    pub first_present_us: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct GraphGpuTelemetry {
    pub node_capacity: usize,
    pub edge_capacity: usize,
    pub node_buffer_generation: u64,
    pub edge_buffer_generation: u64,
    pub node_product_capacity: usize,
    pub edge_product_capacity: usize,
    pub node_product_buffer_generation: u64,
    pub edge_product_buffer_generation: u64,
    pub product_index_bound: bool,
    pub viewport_revision: u64,
    pub lens_uniform_writes: u64,
    pub allocated_bytes: usize,
    pub active_manifold: Manifold,
    pub manifold_switches: u64,
    pub switch_cpu_p95_us: u128,
    pub switch_present_p95_us: u128,
    pub max_hot_page_bytes: u64,
    pub latest_switch: Option<ManifoldSwitchReceipt>,
}

pub(super) struct PendingManifoldSwitch {
    pub(super) receipt: ManifoldSwitchReceipt,
    pub(super) started: Instant,
}

pub(super) struct FixedSamples {
    values: [u128; 256],
    len: usize,
    next: usize,
}

impl FixedSamples {
    pub(super) const fn new() -> Self {
        Self {
            values: [0; 256],
            len: 0,
            next: 0,
        }
    }

    pub(super) fn push(&mut self, value: u128) {
        self.values[self.next] = value;
        self.next = (self.next + 1) % self.values.len();
        self.len = self.len.saturating_add(1).min(self.values.len());
    }

    pub(super) fn p95(&self) -> u128 {
        if self.len == 0 {
            return 0;
        }
        let mut values = [0_u128; 256];
        values[..self.len].copy_from_slice(&self.values[..self.len]);
        values[..self.len].sort_unstable();
        values[(self.len * 95).div_ceil(100).saturating_sub(1)]
    }
}
