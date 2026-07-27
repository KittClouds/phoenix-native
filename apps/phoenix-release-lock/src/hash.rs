use std::fmt::Write as _;

const LOW_OFFSET: u32 = 0x811c_9dc5;
const HIGH_OFFSET: u32 = 0x9e37_79b9;
const LOW_PRIME: u32 = 0x0100_0193;
const HIGH_PRIME: u32 = 0x85eb_ca6b;

pub fn galaxy_hash(bytes: &[u8]) -> String {
    let mut low = LOW_OFFSET;
    let mut high = HIGH_OFFSET;
    for &byte in bytes {
        low = (low ^ u32::from(byte)).wrapping_mul(LOW_PRIME);
        high = (high ^ u32::from(byte)).wrapping_mul(HIGH_PRIME);
    }
    format!("fnv1a64:{high:08x}{low:08x}")
}

pub fn hex_hash(hash: [u8; 32]) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in hash {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::galaxy_hash;

    #[test]
    fn matches_angular_empty_page_hash() {
        assert_eq!(galaxy_hash(&[]), "fnv1a64:9e3779b9811c9dc5");
    }
}
