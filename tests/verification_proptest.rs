//! Property-based formal verification tests using proptest.
//!
//! These tests verify safety invariants SI-1 through SI-10 by generating
//! thousands of random inputs and checking that properties hold universally.
//! Proptest shrinks failing cases to minimal counterexamples.
//!
//! Run with: `cargo test --test verification_proptest`
//! Run with more cases: `PROPTEST_CASES=100000 cargo test --test verification_proptest`

use proptest::prelude::*;
use std::io::Cursor;

use abt::core::device::DeviceInfo;
use abt::core::hasher::hash_reader;
use abt::core::progress::{OperationPhase, Progress};
use abt::core::safety::{DeviceFingerprint, SafetyLevel};
use abt::core::types::*;

// ════════════════════════════════════════════════════════════════════════════════
// SI-1: System Drive Protection
// ════════════════════════════════════════════════════════════════════════════════

fn arb_device_info() -> impl Strategy<Value = DeviceInfo> {
    (
        prop::sample::select(vec![
            "/dev/sda",
            "/dev/sdb",
            "/dev/sdc",
            "/dev/nvme0n1",
            r"\\.\PhysicalDrive0",
            r"\\.\PhysicalDrive1",
        ]),
        any::<bool>(), // is_system
        any::<bool>(), // read_only
        any::<bool>(), // removable
        any::<u64>(),  // size
        prop::sample::select(vec![
            DeviceType::Usb,
            DeviceType::Sd,
            DeviceType::Sata,
            DeviceType::Nvme,
            DeviceType::Mmc,
            DeviceType::Virtual,
            DeviceType::Unknown,
        ]),
    )
        .prop_map(
            |(path, is_system, read_only, removable, size, dtype)| DeviceInfo {
                path: path.to_string(),
                name: "TestDevice".to_string(),
                vendor: "TestVendor".to_string(),
                serial: Some("SN123".to_string()),
                size,
                sector_size: 512,
                physical_sector_size: 4096,
                removable,
                read_only,
                is_system,
                device_type: dtype,
                mount_points: vec![],
                transport: "test".to_string(),
            },
        )
}

