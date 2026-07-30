use super::{GraphGpuTelemetry, InteractionStressProof};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;

pub(super) enum GraphWindowCommand {
    SyncKernelState,
    FitGraph,
    ResetCamera,
    ResetSwitchTelemetry,
    StressInteraction(SyncSender<Result<InteractionStressProof, String>>),
    PickProbePoint(SyncSender<Option<(u32, u32)>>),
    RecoverRenderer(SyncSender<Result<(), String>>),
    ProbeFocus(SyncSender<bool>),
    Barrier(SyncSender<()>),
    Telemetry(SyncSender<GraphGpuTelemetry>),
    Shutdown,
}

#[derive(Default)]
pub(super) struct GraphQueueMetrics {
    pub(super) pending: AtomicU64,
    pub(super) high_water: AtomicU64,
}

impl GraphQueueMetrics {
    pub(super) fn begin_send(&self) {
        let pending = self
            .pending
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        self.high_water.fetch_max(pending, Ordering::Relaxed);
    }

    pub(super) fn cancel_send(&self) {
        self.pending.fetch_sub(1, Ordering::Relaxed);
    }

    pub(super) fn received(&self) {
        self.pending.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum GraphWake {
    CommandsReady,
    ViewportReady,
}
