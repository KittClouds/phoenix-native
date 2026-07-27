#[derive(Clone, Copy, Debug, Default)]
pub struct FrameMetrics {
    pub frame_number: u64,
    pub cpu_encode_submit_us: u128,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LensUpdateMetrics {
    pub bytes_uploaded: usize,
    pub uniform_writes: u64,
    pub topology_buffer_generation_before: u64,
    pub topology_buffer_generation_after: u64,
}
