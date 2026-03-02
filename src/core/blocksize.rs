// Adaptive block size tuning — auto-select optimal I/O block size.
//
// Performs a quick sequential write benchmark on the target device to determine
// the throughput curve, then selects the block size that maximizes throughput
// without excessive memory use.
//
// Strategy:
//   1. Write a small number of blocks at each candidate size (64K, 256K, 1M, 4M, 8M, 16M).
//   2. Measure wall-clock time per block size.
//   3. Select the size with best throughput, applying a diminishing-returns threshold
//      (if doubling block size gains <10% throughput, stop).

#![allow(dead_code)]

use anyhow::Result;
use log::{debug, info};
use std::io::{Seek, SeekFrom, Write};
use std::time::Instant;

/// Candidate block sizes to benchmark, in ascending order.
pub(crate) const CANDIDATES: &[usize] = &[
    64 * 1024,        // 64 KiB
    256 * 1024,       // 256 KiB
    1024 * 1024,      // 1 MiB
    4 * 1024 * 1024,  // 4 MiB
    8 * 1024 * 1024,  // 8 MiB
    16 * 1024 * 1024, // 16 MiB
];

/// Number of blocks to write per candidate for benchmarking.
/// Each block is written once, so total data per candidate = block_size * BENCH_BLOCKS.
pub(crate) const BENCH_BLOCKS: usize = 4;

/// Minimum improvement threshold (fraction) to justify a larger block size.
/// If going from size N to size 2N gains less than this fraction of throughput,
/// we stop and use size N.
pub(crate) const DIMINISHING_RETURNS_THRESHOLD: f64 = 0.10;

/// Maximum block size we'll ever recommend (16 MiB). Beyond this, memory use
/// and alignment issues outweigh throughput gains on most hardware.
pub const MAX_BLOCK_SIZE: usize = 16 * 1024 * 1024;

/// Minimum block size floor (64 KiB). Even if the benchmark shows lower sizes
/// are faster (unlikely), we never go below this.
pub const MIN_BLOCK_SIZE: usize = 64 * 1024;

/// Result of a single block-size benchmark.
#[derive(Debug, Clone)]
pub struct BlockSizeBenchmark {
    pub block_size: usize,
    pub throughput_bytes_per_sec: f64,
    pub total_bytes: u64,
    pub elapsed_secs: f64,
}

/// Result of the adaptive tuning process.
#[derive(Debug, Clone)]
pub struct AdaptiveTuneResult {
    /// Recommended optimal block size.
    pub recommended_block_size: usize,
    /// All benchmark results for each candidate.
    pub benchmarks: Vec<BlockSizeBenchmark>,
    /// Whether the recommendation was limited by diminishing returns.
    pub hit_diminishing_returns: bool,
}

impl std::fmt::Display for AdaptiveTuneResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Adaptive Block Size Tuning Results:")?;
        writeln!(
            f,
            "  {:>10}  {:>12}  {:>10}",
            "Block Size", "Throughput", "Time"
        )?;
        writeln!(
            f,
            "  {:>10}  {:>12}  {:>10}",
            "----------", "----------", "------"
        )?;
        for b in &self.benchmarks {
            let size = humansize::format_size(b.block_size as u64, humansize::BINARY);
            let speed =
                humansize::format_size(b.throughput_bytes_per_sec as u64, humansize::BINARY);
            writeln!(
                f,
                "  {:>10}  {:>10}/s  {:>8.2}s",
                size, speed, b.elapsed_secs
            )?;
        }
        writeln!(
            f,
            "  Recommended: {}{}",
            humansize::format_size(self.recommended_block_size as u64, humansize::BINARY),
            if self.hit_diminishing_returns {
                " (diminishing returns detected)"
            } else {
                ""
            }
        )?;
        Ok(())
    }
}

