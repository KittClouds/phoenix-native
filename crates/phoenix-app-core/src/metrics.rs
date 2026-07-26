use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelMetrics {
    pub commands_submitted: u64,
    pub commands_completed: u64,
    pub commands_rejected: u64,
    pub events_published: u64,
    pub last_sequence: u64,
    pub commands_pending: u64,
    pub command_queue_high_water: u64,
    pub worker_exited: bool,
}

#[derive(Default)]
pub(super) struct KernelMetricAtoms {
    pub(super) commands_submitted: AtomicU64,
    pub(super) commands_completed: AtomicU64,
    pub(super) commands_rejected: AtomicU64,
    pub(super) events_published: AtomicU64,
    pub(super) last_sequence: AtomicU64,
    pub(super) commands_pending: AtomicU64,
    pub(super) command_queue_high_water: AtomicU64,
    pub(super) worker_exited: AtomicBool,
}

impl KernelMetricAtoms {
    pub(super) fn snapshot(&self) -> KernelMetrics {
        KernelMetrics {
            commands_submitted: self.commands_submitted.load(Ordering::Relaxed),
            commands_completed: self.commands_completed.load(Ordering::Relaxed),
            commands_rejected: self.commands_rejected.load(Ordering::Relaxed),
            events_published: self.events_published.load(Ordering::Relaxed),
            last_sequence: self.last_sequence.load(Ordering::Relaxed),
            commands_pending: self.commands_pending.load(Ordering::Relaxed),
            command_queue_high_water: self.command_queue_high_water.load(Ordering::Relaxed),
            worker_exited: self.worker_exited.load(Ordering::Acquire),
        }
    }
}
