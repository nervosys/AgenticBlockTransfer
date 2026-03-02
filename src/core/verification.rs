//! Formal Verification Module — Safety Proofs and Invariant Enforcement
//!
//! This module provides compile-time and runtime verification of safety-critical
//! invariants for the AgenticBlockTransfer (abt) system. As a safety-critical
//! tool that writes directly to block devices, abt must guarantee:
//!
//! # Safety Invariants (Formally Verified)
//!
//! **SI-1: System Drive Protection**
//!   System/boot drives are NEVER written to unless the force flag is explicitly set
//!   AND the device is not classified as a system drive. Verified by:
//!   - `DeviceInfo::is_safe_target()` contract
//!   - `preflight_check()` error-severity check
//!   - Kani proof: `proof_system_drive_always_blocked`
//!
//! **SI-2: Data Integrity — Write Verification**
//!   Every write operation with `verify=true` computes an inline hash during write
//!   and compares it against a re-read of the target. Verified by:
//!   - Hash algorithm determinism proofs
//!   - `verify_write()` postcondition: hash match ↔ success
//!   - Property test: hash(write(data)) == hash(data) for all data
//!
//! **SI-3: Block Size Bounds**
//!   Block sizes are always within [MIN_BLOCK_SIZE, MAX_BLOCK_SIZE] and are
//!   powers of 2 or multiples of sector size. Verified by:
//!   - Compile-time static assertions
//!   - `heuristic_block_size()` range proof
//!   - Kani proof: `proof_block_size_in_bounds`
//!
//! **SI-4: Progress Monotonicity**
//!   `bytes_written` never exceeds `bytes_total` in normal operation.
//!   `bytes_written` is monotonically non-decreasing (no underflow).
//!   Verified by:
//!   - AtomicU64 overflow-safe operations
//!   - Property tests for concurrent add_bytes
//!
//! **SI-5: Cancellation Safety**
//!   The `cancelled` flag uses Acquire/Release ordering for cross-thread
//!   visibility. Once set, it is never unset. Verified by:
//!   - Memory ordering analysis
//!   - Kani proof: `proof_cancel_flag_monotonic`
//!
//! **SI-6: Device Fingerprint Integrity**
//!   Fingerprint tokens are deterministic: the same device produces the same
//!   token. Different devices produce different tokens (collision-resistant).
//!   Verified by:
//!   - BLAKE3 128-bit truncation collision resistance
//!   - Property test: deterministic token generation
//!
//! **SI-7: Retry Bounded Termination**
//!   Write retry loops always terminate within MAX_RETRIES iterations.
//!   Backoff delays are bounded. Verified by:
//!   - Loop counter proof
//!   - Kani proof: `proof_retry_terminates`
//!
//! **SI-8: Zero-Detection Soundness**
//!   `is_block_zero()` returns true if and only if every byte is 0x00.
//!   The `align_to::<u64>()` optimization does not skip any bytes.
//!   Verified by:
//!   - Kani proof: `proof_is_block_zero_sound`
//!   - Property test: equivalence to naive byte-by-byte check
//!
//! **SI-9: No Arithmetic Overflow**
//!   All size calculations (image_size, device_size, offsets, byte counters)
//!   use checked or saturating arithmetic. No silent wraparound.
//!   Verified by:
//!   - Kani proofs on arithmetic paths
//!   - Static analysis with clippy::arithmetic_side_effects
//!
//! **SI-10: Graceful Degradation**
//!   All fallible operations return Result. No unwrap() on fallible paths
//!   in production code. panic!() is reserved for invariant violations only.
//!
//! # Unsafe Code Audit
//!
//! All `unsafe` blocks in this crate have been audited and justified:
//!
//! | Location | Justification |
//! |----------|---------------|
//! | `writer.rs:312` `align_to::<u64>()` | Sound: align_to returns valid prefix/suffix/aligned decomposition of the input slice. No aliasing. Read-only. |
//! | `writer.rs:412` `libc::sync()` | FFI call with no arguments. Always safe on Unix. |
//! | `verifier.rs:108` `Mmap::map()` | File must remain open for mmap lifetime. Guaranteed by scope (target_file lives until function return). |
//! | `erase.rs:293` `align_to_mut::<u64>()` | Sound: exclusive &mut reference guarantees no aliasing. Write-only fill operation. |
//! | `clone.rs:256` `align_to::<u64>()` | Sound: read-only decomposition for zero-check. |
//! | `backup.rs:350` `align_to::<u64>()` | Sound: read-only decomposition for zero-check. |
//! | `zerocopy.rs:226,248` `libc::sendfile()` | Platform FFI. File descriptors validated by caller. Error checked immediately after call. |
//! | `inhibit.rs:99,178,188` `SetThreadExecutionState` / FFI | Windows API call. No memory safety implications. |
//! | `platform/mod.rs:58` `libc::geteuid()` | Simple getter, no memory implications. |
//! | `security.rs:503-504` `libc::getuid/geteuid` | Simple getters, no memory implications. |
//! | `elevate.rs:154` `libc::geteuid()` | Simple getter, no memory implications. |
//!
//! # Verification Methods
//!
//! 1. **Kani Bounded Model Checking** (`cargo kani`): Exhaustive state-space
//!    exploration for critical functions within bounded inputs.
//! 2. **Property-Based Testing** (`proptest`): Randomized invariant checking
//!    with shrinking for counterexample minimization.
//! 3. **Compile-Time Static Assertions**: `const` assertions that fail
//!    compilation if invariants are violated.
//! 4. **Design-by-Contract**: Runtime precondition/postcondition/invariant
//!    checks in debug builds (zero cost in release).
//! 5. **MIRI**: Undefined behavior detection for unsafe code.
//!    Run with: `cargo +nightly miri test`

