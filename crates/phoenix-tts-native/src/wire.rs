//! Fixed little-endian header. Audio is at most 2048 mono s16le frames.
pub const HEADER: usize = 32;
pub const READY: u32 = 0;
pub const STARTED: u32 = 1;
pub const AUDIO: u32 = 2;
pub const EOS: u32 = 3;
pub const QUIET: u32 = 4;
pub const LIMIT: u32 = 5;
pub const FAILED: u32 = 6;
pub const REQUEST: u32 = 10;
pub const BARRIER: u32 = 11;
pub const VOICED_REQUEST: u32 = 12;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: u32,
    pub request: u64,
    pub sequence: u64,
    pub value: u64,
}
impl Header {
    pub fn encode(self) -> [u8; HEADER] {
        let mut b = [0; HEADER];
        b[..4].copy_from_slice(b"PBN1");
        b[4..8].copy_from_slice(&self.kind.to_le_bytes());
        b[8..16].copy_from_slice(&self.request.to_le_bytes());
        b[16..24].copy_from_slice(&self.sequence.to_le_bytes());
        b[24..32].copy_from_slice(&self.value.to_le_bytes());
        b
    }
    pub fn decode(b: [u8; HEADER]) -> crate::Result<Self> {
        if &b[..4] != b"PBN1" {
            return Err(crate::Error::Invalid("wire magic"));
        }
        Ok(Self {
            kind: u32::from_le_bytes(b[4..8].try_into().unwrap()),
            request: u64::from_le_bytes(b[8..16].try_into().unwrap()),
            sequence: u64::from_le_bytes(b[16..24].try_into().unwrap()),
            value: u64::from_le_bytes(b[24..32].try_into().unwrap()),
        })
    }
}
