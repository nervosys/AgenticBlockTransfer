//! Benchmarking suite — measure I/O throughput for block size auto-tuning and comparison.
//!
//! Provides `abt bench` functionality to measure sequential write / read / verify
//! throughput at various block sizes, generating a report useful for:
//!   - Choosing optimal block size for a given device
//!   - Comparing abt throughput against dd on the same hardware
//!   - Validating that I/O optimizations (O_DIRECT, sparse, io_uring) help

use std::io::{Read, Write, Seek, SeekFrom};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use log::info;
use serde::{Deserialize, Serialize};

/// Default block sizes to benchmark (powers of 2 from 4 KiB to 16 MiB).
const DEFAULT_BLOCK_SIZES: &[usize] = &[
    4 * 1024,         // 4 KiB
    16 * 1024,        // 16 KiB
    64 * 1024,        // 64 KiB
    256 * 1024,       // 256 KiB
    1024 * 1024,      // 1 MiB
    4 * 1024 * 1024,  // 4 MiB
    16 * 1024 * 1024, // 16 MiB
];

/// Benchmark configuration.
#[derive(Debug, Clone)]
pub struct BenchConfig {
    /// Target file/device to benchmark.
    pub target: String,
    /// Total bytes to write/read per benchmark iteration.
    pub test_size: u64,
    /// Block sizes to test. If empty, uses DEFAULT_BLOCK_SIZES.
    pub block_sizes: Vec<usize>,
    /// Number of iterations per block size for averaging.
    pub iterations: u32,
    /// Run write benchmark.
    pub bench_write: bool,
    /// Run read benchmark.
    pub bench_read: bool,
    /// Use O_DIRECT / FILE_FLAG_NO_BUFFERING.
    pub direct_io: bool,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            target: String::new(),
            test_size: 64 * 1024 * 1024, // 64 MiB default
            block_sizes: Vec::new(),
            iterations: 3,
            bench_write: true,
            bench_read: true,
            direct_io: false,
        }
    }
}

/// A single benchmark measurement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchMeasurement {
    /// Block size used.
    pub block_size: usize,
    /// Operation type.
    pub operation: String,
    /// Bytes transferred.
    pub bytes: u64,
    /// Duration in milliseconds.
    pub duration_ms: f64,
    /// Throughput in MiB/s.
    pub throughput_mib_s: f64,
    /// IOPS (I/O operations per second).
    pub iops: f64,
}

/// Complete benchmark report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchReport {
    /// Target path benchmarked.
    pub target: String,
    /// Test size per iteration.
    pub test_size: u64,
    /// Number of iterations per measurement.
    pub iterations: u32,
    /// All measurements.
    pub measurements: Vec<BenchMeasurement>,
    /// Recommended block size (highest write throughput).
    pub recommended_block_size: usize,
    /// Peak write throughput observed.
    pub peak_write_mib_s: f64,
    /// Peak read throughput observed.
    pub peak_read_mib_s: f64,
}

/// Run the full benchmark suite.
pub fn run_benchmark(config: &BenchConfig) -> Result<BenchReport> {
    let block_sizes = if config.block_sizes.is_empty() {
        DEFAULT_BLOCK_SIZES.to_vec()
    } else {
        config.block_sizes.clone()
    };

    let mut measurements = Vec::new();
    let mut best_write_throughput = 0.0f64;
    let mut best_write_block = block_sizes[0];
    let mut peak_read = 0.0f64;

    info!(
        "Benchmarking {} — test_size={} MiB, iterations={}, direct_io={}",
        config.target,
        config.test_size / (1024 * 1024),
        config.iterations,
        config.direct_io
    );

    for &block_size in &block_sizes {
        if config.bench_write {
            let m = bench_write(&config.target, block_size, config.test_size, config.iterations)?;
            info!(
                "  Write block_size={:>8}  throughput={:.1} MiB/s  iops={:.0}",
                humanize_block_size(block_size),
                m.throughput_mib_s,
                m.iops
            );
            if m.throughput_mib_s > best_write_throughput {
                best_write_throughput = m.throughput_mib_s;
                best_write_block = block_size;
            }
            measurements.push(m);
        }

        if config.bench_read {
            let m = bench_read(&config.target, block_size, config.test_size, config.iterations)?;
            info!(
                "  Read  block_size={:>8}  throughput={:.1} MiB/s  iops={:.0}",
                humanize_block_size(block_size),
                m.throughput_mib_s,
                m.iops
            );
            if m.throughput_mib_s > peak_read {
                peak_read = m.throughput_mib_s;
            }
            measurements.push(m);
        }
    }

    Ok(BenchReport {
        target: config.target.clone(),
        test_size: config.test_size,
        iterations: config.iterations,
        measurements,
        recommended_block_size: best_write_block,
        peak_write_mib_s: best_write_throughput,
        peak_read_mib_s: peak_read,
    })
}