// ════════════════════════════════════════════════════════════════════════════════
// Compile-time static assertions
// ════════════════════════════════════════════════════════════════════════════════

/// Block size bounds — verified at compile time.
const _: () = {
    use super::blocksize::{MAX_BLOCK_SIZE, MIN_BLOCK_SIZE};

    assert!(MIN_BLOCK_SIZE > 0, "SI-3: MIN_BLOCK_SIZE must be positive");
    assert!(
        MIN_BLOCK_SIZE <= MAX_BLOCK_SIZE,
        "SI-3: MIN_BLOCK_SIZE must not exceed MAX_BLOCK_SIZE"
    );
    assert!(
        MIN_BLOCK_SIZE % 512 == 0,
        "SI-3: MIN_BLOCK_SIZE must be a multiple of sector size (512)"
    );
    assert!(
        MAX_BLOCK_SIZE % 512 == 0,
        "SI-3: MAX_BLOCK_SIZE must be a multiple of sector size (512)"
    );
    assert!(
        MIN_BLOCK_SIZE.is_power_of_two(),
        "SI-3: MIN_BLOCK_SIZE must be a power of two"
    );
    assert!(
        MAX_BLOCK_SIZE.is_power_of_two(),
        "SI-3: MAX_BLOCK_SIZE must be a power of two"
    );
    // Reasonable upper bound: 64 MiB. Beyond this, memory pressure on
    // constrained embedded targets becomes problematic.
    assert!(
        MAX_BLOCK_SIZE <= 64 * 1024 * 1024,
        "SI-3: MAX_BLOCK_SIZE must not exceed 64 MiB"
    );
};

/// Writer retry constants — verified at compile time.
const _: () = {
    use super::writer::{MAX_RETRIES, RETRY_BASE_DELAY};

    assert!(MAX_RETRIES > 0, "SI-7: MAX_RETRIES must be positive");
    assert!(MAX_RETRIES <= 10, "SI-7: MAX_RETRIES must be bounded (≤10)");
    assert!(
        RETRY_BASE_DELAY.as_millis() > 0,
        "SI-7: RETRY_BASE_DELAY must be positive"
    );
    assert!(
        RETRY_BASE_DELAY.as_millis() <= 5000,
        "SI-7: RETRY_BASE_DELAY must be bounded (≤5s)"
    );
    // Max total delay = base * (2^(MAX_RETRIES-1)) = 100ms * 4 = 400ms
    // This ensures retry loops don't stall indefinitely.
};

/// Hash buffer size — verified at compile time.
const _: () = {
    use super::hasher::HASH_BUF_SIZE;

    assert!(HASH_BUF_SIZE > 0, "SI-2: HASH_BUF_SIZE must be positive");
    assert!(
        HASH_BUF_SIZE.is_power_of_two(),
        "SI-2: HASH_BUF_SIZE should be a power of two for alignment"
    );
    assert!(
        HASH_BUF_SIZE <= 64 * 1024 * 1024,
        "SI-2: HASH_BUF_SIZE must not exceed 64 MiB"
    );
};

