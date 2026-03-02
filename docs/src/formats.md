# Image Formats

`abt` supports reading and writing a wide range of disk image formats.

## Supported Formats

| Format | Extensions             | Detection       | Notes                                  |
| ------ | ---------------------- | --------------- | -------------------------------------- |
| RAW    | `.img`, `.raw`, `.bin` | Fallback        | Direct block-level copy                |
| ISO    | `.iso`                 | Magic bytes     | ISO 9660 with El Torito boot detection |
| DMG    | `.dmg`                 | Extension       | macOS disk image                       |
| QCOW2  | `.qcow2`               | Magic `QFI\xfb` | v2/v3, L1→L2→cluster chain             |
| VHD    | `.vhd`                 | Footer magic    | Fixed and Dynamic variants             |
| VHDX   | `.vhdx`                | Identifier      | Header parsing                         |
| VMDK   | `.vmdk`                | Magic bytes     | Sparse extent, grain directory/table   |
| WIM    | `.wim`                 | Magic `MSWIM`   | Header, compression, XML metadata      |
| FFU    | `.ffu`                 | Extension       | Windows Full Flash Update              |

## Compression

Decompression is automatic and detected via magic bytes (not file extensions):

| Format | Magic Bytes      | Library |
| ------ | ---------------- | ------- |
| gzip   | `1f 8b`          | flate2  |
| bzip2  | `BZ`             | bzip2   |
| xz     | `fd 37 7a 58 5a` | xz2     |
| zstd   | `28 b5 2f fd`    | zstd    |
| zip    | `PK\x03\x04`     | zip     |

## Parallel Decompression

For large compressed images, `abt` uses a multi-threaded decompression pipeline:

- **bz2/zstd** — parallel block decompress (multiple threads decode independent blocks)
- **gz/xz** — read-ahead pipeline (background thread reads + decompresses ahead of writer)

## Virtual Disk Formats

QCOW2, VHD, VHDX, and VMDK images are transparently converted to raw block data during write. The conversion is streaming — the full virtual disk is never materialized in memory.

## Plugin System

Custom image format handlers can be registered via the `FormatPlugin` trait:

```rust
pub trait FormatPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn can_handle(&self, magic: &[u8], ext: &str) -> bool;
    fn create_reader(&self, file: File) -> Result<Box<dyn Read>>;
}
```