/// Determine the optimal block size for writing to a target.
///
/// Opens the target for writing, performs a quick sequential write benchmark
/// at each candidate block size, then selects the best one.
///
/// This writes test data to the device, so it should only be called after
/// safety checks have passed and the user has confirmed the write target.
///
/// # Arguments
/// * `target` - Device path to benchmark
/// * `direct_io` - Whether to use direct/unbuffered I/O (matches actual write flags)
pub fn determine_optimal_block_size(target: &str, direct_io: bool) -> Result<AdaptiveTuneResult> {
    info!("Starting adaptive block size tuning on {}", target);

    let mut benchmarks = Vec::new();
    let mut best_throughput = 0.0f64;
    let mut best_size = CANDIDATES[0];
    let mut hit_diminishing_returns = false;

    for &candidate_size in CANDIDATES {
        match benchmark_block_size(target, candidate_size, direct_io) {
            Ok(result) => {
                debug!(
                    "  {} => {}/s",
                    humansize::format_size(candidate_size as u64, humansize::BINARY),
                    humansize::format_size(
                        result.throughput_bytes_per_sec as u64,
                        humansize::BINARY
                    )
                );

                let throughput = result.throughput_bytes_per_sec;
                benchmarks.push(result);

                if throughput > best_throughput {
                    // Check diminishing returns
                    if best_throughput > 0.0 {
                        let improvement = (throughput - best_throughput) / best_throughput;
                        if improvement < DIMINISHING_RETURNS_THRESHOLD {
                            info!(
                                "Diminishing returns: {} -> {} is only {:.1}% improvement (threshold: {:.0}%)",
                                humansize::format_size(best_size as u64, humansize::BINARY),
                                humansize::format_size(candidate_size as u64, humansize::BINARY),
                                improvement * 100.0,
                                DIMINISHING_RETURNS_THRESHOLD * 100.0
                            );
                            hit_diminishing_returns = true;
                            break;
                        }
                    }
                    best_throughput = throughput;
                    best_size = candidate_size;
                }
            }
            Err(e) => {
                debug!("Failed to benchmark {} block size: {}", candidate_size, e);
                // If a larger size fails (e.g., alignment issues), stop
                break;
            }
        }
    }

    // Clamp to bounds
    let recommended = best_size.clamp(MIN_BLOCK_SIZE, MAX_BLOCK_SIZE);

    info!(
        "Adaptive tuning complete: recommended block size = {}",
        humansize::format_size(recommended as u64, humansize::BINARY)
    );

    Ok(AdaptiveTuneResult {
        recommended_block_size: recommended,
        benchmarks,
        hit_diminishing_returns,
    })
}

/// Benchmark a single block size by writing BENCH_BLOCKS blocks to the target.
fn benchmark_block_size(
    target: &str,
    block_size: usize,
    direct_io: bool,
) -> Result<BlockSizeBenchmark> {
    let mut file = open_for_bench(target, direct_io)?;

    // Generate a test pattern (repeating bytes — not zeros, to avoid sparse
    // optimizations on some storage controllers)
    let mut buf = vec![0u8; block_size];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = ((i % 251) + 1) as u8; // Prime-period pattern, never zero
    }

    // Seek to start
    file.seek(SeekFrom::Start(0))?;

    let start = Instant::now();
    let mut total_bytes: u64 = 0;

    for _ in 0..BENCH_BLOCKS {
        file.write_all(&buf)?;
        total_bytes += block_size as u64;
    }
    file.flush()?;

    let elapsed = start.elapsed();
    let elapsed_secs = elapsed.as_secs_f64();
    let throughput = if elapsed_secs > 0.0 {
        total_bytes as f64 / elapsed_secs
    } else {
        total_bytes as f64 // Instantaneous (unlikely but safe)
    };

    // Seek back to start to leave device in known state
    file.seek(SeekFrom::Start(0))?;

    Ok(BlockSizeBenchmark {
        block_size,
        throughput_bytes_per_sec: throughput,
        total_bytes,
        elapsed_secs,
    })
}