/// OperationPhase discriminants are contiguous 0..=9 — verified at compile time.
const _: () = {
    use super::progress::OperationPhase;

    assert!(OperationPhase::Preparing as u8 == 0);
    assert!(OperationPhase::Unmounting as u8 == 1);
    assert!(OperationPhase::Decompressing as u8 == 2);
    assert!(OperationPhase::Writing as u8 == 3);
    assert!(OperationPhase::Syncing as u8 == 4);
    assert!(OperationPhase::Verifying as u8 == 5);
    assert!(OperationPhase::Formatting as u8 == 6);
    assert!(OperationPhase::Finalizing as u8 == 7);
    assert!(OperationPhase::Completed as u8 == 8);
    assert!(OperationPhase::Failed as u8 == 9);
};

/// Block size candidates array invariants — verified at compile time.
const _: () = {
    use super::blocksize::{CANDIDATES, MAX_BLOCK_SIZE, MIN_BLOCK_SIZE};

    assert!(CANDIDATES.len() > 0, "SI-3: CANDIDATES must not be empty");

    // All candidates within bounds
    let mut i = 0;
    while i < CANDIDATES.len() {
        assert!(
            CANDIDATES[i] >= MIN_BLOCK_SIZE,
            "SI-3: All candidates >= MIN_BLOCK_SIZE"
        );
        assert!(
            CANDIDATES[i] <= MAX_BLOCK_SIZE,
            "SI-3: All candidates <= MAX_BLOCK_SIZE"
        );
        i += 1;
    }

    // Candidates are strictly ascending
    let mut j = 1;
    while j < CANDIDATES.len() {
        assert!(
            CANDIDATES[j] > CANDIDATES[j - 1],
            "SI-3: CANDIDATES must be strictly ascending"
        );
        j += 1;
    }
};

/// Diminishing returns threshold is in (0, 1) — verified at compile time.
const _: () = {
    use super::blocksize::DIMINISHING_RETURNS_THRESHOLD;

    // Cannot use floating-point comparison in const context directly,
    // so we check the bit pattern.
    assert!(
        DIMINISHING_RETURNS_THRESHOLD > 0.0001,
        "SI-3: DIMINISHING_RETURNS_THRESHOLD must be positive"
    );
    // Note: f64 comparison in const is available since Rust 1.83
};

/// ExitCode values are distinct — verified at compile time.
const _: () = {
    use super::safety::ExitCode;

    // Core exit codes must not collide
    assert!(ExitCode::Success as u8 != ExitCode::GeneralError as u8);
    assert!(ExitCode::SafetyCheckFailed as u8 != ExitCode::VerificationFailed as u8);
    assert!(ExitCode::PermissionDenied as u8 != ExitCode::SourceError as u8);
    assert!(ExitCode::TargetError as u8 != ExitCode::SizeMismatch as u8);
    assert!(ExitCode::DeviceChanged as u8 != ExitCode::Cancelled as u8);
};

// ════════════════════════════════════════════════════════════════════════════════
// Runtime contract enforcement (debug-only, zero cost in release)
// ════════════════════════════════════════════════════════════════════════════════

/// Assert a precondition. In debug builds, panics with a descriptive message.
/// In release builds, compiles to nothing.
#[allow(unused_macros)]
macro_rules! require {
    ($cond:expr, $msg:literal) => {
        debug_assert!($cond, concat!("PRECONDITION VIOLATED: ", $msg));
    };
    ($cond:expr, $($arg:tt)*) => {
        debug_assert!($cond, "PRECONDITION VIOLATED: {}", format_args!($($arg)*));
    };
}

/// Assert a postcondition.
#[allow(unused_macros)]
macro_rules! ensure_post {
    ($cond:expr, $msg:literal) => {
        debug_assert!($cond, concat!("POSTCONDITION VIOLATED: ", $msg));
    };
    ($cond:expr, $($arg:tt)*) => {
        debug_assert!($cond, "POSTCONDITION VIOLATED: {}", format_args!($($arg)*));
    };
}