/// Benchmark sequential write throughput at a given block size.
fn bench_write(
    target: &str,
    block_size: usize,
    test_size: u64,
    iterations: u32,
) -> Result<BenchMeasurement> {
    let mut total_duration = Duration::ZERO;
    let blocks_per_iter = (test_size as usize) / block_size;

    // Pre-fill a buffer with patterned data (not all zeros — avoids filesystem sparse optimization)
    let buf: Vec<u8> = (0..block_size).map(|i| (i % 251) as u8).collect();

    for _ in 0..iterations {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(target)
            .with_context(|| format!("Cannot open {} for write benchmark", target))?;

        let start = Instant::now();
        for _ in 0..blocks_per_iter {
            file.write_all(&buf)?;
        }
        file.flush()?;
        file.sync_all()?;
        total_duration += start.elapsed();
    }

    let avg_duration = total_duration / iterations;
    let secs = avg_duration.as_secs_f64();
    let mib_s = if secs > 0.0 {
        (test_size as f64) / (1024.0 * 1024.0) / secs
    } else {
        0.0
    };
    let iops = if secs > 0.0 {
        blocks_per_iter as f64 / secs
    } else {
        0.0
    };

    Ok(BenchMeasurement {
        block_size,
        operation: "write".to_string(),
        bytes: test_size,
        duration_ms: avg_duration.as_secs_f64() * 1000.0,
        throughput_mib_s: mib_s,
        iops,
    })
}