proptest! {
    /// SI-1: A system drive is NEVER a safe write target.
    #[test]
    fn si1_system_drive_never_safe(dev in arb_device_info()) {
        if dev.is_system {
            prop_assert!(
                !dev.is_safe_target(),
                "SI-1 VIOLATION: System drive {} was classified as safe target",
                dev.path
            );
        }
    }

    /// SI-1: A read-only drive is NEVER a safe write target.
    #[test]
    fn si1_readonly_never_safe(dev in arb_device_info()) {
        if dev.read_only {
            prop_assert!(
                !dev.is_safe_target(),
                "SI-1 VIOLATION: Read-only drive {} was classified as safe target",
                dev.path
            );
        }
    }

    /// SI-1: If a device is a safe target, it is NOT system and NOT read-only.
    #[test]
    fn si1_safe_implies_non_system_writable(dev in arb_device_info()) {
        if dev.is_safe_target() {
            prop_assert!(!dev.is_system, "SI-1 VIOLATION: Safe target is a system drive");
            prop_assert!(!dev.read_only, "SI-1 VIOLATION: Safe target is read-only");
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════════
// SI-2: Hash Determinism and Correctness
// ════════════════════════════════════════════════════════════════════════════════

fn arb_hash_algorithm() -> impl Strategy<Value = HashAlgorithm> {
    prop::sample::select(vec![
        HashAlgorithm::Md5,
        HashAlgorithm::Sha1,
        HashAlgorithm::Sha256,
        HashAlgorithm::Sha512,
        HashAlgorithm::Blake3,
        HashAlgorithm::Crc32,
    ])
}

proptest! {
    /// SI-2: Hashing the same data twice with the same algorithm produces identical output.
    #[test]
    fn si2_hash_deterministic(
        data in prop::collection::vec(any::<u8>(), 0..4096),
        algo in arb_hash_algorithm()
    ) {
        let progress = Progress::new(data.len() as u64);

        let hash1 = hash_reader(&mut Cursor::new(&data), algo, &progress).unwrap();
        let hash2 = hash_reader(&mut Cursor::new(&data), algo, &progress).unwrap();

        prop_assert_eq!(
            &hash1, &hash2,
            "SI-2 VIOLATION: {} is non-deterministic on {} bytes",
            algo, data.len()
        );
    }

    /// SI-2: Different data produces different hashes (with overwhelming probability).
    /// This can have rare collisions, so we test with substantially different data.
    #[test]
    fn si2_hash_different_data_different_hash(
        data1 in prop::collection::vec(any::<u8>(), 32..128),
        data2 in prop::collection::vec(any::<u8>(), 32..128),
        algo in arb_hash_algorithm()
    ) {
        prop_assume!(data1 != data2);
        let progress = Progress::new(0);

        let hash1 = hash_reader(&mut Cursor::new(&data1), algo, &progress).unwrap();
        let hash2 = hash_reader(&mut Cursor::new(&data2), algo, &progress).unwrap();

        // Note: Hash collisions are theoretically possible but astronomically
        // unlikely for ≥128-bit hashes. CRC32 may collide more often.
        if algo != HashAlgorithm::Crc32 {
            prop_assert_ne!(
                &hash1, &hash2,
                "SI-2 WARNING: {} collision on {} vs {} bytes (extremely unlikely)",
                algo, data1.len(), data2.len()
            );
        }
    }

    /// SI-2: Hash output is always valid lowercase hexadecimal.
    #[test]
    fn si2_hash_output_is_hex(
        data in prop::collection::vec(any::<u8>(), 0..1024),
        algo in arb_hash_algorithm()
    ) {
        let progress = Progress::new(data.len() as u64);
        let hash = hash_reader(&mut Cursor::new(&data), algo, &progress).unwrap();

        prop_assert!(!hash.is_empty(), "SI-2: Hash output must not be empty");
        prop_assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "SI-2 VIOLATION: Hash contains non-hex chars: {}",
            hash
        );
    }

    /// SI-2: Hash output lengths are correct for each algorithm.
    #[test]
    fn si2_hash_output_length(
        data in prop::collection::vec(any::<u8>(), 0..256),
        algo in arb_hash_algorithm()
    ) {
        let progress = Progress::new(data.len() as u64);
        let hash = hash_reader(&mut Cursor::new(&data), algo, &progress).unwrap();

        let expected_len = match algo {
            HashAlgorithm::Md5 => 32,      // 128 bits = 32 hex chars
            HashAlgorithm::Sha1 => 40,     // 160 bits = 40 hex chars
            HashAlgorithm::Sha256 => 64,   // 256 bits = 64 hex chars
            HashAlgorithm::Sha512 => 128,  // 512 bits = 128 hex chars
            HashAlgorithm::Blake3 => 64,   // 256 bits = 64 hex chars
            HashAlgorithm::Crc32 => 8,     // 32 bits = 8 hex chars
        };

        prop_assert_eq!(
            hash.len(), expected_len,
            "SI-2 VIOLATION: {} hash should be {} chars, got {}",
            algo, expected_len, hash.len()
        );
    }
}

// ════════════════════════════════════════════════════════════════════════════════
// SI-3: Block Size Bounds
// ════════════════════════════════════════════════════════════════════════════════

proptest! {
    /// SI-3: heuristic_block_size returns values within [MIN, MAX] for all device sizes.
    #[test]
    fn si3_block_size_always_bounded(device_size: u64) {
        let bs = abt::core::blocksize::heuristic_block_size(device_size);
        prop_assert!(
            bs >= abt::core::blocksize::MIN_BLOCK_SIZE,
            "SI-3 VIOLATION: heuristic({}) = {} < MIN ({})",
            device_size, bs, abt::core::blocksize::MIN_BLOCK_SIZE
        );
        prop_assert!(
            bs <= abt::core::blocksize::MAX_BLOCK_SIZE,
            "SI-3 VIOLATION: heuristic({}) = {} > MAX ({})",
            device_size, bs, abt::core::blocksize::MAX_BLOCK_SIZE
        );
    }

    /// SI-3: Block size is always a power of two.
    #[test]
    fn si3_block_size_power_of_two(device_size: u64) {
        let bs = abt::core::blocksize::heuristic_block_size(device_size);
        prop_assert!(
            bs.is_power_of_two(),
            "SI-3 VIOLATION: heuristic({}) = {} is not a power of two",
            device_size, bs
        );
    }

    /// SI-3: Block size is monotonically non-decreasing with device size.
    #[test]
    fn si3_block_size_monotonic(
        size_a in 0u64..=u64::MAX/2,
        delta in 0u64..=u64::MAX/2
    ) {
        let size_b = size_a.saturating_add(delta);
        let bs_a = abt::core::blocksize::heuristic_block_size(size_a);
        let bs_b = abt::core::blocksize::heuristic_block_size(size_b);
        prop_assert!(
            bs_b >= bs_a,
            "SI-3 VIOLATION: heuristic not monotonic: h({}) = {} > h({}) = {}",
            size_a, bs_a, size_b, bs_b
        );
    }
}

// ════════════════════════════════════════════════════════════════════════════════
// SI-4: Progress Tracking
// ════════════════════════════════════════════════════════════════════════════════

proptest! {
    /// SI-4: Progress snapshot is consistent with add_bytes calls.
    #[test]
    fn si4_progress_accumulation(
        total in 0u64..=u64::MAX/2,
        adds in prop::collection::vec(0u64..=1_000_000, 0..20)
    ) {
        let p = Progress::new(total);
        let mut expected: u64 = 0;

        for &add in &adds {
            p.add_bytes(add);
            expected = expected.wrapping_add(add);
        }

        let snap = p.snapshot();
        prop_assert_eq!(
            snap.bytes_written, expected,
            "SI-4 VIOLATION: bytes_written mismatch after {} additions",
            adds.len()
        );
        prop_assert_eq!(
            snap.bytes_total, total,
            "SI-4 VIOLATION: bytes_total changed"
        );
    }

    /// SI-4: Percent is in [0, 100] when total > 0 and written <= total.
    #[test]
    fn si4_percent_bounded(
        total in 1u64..=u64::MAX/2,
        written_frac in 0.0f64..=1.0
    ) {
        let p = Progress::new(total);
        let written = (total as f64 * written_frac) as u64;
        p.add_bytes(written);

        let snap = p.snapshot();
        prop_assert!(
            snap.percent >= 0.0,
            "SI-4 VIOLATION: percent {} < 0",
            snap.percent
        );
        // Allow slight floating-point overshoot
        prop_assert!(
            snap.percent <= 100.01,
            "SI-4 VIOLATION: percent {} > 100",
            snap.percent
        );
    }
}

// ════════════════════════════════════════════════════════════════════════════════
// SI-5: Cancellation Safety
// ════════════════════════════════════════════════════════════════════════════════

proptest! {
    /// SI-5: Cancel is monotonic — once set, never unset.
    #[test]
    fn si5_cancel_monotonic(
        ops in prop::collection::vec(prop::bool::ANY, 1..50)
    ) {
        let p = Progress::new(1000);
        let mut has_been_cancelled = false;

        for &should_cancel in &ops {
            if should_cancel {
                p.cancel();
                has_been_cancelled = true;
            }
            p.add_bytes(1);

            if has_been_cancelled {
                prop_assert!(
                    p.is_cancelled(),
                    "SI-5 VIOLATION: Cancel flag was unset after being set"
                );
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════════
// SI-6: Device Fingerprint Integrity
// ════════════════════════════════════════════════════════════════════════════════

proptest! {
    /// SI-6: Fingerprint is deterministic — same device always produces same token.
    #[test]
    fn si6_fingerprint_deterministic(
        size in any::<u64>(),
        removable in any::<bool>(),
        is_system in any::<bool>(),
    ) {
        let dev = DeviceInfo {
            path: "/dev/sdb".into(),
            name: "Test".into(),
            vendor: "V".into(),
            serial: Some("SN".into()),
            size,
            sector_size: 512,
            physical_sector_size: 4096,
            removable,
            read_only: false,
            is_system,
            device_type: DeviceType::Usb,
            mount_points: vec![],
            transport: "usb".into(),
        };

        let fp1 = DeviceFingerprint::from_device(&dev);
        let fp2 = DeviceFingerprint::from_device(&dev);

        prop_assert_eq!(
            &fp1.token, &fp2.token,
            "SI-6 VIOLATION: Fingerprint is non-deterministic"
        );
    }

    /// SI-6: Token is always 32 hex characters (128-bit BLAKE3 truncation).
    #[test]
    fn si6_token_format(
        size in any::<u64>(),
        removable in any::<bool>(),
    ) {
        let dev = DeviceInfo {
            path: "/dev/sdb".into(),
            name: "Test".into(),
            vendor: "V".into(),
            serial: None,
            size,
            sector_size: 512,
            physical_sector_size: 4096,
            removable,
            read_only: false,
            is_system: false,
            device_type: DeviceType::Usb,
            mount_points: vec![],
            transport: "usb".into(),
        };

        let fp = DeviceFingerprint::from_device(&dev);

        prop_assert_eq!(
            fp.token.len(), 32,
            "SI-6 VIOLATION: Token must be 32 hex chars, got {}",
            fp.token.len()
        );
        prop_assert!(
            fp.token.chars().all(|c| c.is_ascii_hexdigit()),
            "SI-6 VIOLATION: Token contains non-hex chars: {}",
            fp.token
        );
    }
}

// ════════════════════════════════════════════════════════════════════════════════
// SI-8: Zero-Detection Soundness
// ════════════════════════════════════════════════════════════════════════════════

/// Naive byte-by-byte zero check (reference implementation).
fn naive_is_zero(buf: &[u8]) -> bool {
    buf.iter().all(|&b| b == 0)
}

/// Optimized zero check (mirrors writer.rs and clone.rs implementations).
fn fast_is_zero(buf: &[u8]) -> bool {
    let (prefix, aligned, suffix) = unsafe { buf.align_to::<u64>() };
    prefix.iter().all(|&b| b == 0)
        && aligned.iter().all(|&w| w == 0)
        && suffix.iter().all(|&b| b == 0)
}

proptest! {
    /// SI-8: Optimized zero-check is equivalent to naive byte-by-byte check.
    #[test]
    fn si8_zero_check_equiv_naive(
        data in prop::collection::vec(any::<u8>(), 0..512)
    ) {
        let naive = naive_is_zero(&data);
        let fast = fast_is_zero(&data);

        prop_assert_eq!(
            naive, fast,
            "SI-8 VIOLATION: naive={} fast={} for {} bytes (first non-zero at {:?})",
            naive, fast, data.len(),
            data.iter().position(|&b| b != 0)
        );
    }

    /// SI-8: All-zero buffers always detected correctly.
    #[test]
    fn si8_all_zero_detected(len in 0usize..4096) {
        let buf = vec![0u8; len];
        prop_assert!(
            fast_is_zero(&buf),
            "SI-8 VIOLATION: All-zero buffer of len {} not detected",
            len
        );
    }

    /// SI-8: Any single non-zero byte is detected.
    #[test]
    fn si8_single_nonzero_detected(
        len in 1usize..512,
        pos in 0usize..512,
        val in 1u8..=255u8
    ) {
        prop_assume!(pos < len);
        let mut buf = vec![0u8; len];
        buf[pos] = val;

        prop_assert!(
            !fast_is_zero(&buf),
            "SI-8 VIOLATION: Non-zero byte at pos {} (val {}) not detected in buffer of len {}",
            pos, val, len
        );
    }
}

// ════════════════════════════════════════════════════════════════════════════════
// SI-9: OperationPhase Encoding
// ════════════════════════════════════════════════════════════════════════════════

proptest! {
    /// SI-9: from_u8 round-trips for valid discriminants.
    #[test]
    fn si9_phase_roundtrip(disc in 0u8..=9) {
        let phase = OperationPhase::from_u8(disc);
        prop_assert_eq!(
            phase as u8, disc,
            "SI-9 VIOLATION: from_u8({}) as u8 == {}",
            disc, phase as u8
        );
    }

    /// SI-9: Out-of-range discriminants default safely.
    #[test]
    fn si9_phase_oob_safe(disc in 10u8..=255) {
        let phase = OperationPhase::from_u8(disc);
        prop_assert_eq!(
            phase as u8, 0,
            "SI-9 VIOLATION: OOB disc {} should default to 0 (Preparing), got {}",
            disc, phase as u8
        );
    }
}

// ════════════════════════════════════════════════════════════════════════════════
// SI-10: Type Invariants
// ════════════════════════════════════════════════════════════════════════════════

proptest! {
    /// SI-10: ImageFormat::is_compressed is correct.
    #[test]
    fn si10_compressed_formats(
        format in prop::sample::select(vec![
            ImageFormat::Raw,
            ImageFormat::Iso,
            ImageFormat::Img,
            ImageFormat::Dmg,
            ImageFormat::Vhd,
            ImageFormat::Vhdx,
            ImageFormat::Vmdk,
            ImageFormat::Qcow2,
            ImageFormat::Wim,
            ImageFormat::Ffu,
            ImageFormat::Gz,
            ImageFormat::Bz2,
            ImageFormat::Xz,
            ImageFormat::Zstd,
            ImageFormat::Zip,
        ])
    ) {
        let is_compressed = format.is_compressed();
        let should_be = matches!(
            format,
            ImageFormat::Gz | ImageFormat::Bz2 | ImageFormat::Xz | ImageFormat::Zstd | ImageFormat::Zip
        );
        prop_assert_eq!(
            is_compressed, should_be,
            "SI-10 VIOLATION: {}.is_compressed() = {} but expected {}",
            format, is_compressed, should_be
        );
    }

    /// SI-10: SafetyLevel parses all known string aliases.
    #[test]
    fn si10_safety_level_parses(
        input in prop::sample::select(vec![
            "low", "normal",
            "medium", "cautious", "agent",
            "high", "paranoid", "max",
        ])
    ) {
        let result = input.parse::<SafetyLevel>();
        prop_assert!(
            result.is_ok(),
            "SI-10 VIOLATION: SafetyLevel failed to parse '{}'",
            input
        );
    }

    /// SI-10: WriteConfig round-trips through JSON.
    #[test]
    fn si10_write_config_serialization(
        block_size in prop::sample::select(vec![4096usize, 65536, 1048576, 4194304]),
        verify in any::<bool>(),
        sparse in any::<bool>(),
        direct_io in any::<bool>(),
    ) {
        let cfg = WriteConfig {
            block_size,
            verify,
            sparse,
            direct_io,
            ..WriteConfig::default()
        };

        let json = serde_json::to_string(&cfg).unwrap();
        let cfg2: WriteConfig = serde_json::from_str(&json).unwrap();

        prop_assert_eq!(cfg.block_size, cfg2.block_size);
        prop_assert_eq!(cfg.verify, cfg2.verify);
        prop_assert_eq!(cfg.sparse, cfg2.sparse);
        prop_assert_eq!(cfg.direct_io, cfg2.direct_io);
    }
}

// ════════════════════════════════════════════════════════════════════════════════
// SI-2: Cross-algorithm hash consistency
// ════════════════════════════════════════════════════════════════════════════════

proptest! {
    /// SI-2: hash_reader and hash_file produce identical results for the same data.
    #[test]
    fn si2_hash_reader_vs_file_consistent(
        data in prop::collection::vec(any::<u8>(), 1..2048),
        algo in arb_hash_algorithm()
    ) {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut f = NamedTempFile::new().unwrap();
        f.write_all(&data).unwrap();
        f.flush().unwrap();

        let progress = Progress::new(data.len() as u64);

        let file_hash = abt::core::hasher::hash_file(f.path(), algo, &progress).unwrap();
        let reader_hash = hash_reader(&mut Cursor::new(&data), algo, &progress).unwrap();

        prop_assert_eq!(
            &file_hash, &reader_hash,
            "SI-2 VIOLATION: hash_file != hash_reader for {} on {} bytes",
            algo, data.len()
        );
    }
}