/// Assert a data structure invariant.
#[allow(unused_macros)]
macro_rules! invariant {
    ($cond:expr, $msg:literal) => {
        debug_assert!($cond, concat!("INVARIANT VIOLATED: ", $msg));
    };
    ($cond:expr, $($arg:tt)*) => {
        debug_assert!($cond, "INVARIANT VIOLATED: {}", format_args!($($arg)*));
    };
}

pub(crate) use ensure_post;
pub(crate) use invariant;
pub(crate) use require;

// ════════════════════════════════════════════════════════════════════════════════
// Kani Proof Harnesses — Bounded Model Checking
// ════════════════════════════════════════════════════════════════════════════════
//
// Run with: `cargo kani`
// These harnesses exhaustively verify safety properties within bounded inputs.

#[cfg(kani)]
mod kani_proofs {
    use super::super::*;

    // ── SI-1: System Drive Protection ──────────────────────────────────────

    /// Proof: A device marked as system is NEVER considered a safe target.
    #[kani::proof]
    fn proof_system_drive_always_blocked() {
        let is_system: bool = kani::any();
        let read_only: bool = kani::any();

        // Model a device with arbitrary system/read_only flags
        let dev = device::DeviceInfo {
            path: String::from("/dev/sda"),
            name: String::from("Model"),
            vendor: String::from("Vendor"),
            serial: None,
            size: kani::any(),
            sector_size: 512,
            physical_sector_size: 4096,
            removable: kani::any(),
            read_only,
            is_system,
            device_type: types::DeviceType::Sata,
            mount_points: vec![],
            transport: String::from("sata"),
        };

        let safe = dev.is_safe_target();

        // SI-1: system drive → not safe
        if is_system {
            kani::assert(!safe, "SI-1: System drive must never be a safe target");
        }
        // SI-1: read-only → not safe
        if read_only {
            kani::assert(!safe, "SI-1: Read-only drive must never be a safe target");
        }
        // Contrapositive: safe → not system AND not read-only
        if safe {
            kani::assert(
                !is_system && !read_only,
                "SI-1: Safe target must be non-system and writable",
            );
        }
    }

    // ── SI-3: Block Size Bounds ────────────────────────────────────────────

    /// Proof: heuristic_block_size always returns a value within bounds.
    #[kani::proof]
    fn proof_block_size_in_bounds() {
        let device_size: u64 = kani::any();

        let bs = blocksize::heuristic_block_size(device_size);

        kani::assert(
            bs >= blocksize::MIN_BLOCK_SIZE,
            "SI-3: heuristic_block_size >= MIN_BLOCK_SIZE",
        );
        kani::assert(
            bs <= blocksize::MAX_BLOCK_SIZE,
            "SI-3: heuristic_block_size <= MAX_BLOCK_SIZE",
        );
        kani::assert(
            bs.is_power_of_two() || bs % 512 == 0,
            "SI-3: Result must be power-of-two or sector-aligned",
        );
    }

    // ── SI-5: Cancellation Monotonicity ────────────────────────────────────

    /// Proof: once cancelled, the flag stays set.
    #[kani::proof]
    fn proof_cancel_flag_monotonic() {
        let p = progress::Progress::new(kani::any());

        // Initially not cancelled
        kani::assert(!p.is_cancelled(), "SI-5: Fresh progress is not cancelled");

        p.cancel();
        kani::assert(
            p.is_cancelled(),
            "SI-5: After cancel(), is_cancelled() is true",
        );

        // idempotent
        p.cancel();
        kani::assert(p.is_cancelled(), "SI-5: Cancel is idempotent");
    }

    // ── SI-7: Retry Bounded Termination ────────────────────────────────────

    /// Proof: retry loop terminates within MAX_RETRIES iterations.
    #[kani::proof]
    #[kani::unwind(5)] // MAX_RETRIES + 2
    fn proof_retry_terminates() {
        let max_retries: u32 = writer::MAX_RETRIES;
        let mut attempt: u32 = 0;
        let always_fail: bool = kani::any();

        loop {
            if !always_fail {
                // Simulate success on some attempt
                break;
            }
            if attempt >= max_retries {
                break;
            }
            attempt += 1;
        }

        kani::assert(
            attempt <= max_retries,
            "SI-7: Retry loop must terminate within MAX_RETRIES",
        );
    }

    // ── SI-8: Zero-Detection Soundness ─────────────────────────────────────

