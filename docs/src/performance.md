# Performance Tuning

`abt` includes several performance optimization features for high-throughput disk imaging.

## Block Size

The default block size is 4 MiB. Use `--block-size` or the adaptive auto-tuner:

```bash
# Manual block size
abt write -i image.iso -o /dev/sdb -b 8M

# Auto-tune (benchmark-based)
abt write -i image.iso -o /dev/sdb --auto-block-size
```

## Direct I/O

Bypass the OS page cache for large sequential writes:

```bash
abt write -i image.iso -o /dev/sdb --direct-io
```

- Linux: `O_DIRECT` with aligned buffers
- Windows: `FILE_FLAG_NO_BUFFERING` + `FILE_FLAG_WRITE_THROUGH`

## Zero-Copy Transfers

When source and target are both files/devices on the same system:

- **Linux**: `splice(2)` for kernel-to-kernel data transfer
- **macOS/FreeBSD**: `sendfile(2)` for file-to-socket zero-copy
- Falls back to buffered I/O on other platforms

## io_uring (Linux)

On Linux kernel 5.1+, `abt` uses `io_uring` for asynchronous I/O with aligned double-buffered pipeline. Graceful fallback on older kernels.

## Sparse Writes

Skip all-zero blocks to dramatically speed up partially-empty images:

```bash
abt write -i image.raw -o /dev/sdb --sparse
```

## Benchmarking

```bash
# Full I/O benchmark
abt bench /dev/sdb

# Specific block sizes with JSON output
abt bench /dev/sdb -b 64K -b 1M -b 4M --json
```

## Parallel Decompression

Compressed images use multi-threaded decompression pipelines. Thread count auto-detected from CPU cores, configurable via environment.
