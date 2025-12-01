//! Data verification utilities for testing correctness.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Verifies data integrity by checking sequence numbers and content.
pub struct DataVerifier {
    /// Expected checksum/hash per sequence
    expected: Mutex<HashMap<u64, u64>>,
    /// Sequences we've verified
    verified: Mutex<HashSet<u64>>,
    /// Count of mismatches
    mismatches: AtomicU64,
    /// Count of duplicates
    duplicates: AtomicU64,
    /// Count of out-of-order
    out_of_order: AtomicU64,
    /// Last verified sequence
    last_verified: AtomicU64,
}

impl DataVerifier {
    pub fn new() -> Self {
        Self {
            expected: Mutex::new(HashMap::new()),
            verified: Mutex::new(HashSet::new()),
            mismatches: AtomicU64::new(0),
            duplicates: AtomicU64::new(0),
            out_of_order: AtomicU64::new(0),
            last_verified: AtomicU64::new(0),
        }
    }

    /// Register expected data for a sequence
    pub fn expect(&self, seq: u64, data: &[u8]) {
        let hash = simple_hash(data);
        self.expected.lock().unwrap().insert(seq, hash);
    }

    /// Verify received data
    pub fn verify(&self, seq: u64, data: &[u8]) -> VerifyResult {
        let hash = simple_hash(data);
        
        // Check for duplicates
        {
            let mut verified = self.verified.lock().unwrap();
            if verified.contains(&seq) {
                self.duplicates.fetch_add(1, Ordering::Relaxed);
                return VerifyResult::Duplicate;
            }
            verified.insert(seq);
        }

        // Check for out-of-order
        let last = self.last_verified.load(Ordering::Relaxed);
        if seq > 0 && seq != last + 1 && last > 0 {
            self.out_of_order.fetch_add(1, Ordering::Relaxed);
        }
        self.last_verified.store(seq, Ordering::Relaxed);

        // Check hash if we have expected data
        if let Some(&expected_hash) = self.expected.lock().unwrap().get(&seq) {
            if hash != expected_hash {
                self.mismatches.fetch_add(1, Ordering::Relaxed);
                return VerifyResult::Mismatch { seq, expected: expected_hash, actual: hash };
            }
        }

        VerifyResult::Ok
    }

    /// Get verification statistics
    pub fn stats(&self) -> VerifyStats {
        VerifyStats {
            verified_count: self.verified.lock().unwrap().len() as u64,
            expected_count: self.expected.lock().unwrap().len() as u64,
            mismatches: self.mismatches.load(Ordering::Relaxed),
            duplicates: self.duplicates.load(Ordering::Relaxed),
            out_of_order: self.out_of_order.load(Ordering::Relaxed),
        }
    }

    /// Check if any errors occurred
    pub fn has_errors(&self) -> bool {
        self.mismatches.load(Ordering::Relaxed) > 0
    }
}

impl Default for DataVerifier {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyResult {
    Ok,
    Mismatch { seq: u64, expected: u64, actual: u64 },
    Duplicate,
    Missing,
}

#[derive(Debug, Clone)]
pub struct VerifyStats {
    pub verified_count: u64,
    pub expected_count: u64,
    pub mismatches: u64,
    pub duplicates: u64,
    pub out_of_order: u64,
}

impl VerifyStats {
    pub fn is_ok(&self) -> bool {
        self.mismatches == 0
    }

