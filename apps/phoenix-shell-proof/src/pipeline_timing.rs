//! Coarse monotonic milestones; no logging in the steady-state frame loop.
use std::{sync::OnceLock, time::Instant};
static ORIGIN: OnceLock<Instant> = OnceLock::new();
pub fn initialize() {
    ORIGIN.get_or_init(Instant::now);
}
pub fn mark(stage: &str, generation: u64, duration_us: u128) {
    let elapsed_us = ORIGIN.get_or_init(Instant::now).elapsed().as_micros();
    eprintln!("PHOENIX_PIPELINE_TIMING stage={stage} generation={generation} process_us={elapsed_us} duration_us={duration_us}");
}
