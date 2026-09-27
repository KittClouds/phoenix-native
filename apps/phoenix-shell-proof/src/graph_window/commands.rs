use super::{GraphGpuTelemetry, InteractionStressProof};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;

pub(super) enum GraphWindowCommand {
    SyncKernelState,
    InspectCaps {
        space_only: bool,
        depth: u8,
        cutaway: bool,
    },
    FitGraph,
    ResetCamera,
    ResetSwitchTelemetry,
    RouteWalk(RouteWalkRequest),
    ReaderGlow(Option<ReaderGlowRequest>),
    StressInteraction(SyncSender<Result<InteractionStressProof, String>>),
    PickProbePoint(SyncSender<Option<(u32, u32)>>),
    RecoverRenderer(SyncSender<Result<(), String>>),
    ProbeFocus(SyncSender<bool>),
    Barrier(SyncSender<()>),
    Telemetry(SyncSender<GraphGpuTelemetry>),
    Shutdown,
}

/// The Reader's current segment as spoken byte ranges in its saved
/// revision. The graph thread resolves them through stored bindings.
#[derive(Clone, Debug)]
pub struct ReaderGlowRequest {
    pub segment: u32,
    pub ranges: std::sync::Arc<[(u32, u32)]>,
    pub playing: bool,
    pub follow: bool,
    pub observed_at: std::time::Instant,
}

/// Shell requests for the guided route walk. The renderer owns the frozen
/// route; the shell only issues transport commands and reads status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteWalkRequest {
    Start,
    Transport(graph_render_wgpu::RouteWalkCommand),
    Exit,
    FlowStart,
    FlowExit,
    StoryStart,
    Story(graph_render_wgpu::StoryCommand),
    StoryExit,
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

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug)]
pub(super) enum GraphWake {
    CommandsReady,
    ViewportReady,
    PickReady,
}
