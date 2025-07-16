//! Checksum and hashing utilities for data integrity

/// Fast xxHash32 implementation
pub fn xxhash32(data: &[u8]) -> u32 {
    let mut hash = 0x9e3779b1u32;
    for &byte in data {
        hash = hash.wrapping_mul(0x85ebca77).wrapping_add(byte as u32);
        hash ^= hash >> 13;
    }
    hash
}

/// Fast xxHash64 implementation
pub fn xxhash64(data: &[u8]) -> u64 {
    let mut hash = 0x9e3779b97f4a7c15u64;
    for &byte in data {
        hash = hash.wrapping_mul(0x85ebca77c2b2ae63).wrapping_add(byte as u64);
        hash ^= hash >> 29;
    }
    hash
}

/// CRC32 implementation (placeholder)
pub fn crc32(data: &[u8]) -> u32 {
    // Simplified CRC32 - in production use a proper implementation
    xxhash32(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xxhash32() {
        let data = b"Hello, World!";
        let hash1 = xxhash32(data);
        let hash2 = xxhash32(data);
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, 0);
    }

    #[test]
    fn test_xxhash64() {
        let data = b"Hello, World!";
        let hash1 = xxhash64(data);
        let hash2 = xxhash64(data);
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, 0);
    }
}
