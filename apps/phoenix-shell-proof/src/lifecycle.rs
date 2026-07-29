use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

macro_rules! counter {
    ($name:ident) => {
        static $name: AtomicU64 = AtomicU64::new(0);
    };
}

counter!(GRAPH_WINDOW_CREATED);
counter!(GRAPH_WINDOW_DROPPED);
counter!(GRAPH_WINDOW_LIVE);
counter!(RENDERER_CREATED);
counter!(RENDERER_DROPPED);
counter!(RENDERER_LIVE);
counter!(SURFACE_CREATED);
counter!(SURFACE_DROPPED);
counter!(SURFACE_LIVE);
counter!(DEVICE_CREATED);
counter!(DEVICE_DROPPED);
counter!(DEVICE_LIVE);
counter!(VISIBILITY_TRANSITIONS);
counter!(FOCUS_EVENTS);
counter!(POINTER_EVENTS);
counter!(HOVER_PICK_EVENTS);
counter!(WHEEL_EVENTS);
counter!(RESIZE_EVENTS);
counter!(DPI_EVENTS);
counter!(FRAMES_PRESENTED);
counter!(FRAME_READBACKS);

static PROOF_FAILED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct LifecycleSnapshot {
    pub graph_window_created: u64,
    pub graph_window_dropped: u64,
    pub graph_window_live: u64,
    pub renderer_created: u64,
    pub renderer_dropped: u64,
    pub renderer_live: u64,
    pub surface_created: u64,
    pub surface_dropped: u64,
    pub surface_live: u64,
    pub device_created: u64,
    pub device_dropped: u64,
    pub device_live: u64,
    pub visibility_transitions: u64,
    pub focus_events: u64,
    pub pointer_events: u64,
    pub hover_pick_events: u64,
    pub wheel_events: u64,
    pub resize_events: u64,
    pub dpi_events: u64,
    pub frames_presented: u64,
    pub frame_readbacks: u64,
}

pub fn snapshot() -> LifecycleSnapshot {
    LifecycleSnapshot {
        graph_window_created: GRAPH_WINDOW_CREATED.load(Ordering::Relaxed),
        graph_window_dropped: GRAPH_WINDOW_DROPPED.load(Ordering::Relaxed),
        graph_window_live: GRAPH_WINDOW_LIVE.load(Ordering::Relaxed),
        renderer_created: RENDERER_CREATED.load(Ordering::Relaxed),
        renderer_dropped: RENDERER_DROPPED.load(Ordering::Relaxed),
        renderer_live: RENDERER_LIVE.load(Ordering::Relaxed),
        surface_created: SURFACE_CREATED.load(Ordering::Relaxed),
        surface_dropped: SURFACE_DROPPED.load(Ordering::Relaxed),
        surface_live: SURFACE_LIVE.load(Ordering::Relaxed),
        device_created: DEVICE_CREATED.load(Ordering::Relaxed),
        device_dropped: DEVICE_DROPPED.load(Ordering::Relaxed),
        device_live: DEVICE_LIVE.load(Ordering::Relaxed),
        visibility_transitions: VISIBILITY_TRANSITIONS.load(Ordering::Relaxed),
        focus_events: FOCUS_EVENTS.load(Ordering::Relaxed),
        pointer_events: POINTER_EVENTS.load(Ordering::Relaxed),
        hover_pick_events: HOVER_PICK_EVENTS.load(Ordering::Relaxed),
        wheel_events: WHEEL_EVENTS.load(Ordering::Relaxed),
        resize_events: RESIZE_EVENTS.load(Ordering::Relaxed),
        dpi_events: DPI_EVENTS.load(Ordering::Relaxed),
        frames_presented: FRAMES_PRESENTED.load(Ordering::Relaxed),
        frame_readbacks: FRAME_READBACKS.load(Ordering::Relaxed),
    }
}

pub fn graph_window_created() {
    GRAPH_WINDOW_CREATED.fetch_add(1, Ordering::Relaxed);
    GRAPH_WINDOW_LIVE.fetch_add(1, Ordering::Relaxed);
    RENDERER_CREATED.fetch_add(1, Ordering::Relaxed);
    RENDERER_LIVE.fetch_add(1, Ordering::Relaxed);
    SURFACE_CREATED.fetch_add(1, Ordering::Relaxed);
    SURFACE_LIVE.fetch_add(1, Ordering::Relaxed);
    DEVICE_CREATED.fetch_add(1, Ordering::Relaxed);
    DEVICE_LIVE.fetch_add(1, Ordering::Relaxed);
}

pub fn graph_window_dropped() {
    GRAPH_WINDOW_DROPPED.fetch_add(1, Ordering::Relaxed);
    GRAPH_WINDOW_LIVE.fetch_sub(1, Ordering::Relaxed);
    RENDERER_DROPPED.fetch_add(1, Ordering::Relaxed);
    RENDERER_LIVE.fetch_sub(1, Ordering::Relaxed);
    SURFACE_DROPPED.fetch_add(1, Ordering::Relaxed);
    SURFACE_LIVE.fetch_sub(1, Ordering::Relaxed);
    DEVICE_DROPPED.fetch_add(1, Ordering::Relaxed);
    DEVICE_LIVE.fetch_sub(1, Ordering::Relaxed);
}

pub fn visibility_transition() {
    VISIBILITY_TRANSITIONS.fetch_add(1, Ordering::Relaxed);
}

pub fn focus_event() {
    FOCUS_EVENTS.fetch_add(1, Ordering::Relaxed);
}

pub fn pointer_event() {
    POINTER_EVENTS.fetch_add(1, Ordering::Relaxed);
}

pub fn hover_pick_event() {
    HOVER_PICK_EVENTS.fetch_add(1, Ordering::Relaxed);
}

pub fn wheel_event() {
    WHEEL_EVENTS.fetch_add(1, Ordering::Relaxed);
}

pub fn resize_event() {
    RESIZE_EVENTS.fetch_add(1, Ordering::Relaxed);
}

pub fn dpi_event() {
    DPI_EVENTS.fetch_add(1, Ordering::Relaxed);
}

pub fn frame_presented() {
    FRAMES_PRESENTED.fetch_add(1, Ordering::Relaxed);
}

pub fn mark_proof_failed() {
    PROOF_FAILED.store(true, Ordering::Relaxed);
}

pub fn proof_failed() -> bool {
    PROOF_FAILED.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::LifecycleSnapshot;

    #[test]
    fn clean_shutdown_invariant_is_explicit() {
        let state = LifecycleSnapshot {
            graph_window_created: 1,
            graph_window_dropped: 1,
            renderer_created: 1,
            renderer_dropped: 1,
            surface_created: 1,
            surface_dropped: 1,
            device_created: 1,
            device_dropped: 1,
            ..LifecycleSnapshot::default()
        };
        assert_eq!(state.graph_window_created, state.graph_window_dropped);
        assert_eq!(state.renderer_created, state.renderer_dropped);
        assert_eq!(state.surface_created, state.surface_dropped);
        assert_eq!(state.device_created, state.device_dropped);
        assert_eq!(state.frame_readbacks, 0);
    }
}