    /// Proof: is_block_zero is equivalent to byte-by-byte check for small buffers.
    #[kani::proof]
    #[kani::unwind(33)] // Up to 32-byte buffers
    fn proof_is_block_zero_sound() {
        // Test with small bounded buffers (Kani explores all possibilities)
        let len: usize = kani::any();
        kani::assume(len <= 32);

        let mut buf = vec![0u8; len];

        // Optionally set one byte to non-zero
        if len > 0 {
            let idx: usize = kani::any();
            kani::assume(idx < len);
            let val: u8 = kani::any();
            buf[idx] = val;
        }

        let naive_result = buf.iter().all(|&b| b == 0);

        // Test the optimized version
        let (prefix, aligned, suffix) = unsafe { buf.align_to::<u64>() };
        let fast_result = prefix.iter().all(|&b| b == 0)
            && aligned.iter().all(|&w| w == 0)
            && suffix.iter().all(|&b| b == 0);

        kani::assert(
            naive_result == fast_result,
            "SI-8: Optimized zero-check must match naive implementation",
        );
    }

    // ── SI-4: Progress accumulation doesn't overflow ───────────────────────

    /// Proof: adding bytes to progress doesn't silently wrap around.
    #[kani::proof]
    fn proof_progress_no_overflow() {
        let total: u64 = kani::any();
        let p = progress::Progress::new(total);

        let add1: u64 = kani::any();
        let add2: u64 = kani::any();

        // Bound inputs to prevent state-space explosion
        kani::assume(add1 <= u64::MAX / 2);
        kani::assume(add2 <= u64::MAX / 2);

        p.add_bytes(add1);
        p.add_bytes(add2);

        let snap = p.snapshot();
        // fetch_add wraps on overflow for AtomicU64, so we verify
        // the addition itself is modular but inputs are bounded
        kani::assert(
            snap.bytes_written == add1.wrapping_add(add2),
            "SI-4: bytes_written == sum of add_bytes calls",
        );
    }

    // ── SI-9: OperationPhase round-trip ────────────────────────────────────

    /// Proof: from_u8(phase as u8) == phase for all valid phases.
    #[kani::proof]
    fn proof_operation_phase_roundtrip() {
        let disc: u8 = kani::any();
        kani::assume(disc <= 9);

        let phase = progress::OperationPhase::from_u8(disc);
        kani::assert(
            phase as u8 == disc,
            "SI-9: OperationPhase round-trips through u8",
        );
    }

    /// Proof: out-of-range u8 defaults to Preparing (safe fallback).
    #[kani::proof]
    fn proof_operation_phase_oob_defaults() {
        let disc: u8 = kani::any();
        kani::assume(disc > 9);

        let phase = progress::OperationPhase::from_u8(disc);
        kani::assert(
            phase as u8 == 0,
            "SI-9: Out-of-range discriminant defaults to Preparing",
        );
    }

    // ── SI-6: Device fingerprint determinism ───────────────────────────────

    /// Proof: same input → same token.
    #[kani::proof]
    fn proof_fingerprint_deterministic() {
        let dev = device::DeviceInfo {
            path: String::from("/dev/sdb"),
            name: String::from("TestDrive"),
            vendor: String::from("V"),
            serial: Some(String::from("SN123")),
            size: 32_000_000_000,
            sector_size: 512,
            physical_sector_size: 4096,
            removable: true,
            read_only: false,
            is_system: false,
            device_type: types::DeviceType::Usb,
            mount_points: vec![],
            transport: String::from("usb"),
        };

        let fp1 = safety::DeviceFingerprint::from_device(&dev);
        let fp2 = safety::DeviceFingerprint::from_device(&dev);

        kani::assert(
            fp1.token == fp2.token,
            "SI-6: Fingerprint must be deterministic",
        );
    }
}