/// Benchmark sequential read throughput at a given block size.
fn bench_read(
    target: &str,
    block_size: usize,
    test_size: u64,
    iterations: u32,
) -> Result<BenchMeasurement> {
    // Ensure the file exists with test data
    let file_size = match std::fs::metadata(target) {
        Ok(m) => m.len(),
        Err(_) => 0,
    };
    if file_size < test_size {
        // Write test data first
        let buf: Vec<u8> = (0..block_size).map(|i| (i % 251) as u8).collect();
        let blocks = (test_size as usize) / block_size;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(target)?;
        for _ in 0..blocks {
            f.write_all(&buf)?;
        }
        f.sync_all()?;
    }

    let mut total_duration = Duration::ZERO;
    let mut buf = vec![0u8; block_size];

    for _ in 0..iterations {
        let mut file = std::fs::File::open(target)
            .with_context(|| format!("Cannot open {} for read benchmark", target))?;

        file.seek(SeekFrom::Start(0))?;

        let start = Instant::now();
        let mut total_read = 0u64;
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    total_read += n as u64;
                    if total_read >= test_size {
                        break;
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
        total_duration += start.elapsed();
    }

    let avg_duration = total_duration / iterations;
    let secs = avg_duration.as_secs_f64();
    let blocks_per_iter = (test_size as usize) / block_size;
    let mib_s = if secs > 0.0 {
        (test_size as f64) / (1024.0 * 1024.0) / secs
    } else {
        0.0
    };
    let iops = if secs > 0.0 {
        blocks_per_iter as f64 / secs
    } else {
        0.0
    };

    Ok(BenchMeasurement {
        block_size,
        operation: "read".to_string(),
        bytes: test_size,
        duration_ms: avg_duration.as_secs_f64() * 1000.0,
        throughput_mib_s: mib_s,
        iops,
    })
}

fn humanize_block_size(size: usize) -> String {
    if size >= 1024 * 1024 {
        format!("{} MiB", size / (1024 * 1024))
    } else if size >= 1024 {
        format!("{} KiB", size / 1024)
    } else {
        format!("{} B", size)
    }
}

/// Format a benchmark report for terminal display.
pub fn format_report(report: &BenchReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("Benchmark Report: {}\n", report.target));
    out.push_str(&format!(
        "Test size: {} MiB | Iterations: {}\n",
        report.test_size / (1024 * 1024),
        report.iterations
    ));
    out.push_str(&"═".repeat(70));
    out.push('\n');
    out.push_str(&format!(
        "{:<12} {:<10} {:>12} {:>12} {:>10}\n",
        "Block Size", "Operation", "Duration", "Throughput", "IOPS"
    ));
    out.push_str(&"─".repeat(70));
    out.push('\n');

    for m in &report.measurements {
        out.push_str(&format!(
            "{:<12} {:<10} {:>9.1} ms {:>8.1} MiB/s {:>10.0}\n",
            humanize_block_size(m.block_size),
            m.operation,
            m.duration_ms,
            m.throughput_mib_s,
            m.iops,
        ));
    }

    out.push_str(&"═".repeat(70));
    out.push('\n');
    out.push_str(&format!(
        "Recommended block size: {}\n",
        humanize_block_size(report.recommended_block_size)
    ));
    out.push_str(&format!("Peak write: {:.1} MiB/s\n", report.peak_write_mib_s));
    out.push_str(&format!("Peak read:  {:.1} MiB/s\n", report.peak_read_mib_s));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_bench_config_default() {
        let config = BenchConfig::default();
        assert_eq!(config.test_size, 64 * 1024 * 1024);
        assert_eq!(config.iterations, 3);
        assert!(config.bench_write);
        assert!(config.bench_read);
    }

    #[test]
    fn test_bench_write() {
        let tmp = NamedTempFile::new().unwrap();
        let result = bench_write(
            tmp.path().to_str().unwrap(),
            4096,
            32 * 1024, // 32 KiB
            1,
        )
        .unwrap();

        assert_eq!(result.block_size, 4096);
        assert_eq!(result.operation, "write");
        assert_eq!(result.bytes, 32 * 1024);
        assert!(result.throughput_mib_s > 0.0);
        assert!(result.iops > 0.0);
    }

    #[test]
    fn test_bench_read() {
        let tmp = NamedTempFile::new().unwrap();
        let result = bench_read(
            tmp.path().to_str().unwrap(),
            4096,
            32 * 1024,
            1,
        )
        .unwrap();

        assert_eq!(result.block_size, 4096);
        assert_eq!(result.operation, "read");
        assert!(result.throughput_mib_s > 0.0);
    }

    #[test]
    fn test_run_benchmark() {
        let tmp = NamedTempFile::new().unwrap();
        let config = BenchConfig {
            target: tmp.path().to_str().unwrap().to_string(),
            test_size: 16 * 1024, // 16 KiB (small for tests)
            block_sizes: vec![1024, 4096],
            iterations: 1,
            bench_write: true,
            bench_read: true,
            direct_io: false,
        };

        let report = run_benchmark(&config).unwrap();
        assert_eq!(report.measurements.len(), 4); // 2 sizes × 2 ops
        assert!(report.peak_write_mib_s > 0.0);
        assert!(report.peak_read_mib_s > 0.0);
        assert!(report.recommended_block_size > 0);
    }

    #[test]
    fn test_format_report() {
        let report = BenchReport {
            target: "test".to_string(),
            test_size: 1024 * 1024,
            iterations: 1,
            measurements: vec![BenchMeasurement {
                block_size: 4096,
                operation: "write".to_string(),
                bytes: 1024 * 1024,
                duration_ms: 50.0,
                throughput_mib_s: 20.0,
                iops: 5000.0,
            }],
            recommended_block_size: 4096,
            peak_write_mib_s: 20.0,
            peak_read_mib_s: 15.0,
        };

        let output = format_report(&report);
        assert!(output.contains("Benchmark Report"));
        assert!(output.contains("20.0 MiB/s"));
        assert!(output.contains("4 KiB"));
    }

    #[test]
    fn test_humanize_block_size() {
        assert_eq!(humanize_block_size(512), "512 B");
        assert_eq!(humanize_block_size(4096), "4 KiB");
        assert_eq!(humanize_block_size(1024 * 1024), "1 MiB");
        assert_eq!(humanize_block_size(4 * 1024 * 1024), "4 MiB");
    }

    #[test]
    fn test_bench_write_only() {
        let tmp = NamedTempFile::new().unwrap();
        let config = BenchConfig {
            target: tmp.path().to_str().unwrap().to_string(),
            test_size: 8 * 1024,
            block_sizes: vec![1024],
            iterations: 1,
            bench_write: true,
            bench_read: false,
            direct_io: false,
        };

        let report = run_benchmark(&config).unwrap();
        assert_eq!(report.measurements.len(), 1);
        assert_eq!(report.measurements[0].operation, "write");
    }
}
