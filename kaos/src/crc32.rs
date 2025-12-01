use crc32fast::Hasher;

/// Cross-platform, hardware-accelerated CRC32 (Ethernet polynomial)
pub fn crc32_simd(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

/// Incremental CRC32 (continue from previous CRC value)
pub fn crc32_incremental(initial_crc: u32, data: &[u8]) -> u32 {
    let mut hasher = Hasher::new_with_initial(initial_crc);
    hasher.update(data);
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_consistency() {
        let data = b"Test data for CRC32 consistency";
        let crc1 = crc32_simd(data);
        let crc2 = crc32_simd(data);
        assert_eq!(crc1, crc2, "CRC32 should be consistent for same input");
    }

    #[test]
    fn test_crc32_incremental() {
        let data1 = b"Hello, ";
        let data2 = b"World!";
        let combined = b"Hello, World!";
        let crc1 = crc32_simd(data1);
        let crc_incremental = crc32_incremental(crc1, data2);
        let crc_combined = crc32_simd(combined);
        assert_eq!(crc_incremental, crc_combined);
    }

    #[test]
    fn test_crc32_empty() {
        let empty: &[u8] = &[];
        let crc = crc32_simd(empty);
        assert_eq!(crc, 0);
    }

    #[test]
    fn test_crc32_single_byte() {
        let data = b"A";
        let crc = crc32_simd(data);
        assert_ne!(crc, 0);
    }

    #[test]
    fn test_crc32_compatibility_with_crc32fast() {
        let data = b"Hello, World!";
        let mut hasher = Hasher::new();
        hasher.update(data);
        let expected = hasher.finalize();
        let actual = crc32_simd(data);
        assert_eq!(expected, actual, "crc32_simd should match crc32fast");
    }
}