    pub fn delivery_rate(&self) -> f64 {
        if self.expected_count > 0 {
            self.verified_count as f64 / self.expected_count as f64
        } else {
            1.0
        }
    }
}

/// Simple hash for data verification
fn simple_hash(data: &[u8]) -> u64 {
    // FNV-1a hash
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Checks sequence continuity and detects gaps.
pub struct SequenceChecker {
    /// Expected next sequence
    next_expected: AtomicU64,
    /// Gaps detected: (start, end)
    gaps: Mutex<Vec<(u64, u64)>>,
    /// Total sequences seen
    total_seen: AtomicU64,
    /// Out of order count
    out_of_order: AtomicU64,
    /// Highest sequence seen
    highest_seen: AtomicU64,
}

impl SequenceChecker {
    pub fn new() -> Self {
        Self {
            next_expected: AtomicU64::new(0),
            gaps: Mutex::new(Vec::new()),
            total_seen: AtomicU64::new(0),
            out_of_order: AtomicU64::new(0),
            highest_seen: AtomicU64::new(0),
        }
    }

    pub fn with_start(start: u64) -> Self {
        let checker = Self::new();
        checker.next_expected.store(start, Ordering::Relaxed);
        checker
    }

    /// Check a sequence number
    pub fn check(&self, seq: u64) -> SequenceStatus {
        self.total_seen.fetch_add(1, Ordering::Relaxed);
        
        // Update highest seen
        let mut highest = self.highest_seen.load(Ordering::Relaxed);
        while seq > highest {
            match self.highest_seen.compare_exchange_weak(
                highest, seq, Ordering::Relaxed, Ordering::Relaxed
            ) {
                Ok(_) => break,
                Err(h) => highest = h,
            }
        }

        let expected = self.next_expected.load(Ordering::Relaxed);
        
        if seq == expected {
            // Perfect - in order
            self.next_expected.store(seq + 1, Ordering::Relaxed);
            SequenceStatus::InOrder
        } else if seq < expected {
            // Out of order (late arrival or duplicate)
            self.out_of_order.fetch_add(1, Ordering::Relaxed);
            SequenceStatus::OutOfOrder
        } else {
            // Gap detected
            let gap_start = expected;
            let gap_end = seq - 1;
            self.gaps.lock().unwrap().push((gap_start, gap_end));
            self.next_expected.store(seq + 1, Ordering::Relaxed);
            SequenceStatus::Gap { start: gap_start, end: gap_end }
        }
    }

    /// Get all detected gaps
    pub fn gaps(&self) -> Vec<(u64, u64)> {
        self.gaps.lock().unwrap().clone()
    }

    /// Get total gap count (number of missing sequences)
    pub fn total_gap_size(&self) -> u64 {
        self.gaps.lock().unwrap()
            .iter()
            .map(|(start, end)| end - start + 1)
            .sum()
    }

    /// Get statistics
    pub fn stats(&self) -> SequenceStats {
        let gaps = self.gaps.lock().unwrap();
        SequenceStats {
            total_seen: self.total_seen.load(Ordering::Relaxed),
            highest_seen: self.highest_seen.load(Ordering::Relaxed),
            gap_count: gaps.len() as u64,
            total_missing: gaps.iter().map(|(s, e)| e - s + 1).sum(),
            out_of_order: self.out_of_order.load(Ordering::Relaxed),
        }
    }
}

impl Default for SequenceChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceStatus {
    InOrder,
    OutOfOrder,
    Gap { start: u64, end: u64 },
}

#[derive(Debug, Clone)]
pub struct SequenceStats {
    pub total_seen: u64,
    pub highest_seen: u64,
    pub gap_count: u64,
    pub total_missing: u64,
    pub out_of_order: u64,
}

impl SequenceStats {
    pub fn is_perfect(&self) -> bool {
        self.gap_count == 0 && self.out_of_order == 0
    }

    pub fn delivery_rate(&self) -> f64 {
        if self.highest_seen > 0 {
            self.total_seen as f64 / (self.highest_seen + 1) as f64
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_verifier() {
        let verifier = DataVerifier::new();
        
        // Register expected
        verifier.expect(0, b"hello");
        verifier.expect(1, b"world");
        
        // Verify correct data
        assert_eq!(verifier.verify(0, b"hello"), VerifyResult::Ok);
        assert_eq!(verifier.verify(1, b"world"), VerifyResult::Ok);
        
        // Verify duplicate
        assert_eq!(verifier.verify(0, b"hello"), VerifyResult::Duplicate);
        
        let stats = verifier.stats();
        assert_eq!(stats.verified_count, 2);
        assert_eq!(stats.duplicates, 1);
    }

    #[test]
    fn test_data_verifier_mismatch() {
        let verifier = DataVerifier::new();
        verifier.expect(0, b"expected");
        
        let result = verifier.verify(0, b"different");
        assert!(matches!(result, VerifyResult::Mismatch { .. }));
        assert!(verifier.has_errors());
    }

    #[test]
    fn test_sequence_checker_in_order() {
        let checker = SequenceChecker::new();
        
        for seq in 0..100 {
            assert_eq!(checker.check(seq), SequenceStatus::InOrder);
        }
        
        let stats = checker.stats();
        assert!(stats.is_perfect());
        assert_eq!(stats.total_seen, 100);
    }

    #[test]
    fn test_sequence_checker_gap() {
        let checker = SequenceChecker::new();
        
        checker.check(0);
        checker.check(1);
        // Skip 2, 3, 4
        let status = checker.check(5);
        
        assert!(matches!(status, SequenceStatus::Gap { start: 2, end: 4 }));
        assert_eq!(checker.total_gap_size(), 3);
    }

    #[test]
    fn test_sequence_checker_out_of_order() {
        let checker = SequenceChecker::new();
        
        checker.check(0);
        checker.check(2); // Gap
        checker.check(1); // Out of order (late)
        
        let stats = checker.stats();
        assert_eq!(stats.out_of_order, 1);
    }
}