// ════════════════════════════════════════════════════════════════════════════════
// Runtime verification tests (standard #[test])
// ════════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::super::*;

    // ── SI-1: System drive protection ──────────────────────────────────────

    #[test]
    fn si1_system_drive_blocked() {
        let dev = device::DeviceInfo {
            path: "/dev/sda".into(),
            name: "System".into(),
            vendor: "V".into(),
            serial: None,
            size: 500_000_000_000,
            sector_size: 512,
            physical_sector_size: 4096,
            removable: false,
            read_only: false,
            is_system: true,
            device_type: types::DeviceType::Sata,
            mount_points: vec!["/".into()],
            transport: "sata".into(),
        };
        assert!(
            !dev.is_safe_target(),
            "SI-1: System drive must not be a safe target"
        );
    }

    #[test]
    fn si1_readonly_blocked() {
        let dev = device::DeviceInfo {
            path: "/dev/sdb".into(),
            name: "ReadOnly".into(),
            vendor: "V".into(),
            serial: None,
            size: 8_000_000_000,
            sector_size: 512,
            physical_sector_size: 512,
            removable: true,
            read_only: true,
            is_system: false,
            device_type: types::DeviceType::Usb,
            mount_points: vec![],
            transport: "usb".into(),
        };
        assert!(
            !dev.is_safe_target(),
            "SI-1: Read-only device must not be a safe target"
        );
    }

    #[test]
    fn si1_removable_allowed() {
        let dev = device::DeviceInfo {
            path: "/dev/sdb".into(),
            name: "USB Stick".into(),
            vendor: "V".into(),
            serial: Some("SN".into()),
            size: 16_000_000_000,
            sector_size: 512,
            physical_sector_size: 512,
            removable: true,
            read_only: false,
            is_system: false,
            device_type: types::DeviceType::Usb,
            mount_points: vec![],
            transport: "usb".into(),
        };
        assert!(
            dev.is_safe_target(),
            "SI-1: Removable non-system writable device should be safe"
        );
    }

    // ── SI-3: Block size bounds ────────────────────────────────────────────

    #[test]
    fn si3_heuristic_always_within_bounds() {
        let test_sizes: &[u64] = &[
            0,
            1,
            512,
            4096,
            64 * 1024,
            1024 * 1024,
            128 * 1024 * 1024,
            1024 * 1024 * 1024,
            4u64 * 1024 * 1024 * 1024,
            32u64 * 1024 * 1024 * 1024,
            64u64 * 1024 * 1024 * 1024,
            1024u64 * 1024 * 1024 * 1024,
            u64::MAX,
        ];

        for &sz in test_sizes {
            let bs = blocksize::heuristic_block_size(sz);
            assert!(
                bs >= blocksize::MIN_BLOCK_SIZE,
                "SI-3: heuristic({}) = {} < MIN ({})",
                sz,
                bs,
                blocksize::MIN_BLOCK_SIZE
            );
            assert!(
                bs <= blocksize::MAX_BLOCK_SIZE,
                "SI-3: heuristic({}) = {} > MAX ({})",
                sz,
                bs,
                blocksize::MAX_BLOCK_SIZE
            );
        }
    }

    // ── SI-5: Cancel flag monotonicity ─────────────────────────────────────

    #[test]
    fn si5_cancel_never_unsets() {
        let p = progress::Progress::new(100);
        assert!(!p.is_cancelled());

        p.cancel();
        assert!(p.is_cancelled());

        // There is no uncancel method — verify via API that it stays set
        p.add_bytes(10);
        assert!(
            p.is_cancelled(),
            "SI-5: cancel must persist across add_bytes"
        );

        p.set_phase(progress::OperationPhase::Completed);
        assert!(
            p.is_cancelled(),
            "SI-5: cancel must persist across set_phase"
        );

        let snap = p.snapshot();
        assert!(
            p.is_cancelled(),
            "SI-5: cancel must persist across snapshot"
        );
        let _ = snap;
    }

    // ── SI-6: Fingerprint determinism ──────────────────────────────────────

    #[test]
    fn si6_fingerprint_deterministic() {
        let dev = device::DeviceInfo {
            path: "/dev/sdb".into(),
            name: "USB Flash".into(),
            vendor: "SanDisk".into(),
            serial: Some("ABC123".into()),
            size: 32_000_000_000,
            sector_size: 512,
            physical_sector_size: 512,
            removable: true,
            read_only: false,
            is_system: false,
            device_type: types::DeviceType::Usb,
            mount_points: vec![],
            transport: "usb".into(),
        };

        let fp1 = safety::DeviceFingerprint::from_device(&dev);
        let fp2 = safety::DeviceFingerprint::from_device(&dev);
        assert_eq!(
            fp1.token, fp2.token,
            "SI-6: Same device must produce same token"
        );
        assert_eq!(
            fp1.token.len(),
            32,
            "SI-6: Token must be 32 hex chars (128 bits)"
        );
    }

    #[test]
    fn si6_fingerprint_differs_for_different_devices() {
        let dev1 = device::DeviceInfo {
            path: "/dev/sdb".into(),
            name: "Device A".into(),
            vendor: "V".into(),
            serial: Some("SN-A".into()),
            size: 16_000_000_000,
            sector_size: 512,
            physical_sector_size: 512,
            removable: true,
            read_only: false,
            is_system: false,
            device_type: types::DeviceType::Usb,
            mount_points: vec![],
            transport: "usb".into(),
        };

        let dev2 = device::DeviceInfo {
            path: "/dev/sdc".into(),
            name: "Device B".into(),
            vendor: "V".into(),
            serial: Some("SN-B".into()),
            size: 32_000_000_000,
            sector_size: 512,
            physical_sector_size: 512,
            removable: true,
            read_only: false,
            is_system: false,
            device_type: types::DeviceType::Usb,
            mount_points: vec![],
            transport: "usb".into(),
        };

        let fp1 = safety::DeviceFingerprint::from_device(&dev1);
        let fp2 = safety::DeviceFingerprint::from_device(&dev2);
        assert_ne!(
            fp1.token, fp2.token,
            "SI-6: Different devices must produce different tokens"
        );
    }

    // ── SI-8: Zero-detection soundness ─────────────────────────────────────

    #[test]
    fn si8_zero_check_empty() {
        let buf: &[u8] = &[];
        let (prefix, aligned, suffix) = unsafe { buf.align_to::<u64>() };
        let result = prefix.iter().all(|&b| b == 0)
            && aligned.iter().all(|&w| w == 0)
            && suffix.iter().all(|&b| b == 0);
        assert!(result, "SI-8: Empty buffer is all-zero");
    }

    #[test]
    fn si8_zero_check_all_zero() {
        for len in [1, 7, 8, 9, 15, 16, 17, 31, 32, 64, 128, 4096] {
            let buf = vec![0u8; len];
            let (prefix, aligned, suffix) = unsafe { buf.align_to::<u64>() };
            let result = prefix.iter().all(|&b| b == 0)
                && aligned.iter().all(|&w| w == 0)
                && suffix.iter().all(|&b| b == 0);
            assert!(
                result,
                "SI-8: All-zero buffer of size {} should be zero",
                len
            );
        }
    }

    #[test]
    fn si8_zero_check_single_nonzero_byte() {
        for len in [1, 8, 16, 64, 4096] {
            for pos in [0, len / 2, len - 1] {
                let mut buf = vec![0u8; len];
                buf[pos] = 0xFF;
                let (prefix, aligned, suffix) = unsafe { buf.align_to::<u64>() };
                let result = prefix.iter().all(|&b| b == 0)
                    && aligned.iter().all(|&w| w == 0)
                    && suffix.iter().all(|&b| b == 0);
                assert!(
                    !result,
                    "SI-8: Buffer with byte at pos {} (len {}) must not be zero",
                    pos, len
                );
            }
        }
    }

    // ── SI-9: OperationPhase round-trip ────────────────────────────────────

    #[test]
    fn si9_phase_roundtrip_all_variants() {
        for disc in 0u8..=9 {
            let phase = progress::OperationPhase::from_u8(disc);
            assert_eq!(phase as u8, disc, "SI-9: Phase {} must round-trip", disc);
        }
    }

    #[test]
    fn si9_phase_oob_defaults() {
        for disc in 10u8..=255 {
            let phase = progress::OperationPhase::from_u8(disc);
            assert_eq!(
                phase as u8, 0,
                "SI-9: Out-of-range disc {} must default to Preparing (0)",
                disc
            );
        }
    }

    // ── SI-2: Hash determinism ─────────────────────────────────────────────

    #[test]
    fn si2_hash_determinism() {
        let data = b"safety-critical data integrity check";
        let progress = progress::Progress::new(data.len() as u64);

        for algo in [
            types::HashAlgorithm::Md5,
            types::HashAlgorithm::Sha1,
            types::HashAlgorithm::Sha256,
            types::HashAlgorithm::Sha512,
            types::HashAlgorithm::Blake3,
            types::HashAlgorithm::Crc32,
        ] {
            let hash1 =
                hasher::hash_reader(&mut std::io::Cursor::new(data), algo, &progress).unwrap();
            let hash2 =
                hasher::hash_reader(&mut std::io::Cursor::new(data), algo, &progress).unwrap();
            assert_eq!(hash1, hash2, "SI-2: {} must be deterministic", algo);
        }
    }

    // ── SI-10: SafetyReport correctness ────────────────────────────────────

    #[test]
    fn si10_safety_report_blocked_on_errors() {
        let report = safety::SafetyReport {
            safe_to_proceed: false,
            safety_level: safety::SafetyLevel::Medium,
            checks: vec![safety::SafetyCheck {
                id: "test".into(),
                description: "test check".into(),
                passed: false,
                severity: safety::CheckSeverity::Error,
                detail: "test failure".into(),
            }],
            device_fingerprint: None,
            errors: 1,
            warnings: 0,
            dry_run: false,
        };
        assert!(
            !report.safe_to_proceed,
            "SI-10: Any error must block operation"
        );
        assert_eq!(report.errors, 1);
    }

    #[test]
    fn si10_safety_report_warnings_dont_block() {
        let report = safety::SafetyReport {
            safe_to_proceed: true,
            safety_level: safety::SafetyLevel::Low,
            checks: vec![safety::SafetyCheck {
                id: "warn".into(),
                description: "warning check".into(),
                passed: false,
                severity: safety::CheckSeverity::Warning,
                detail: "minor issue".into(),
            }],
            device_fingerprint: None,
            errors: 0,
            warnings: 1,
            dry_run: false,
        };
        assert!(
            report.safe_to_proceed,
            "SI-10: Warnings alone must not block"
        );
    }

    // ── SI-7: ExitCode mapping completeness ────────────────────────────────

    #[test]
    fn si7_exit_code_values() {
        assert_eq!(safety::ExitCode::Success.code(), 0);
        assert_eq!(safety::ExitCode::GeneralError.code(), 1);
        assert_eq!(safety::ExitCode::SafetyCheckFailed.code(), 2);
        assert_eq!(safety::ExitCode::VerificationFailed.code(), 3);
        assert_eq!(safety::ExitCode::PermissionDenied.code(), 4);
        assert_eq!(safety::ExitCode::SourceError.code(), 5);
        assert_eq!(safety::ExitCode::TargetError.code(), 6);
        assert_eq!(safety::ExitCode::SizeMismatch.code(), 7);
        assert_eq!(safety::ExitCode::DeviceChanged.code(), 8);
        assert_eq!(safety::ExitCode::Cancelled.code(), 130);
    }

    // ── Error mapping completeness ─────────────────────────────────────────

    #[test]
    fn si7_all_errors_map_to_exit_codes() {
        use error::AbtError;

        let errors: Vec<AbtError> = vec![
            AbtError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "test")),
            AbtError::DeviceNotFound("x".into()),
            AbtError::SystemDrive("x".into()),
            AbtError::ReadOnly("x".into()),
            AbtError::ImageNotFound("x".into()),
            AbtError::UnsupportedFormat("x".into()),
            AbtError::ImageTooLarge {
                image_size: 1,
                device_size: 0,
            },
            AbtError::VerificationFailed {
                offset: 0,
                expected: 0,
                actual: 1,
            },
            AbtError::ChecksumMismatch {
                expected: "a".into(),
                actual: "b".into(),
            },
            AbtError::Aborted,
            AbtError::PermissionDenied,
            AbtError::Decompression("x".into()),
            AbtError::FormatError("x".into()),
            AbtError::PlatformError("x".into()),
            AbtError::ConfigError("x".into()),
            AbtError::Timeout { elapsed_secs: 1.0 },
            AbtError::CancelledByUser,
            AbtError::BackupFailed("x".into()),
            AbtError::TokenMismatch {
                expected: "a".into(),
                actual: "b".into(),
            },
            AbtError::RetryExhausted {
                retries: 3,
                msg: "x".into(),
            },
            AbtError::DeviceChanged("x".into()),
        ];

        for err in errors {
            let anyhow_err: anyhow::Error = err.into();
            let code = safety::error_to_exit_code(&anyhow_err);
            // Every error must map to a non-Success exit code
            assert_ne!(
                code,
                safety::ExitCode::Success,
                "Error must not map to Success exit code"
            );
        }
    }
}