/// Open a device for benchmarking with the same flags as actual writes.
fn open_for_bench(path: &str, direct_io: bool) -> Result<Box<dyn WriteSeek>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut flags = libc::O_SYNC;
        if direct_io {
            flags |= libc::O_DIRECT;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(flags)
            .open(path)?;
        Ok(Box::new(file))
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let mut flags = 0u32;
        flags |= 0x80000000; // FILE_FLAG_WRITE_THROUGH
        if direct_io {
            flags |= 0x20000000; // FILE_FLAG_NO_BUFFERING
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(flags)
            .open(path)?;
        Ok(Box::new(file))
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = direct_io;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        Ok(Box::new(file))
    }
}

trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

/// Select a reasonable default block size based on device size without benchmarking.
/// Used when benchmarking is not desired or not possible.
///
/// # Safety Invariant SI-3
/// The returned block size is always within [MIN_BLOCK_SIZE, MAX_BLOCK_SIZE]
/// and is a power of two.
pub fn heuristic_block_size(device_size_bytes: u64) -> usize {
    let result = match device_size_bytes {
        0..=134_217_728 => 256 * 1024,              // ≤128 MiB → 256 KiB
        134_217_729..=4_294_967_296 => 1024 * 1024, // ≤4 GiB → 1 MiB
        4_294_967_297..=34_359_738_368 => 4 * 1024 * 1024, // ≤32 GiB → 4 MiB
        _ => 8 * 1024 * 1024,                       // >32 GiB → 8 MiB
    };

    // SI-3: Postconditions
    debug_assert!(
        result >= MIN_BLOCK_SIZE,
        "POSTCONDITION VIOLATED: heuristic_block_size returned {} < MIN_BLOCK_SIZE ({})",
        result,
        MIN_BLOCK_SIZE
    );
    debug_assert!(
        result <= MAX_BLOCK_SIZE,
        "POSTCONDITION VIOLATED: heuristic_block_size returned {} > MAX_BLOCK_SIZE ({})",
        result,
        MAX_BLOCK_SIZE
    );
    debug_assert!(
        result.is_power_of_two(),
        "POSTCONDITION VIOLATED: heuristic_block_size returned {} which is not a power of two",
        result
    );

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_small_device() {
        assert_eq!(heuristic_block_size(64 * 1024 * 1024), 256 * 1024);
    }

    #[test]
    fn heuristic_medium_device() {
        assert_eq!(heuristic_block_size(2 * 1024 * 1024 * 1024), 1024 * 1024);
    }

    #[test]
    fn heuristic_large_device() {
        assert_eq!(
            heuristic_block_size(16u64 * 1024 * 1024 * 1024),
            4 * 1024 * 1024
        );
    }

    #[test]
    fn heuristic_huge_device() {
        assert_eq!(
            heuristic_block_size(256u64 * 1024 * 1024 * 1024),
            8 * 1024 * 1024
        );
    }

    #[test]
    fn candidates_are_ascending() {
        for w in CANDIDATES.windows(2) {
            assert!(w[0] < w[1], "{} should be < {}", w[0], w[1]);
        }
    }

    #[test]
    fn min_leq_max() {
        assert!(MIN_BLOCK_SIZE <= MAX_BLOCK_SIZE);
    }

    #[test]
    fn candidates_within_bounds() {
        for &c in CANDIDATES {
            assert!(c >= MIN_BLOCK_SIZE, "{} < MIN", c);
            assert!(c <= MAX_BLOCK_SIZE, "{} > MAX", c);
        }
    }

    #[test]
    fn display_tune_result() {
        let result = AdaptiveTuneResult {
            recommended_block_size: 4 * 1024 * 1024,
            benchmarks: vec![
                BlockSizeBenchmark {
                    block_size: 256 * 1024,
                    throughput_bytes_per_sec: 100_000_000.0,
                    total_bytes: 1024 * 1024,
                    elapsed_secs: 0.01,
                },
                BlockSizeBenchmark {
                    block_size: 4 * 1024 * 1024,
                    throughput_bytes_per_sec: 250_000_000.0,
                    total_bytes: 16 * 1024 * 1024,
                    elapsed_secs: 0.064,
                },
            ],
            hit_diminishing_returns: false,
        };
        let s = format!("{}", result);
        assert!(s.contains("Recommended"));
        assert!(s.contains("Throughput"));
    }
}
