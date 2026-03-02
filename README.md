# AgenticBlockTransfer (abt)

**Agentic-first CLI successor to `dd`. Human-first GUI/TUI successor to balenaEtcher, Ventoy, Rufus, Fedora Media Writer, and rpi-imager.**

Simple. Reliable. Efficient. Fast.

AgenticBlockTransfer is a cross-platform disk imaging tool that reads, writes, verifies, hashes, and formats block devices via CLI, TUI, and GUI interfaces — with native support for AI agent orchestration through JSON-LD ontology, OpenAPI, and MCP.

## Heritage

AgenticBlockTransfer (`abt`) traces its lineage to IBM's **BLock Transfer** (BLT, pronounced "blit") — a mainframe utility for moving data between block-addressable devices. When UNIX adapted the concept, it became **Dataset Definition** (`dd`), a tool that has endured for over 50 years as the de facto standard for raw block I/O. The `dd` interface — terse, powerful, unforgiving — was designed for operators who understood the hardware.

`abt` carries this heritage forward into an era of AI agents and human-centered design:

- **For AI agents** — the CLI exposes a machine-readable [JSON-LD](https://json-ld.org/) ontology (`abt ontology`) so any agentic system can discover, parameterize, and invoke block transfer operations without human documentation. This is `dd` made natively intelligible to machines.
- **For humans** — the TUI and GUI modes provide the guided, safe experience of balenaEtcher and Rufus — device enumeration, format auto-detection, progress visualization, post-write verification — without the 500 MB Electron runtime or platform lock-in.

The name **AgenticBlockTransfer** is a direct nod: *Block Transfer* from IBM's BLT, *Agentic* because the tool is built to be operated by both humans and autonomous AI systems.

## Features

- **Multi-format** — ISO, IMG, RAW, DMG, VHD, VHDX, VMDK, QCOW2, WIM, FFU
- **Auto-decompression** — gz, bz2, xz, zstd, zip (detected via magic bytes, not file extensions)
- **Write verification** — byte-for-byte read-back with exact mismatch offset reporting
- **Multi-algorithm hashing** — SHA-256, SHA-512, MD5, BLAKE3, CRC32
- **Native device enumeration** — sysfs/lsblk (Linux), `diskutil` (macOS), PowerShell `Get-Disk` (Windows)
- **Device formatting** — ext2/3/4, FAT16/32, exFAT, NTFS, APFS, HFS+, btrfs, XFS, F2FS
- **Safety guards** — system drive protection, removable-only defaults, interactive confirmation
- **Agentic safety** — pre-flight checks, dry-run mode, device fingerprints, structured exit codes, partition table backup
- **Three UI modes** — CLI, TUI (ratatui with built-in file browser), native GUI (egui/eframe with 6 theme presets)
- **AI ontology** — JSON-LD / schema.org capability schema for agentic integration
- **Broad device scope** — USB, SD, NVMe, SATA, eMMC, SPI flash, I2C EEPROM, MTD, loopback
- **HTTP/HTTPS sources** — `abt write -i https://releases.ubuntu.com/.../ubuntu.iso -o /dev/sdb` with streaming progress
- **Sparse writes** — skip all-zero blocks (like `dd conv=sparse`) for dramatically faster large images
- **Signal handling** — graceful Ctrl+C with device sync (second Ctrl+C force-exits)
- **Shell completions** — bash, zsh, fish, PowerShell via `abt completions <shell>`
- **Structured logging** — JSON-structured file logging via `--log-file`
- **Partition introspection** — GPT/MBR partition table parsing for `abt info` and safety reports
- **Man pages** — `abt man` generates roff man pages for all commands
- **Config file** — `~/.config/abt/config.toml` for persistent defaults (block size, verify, safety level)
- **CI/CD** — GitHub Actions with Linux + macOS + Windows matrix, clippy, fmt, feature combinations
- **ISO 9660 metadata** — volume label, El Torito boot detection, Joliet extensions via `abt info`
- **Native file dialog** — rfd-powered Browse / Open Image in GUI mode
- **Desktop notifications** — OS-native toast on write completion / failure (notify-rust)
- **Adaptive block size** — auto-tune I/O block size via sequential benchmark or device-size heuristic
- **Loopback device testing** — safe automated write/read testing without real media
- **Error recovery / resume** — resume interrupted writes from JSON checkpoint with integrity verification
- **Drag-and-drop** — drop image files onto the GUI window with hover overlay and extension filtering
- **QCOW2 image reader** — transparent QCOW2 v2/v3 → raw streaming (L1→L2→cluster chain)
- **VHD/VHDX image reader** — VHD Fixed + Dynamic, VHDX header parsing, transparent → raw streaming
- **VMDK image reader** — VMware sparse extent parsing (grain directory/table chain), transparent → raw streaming
- **WIM metadata parser** — header, compression flags, GUID, XML image metadata, boot index via `abt info`
- **Plugin/extension system** — `FormatPlugin` trait + `PluginRegistry` for custom image format handlers
- **Memory-mapped verification** — `memmap2`-based verify with automatic fallback to standard I/O
- **MCP server mode** — `abt mcp` exposes all capabilities via Model Context Protocol (JSON-RPC 2.0 over stdio)
- **Device cloning** — `abt clone` for block-level device-to-device copy with inline hashing, sparse optimization, post-clone verification
- **Secure erase** — `abt erase` with 6 methods: auto, zero-fill, random-fill, ATA Secure Erase, NVMe Sanitize, TRIM/discard; multi-pass support
- **Boot sector validation** — `abt boot` validates MBR signature, boot code, GPT header, EFI System Partition, bootloader jump instructions
- **Raspberry Pi OS catalog** — `abt catalog` fetches and browses the official rpi-imager OS list with search and filtering
- **OpenAPI schema** — `abt ontology -f openapi` generates an OpenAPI 3.1 spec with 9 endpoints and 12 schemas for REST wrapper integration
- **YAML ontology output** — `abt ontology -f yaml` exports capability schema in YAML format
- **FreeBSD support** — device enumeration via sysctl + geom, unmount, elevation check
- **Async I/O (io_uring)** — Linux kernel 5.1+ io_uring with aligned double-buffered pipeline; graceful fallback on other platforms
- **Zero-copy transfers** — splice(2) on Linux, sendfile(2) on macOS/FreeBSD; automatic fallback to buffered I/O
- **Benchmarking suite** — `abt bench` — sequential read/write throughput at multiple block sizes, IOPS, recommended block size, JSON export
- **Network Block Device source** — `abt write -i nbd://server:port/export -o /dev/sdb` with NBD protocol client (new-style handshake)
- **Differential writes** — `abt diff` — block-level comparison, only writes changed blocks, dry-run mode, skip percentage reporting
- **Parallel decompression** — pigz/pbzip2-style multi-threaded decompression pipeline (parallel block decompress for bz2/zstd, read-ahead for gz/xz)
- **Multicast imaging** — UDP multicast sender/receiver for flashing multiple devices simultaneously; CRC32 per-chunk integrity, NAK recovery, session ID
- **Ventoy-style multi-boot** — `abt multiboot` — multi-ISO USB with auto-detected GRUB2 config, registry management, OS type detection
- **Localization (i18n)** — 12 locales with 4 built-in message catalogs (en/de/fr/es), positional format args, auto-detect system locale
- **Accessibility (a11y)** — 16 ARIA roles, WCAG 2.1 AA high-contrast palette, announcement queue, screen-reader hints, keyboard-only mode
- **OS Customization** — `abt customize` generates firstrun.sh / cloud-init YAML for hostname, SSH keys, WiFi, users, timezone, locale, packages
- **Image download cache** — `abt cache` — SHA-256 verified local download cache with eviction policies (max-age/entries/size), manifest persistence
- **Drive health diagnostics** — `abt health` — multi-pass bad block detection (6 patterns), fake flash drive detection, quick read-only check
- **Sleep inhibitor** — RAII-guarded OS sleep prevention during writes (systemd-inhibit on Linux, caffeinate on macOS, SetThreadExecutionState on Windows)
- **Drive backup** — `abt backup` — save drive contents to compressed image with 5 formats (none/gzip/zstd/bzip2/xz), inline SHA-256, sparse zero-skip
- **Persistent storage** — `abt persist` — create persistent storage for live Linux USB (casper/Fedora/Ventoy modes), partition or image file based

## Quick Start

```bash
# Install
cargo install --path .

# List devices
abt list

# Write an image (auto-detects format, decompresses, verifies)
abt write -i ubuntu-24.04.iso -o /dev/sdb

# Write directly from a URL (streamed download → write)
abt write -i https://releases.ubuntu.com/24.04/ubuntu-24.04-desktop-amd64.iso -o /dev/sdb

# Sparse write (skip zero blocks — faster for images with empty space)
abt write -i image.raw -o /dev/sdb --sparse

# Windows
abt write -i image.img -o \\.\PhysicalDrive1

# Verify written data
abt verify -i ubuntu-24.04.iso -o /dev/sdb

# Checksum
abt checksum image.iso

# Export AI ontology
abt ontology --full

# Generate shell completions
abt completions bash > /etc/bash_completion.d/abt
abt completions powershell | Out-String | Invoke-Expression

# Generate man pages
abt man --output-dir /usr/local/share/man/man1/

# Start MCP server (for AI agent integration via Model Context Protocol)
abt mcp

# MCP single-request mode (process one JSON-RPC message and exit)
abt mcp --oneshot

# Inspect a QCOW2 image
abt info disk.qcow2

# Inspect a VMDK image (grain layout, embedded descriptor)
abt info disk.vmdk

# Inspect a WIM file (compression, image count, XML metadata)
abt info install.wim

# Clone a device (block-level copy with verification)
abt clone -i /dev/sda -o /dev/sdb --verify --sparse

# Securely erase a device (auto-selects best method)
abt erase /dev/sdb

# Multi-pass random erase
abt erase /dev/sdb --method random --passes 3

# Validate boot sector of a device or image
abt boot /dev/sdb
abt boot ubuntu.iso --json

# Browse Raspberry Pi OS catalog
abt catalog
abt catalog --search ubuntu --flat

# Export ontology as OpenAPI schema
abt ontology -f openapi
abt ontology -f yaml

# Write from a VHD image (transparent format conversion)
abt write -i disk.vhd -o /dev/sdb

# Write from a VMDK image
abt write -i disk.vmdk -o /dev/sdb
```

```bash
# Benchmark I/O throughput (find optimal block size)
abt bench /dev/sdb --test-size 128

# Benchmark specific block sizes
abt bench /dev/sdb -b 64K -b 1M -b 4M --json

# Differential write (only write changed blocks)
abt diff -i updated-image.iso -o /dev/sdb

# Differential write dry-run (see what would change)
abt diff -i updated-image.iso -o /dev/sdb --dry-run

# Write from a Network Block Device server
abt write -i nbd://192.168.1.10:10809/export -o /dev/sdb
```

## Safety: Why abt Is Safer Than dd

`dd` will silently destroy any target — including your boot drive — with zero validation. It has two exit codes (0 and 1), no dry-run mode, and no way to confirm you're writing to the right device. For an AI agent invoking `dd`, one wrong character in a device path means catastrophic data loss with no warning and no undo.

`abt` is designed from the ground up to prevent this:

### Pre-flight Safety Checks

Every write operation runs a structured pre-flight analysis before any bytes are written:

```bash
# See exactly what will happen without writing anything
abt write -i ubuntu.iso -o /dev/sdb --dry-run

# JSON output for agent consumption
abt write -i ubuntu.iso -o /dev/sdb --dry-run -o json
```

The safety report checks:
- **Source exists and is readable** — no silent failures
- **Image format detected** — auto-identified from magic bytes
- **Target device exists** — confirmed via OS device enumeration
- **Not a system drive** — blocks writes to boot/OS drives (ERROR)
- **Not read-only** — detects write-protect switches
- **Removable media check** — warns or blocks non-removable targets
- **No mounted filesystems** — detects in-use partitions
- **Image fits on device** — size validation before writing
- **Self-write protection** — refuses if source is on target device
- **Privilege check** — warns if not elevated

### Safety Levels

| Level      | Default for  | Behavior                                                                     |
| ---------- | ------------ | ---------------------------------------------------------------------------- |
| `normal`   | Humans       | Blocks system drives, interactive y/N confirmation                           |
| `cautious` | Agents       | + validates image size, warns on non-removable, requires token or prompt     |
| `paranoid` | Critical ops | + requires `--confirm-token`, backs up partition table, only removable media |

```bash
abt write -i image.iso -o /dev/sdb --safety-level cautious
abt write -i image.iso -o /dev/sdb --safety-level paranoid --confirm-token <token>
```

### Device Fingerprints (TOCTOU Prevention)

Agents enumerate devices and write in separate steps. Without protection, the device could change between enumeration and write. `abt` solves this with device fingerprints:

```bash
# Step 1: Agent enumerates devices, gets fingerprints
abt list --json
# → { "devices": [{ "path": "/dev/sdb", ..., "confirm_token": "a1b2c3d4..." }] }

# Step 2: Agent writes using the token — abt verifies the device hasn't changed
abt write -i image.iso -o /dev/sdb --confirm-token a1b2c3d4... --safety-level cautious
```

### Structured Exit Codes

`dd` returns 0 or 1. `abt` returns specific exit codes so agents can programmatically determine exactly what went wrong:

| Code | Meaning                                     |
| ---- | ------------------------------------------- |
| 0    | Success                                     |
| 1    | General error                               |
| 2    | Safety check failed (blocked by pre-flight) |
| 3    | Verification failed (data mismatch)         |
| 4    | Permission denied                           |
| 5    | Source not found/unreadable                 |
| 6    | Target not found/read-only                  |
| 7    | Image too large for device                  |
| 8    | Device changed (token mismatch)             |
| 130  | Cancelled (Ctrl+C / SIGINT)                 |

### Partition Table Backup

```bash
# Automatic at paranoid level, optional otherwise
abt write -i image.iso -o /dev/sdb --backup-partition-table
```

Saves the first 1 MiB (MBR + GPT) to a timestamped file in the system temp directory before writing.

### dd vs abt Safety Comparison

| Scenario                 | dd                   | abt                                   |
| ------------------------ | -------------------- | ------------------------------------- |
| Write to system drive    | Silently destroys OS | **BLOCKED** (unless --force)          |
| Wrong device path        | Silently overwrites  | **Pre-flight check + confirm**        |
| Image larger than device | Writes until error   | **BLOCKED** before write              |
| Source on target device  | Destroys source      | **BLOCKED** (self-write detect)       |
| No elevated privileges   | Cryptic error        | **Clear warning** with instructions   |
| Mounted filesystem       | Corrupts filesystem  | **Warning** + auto-unmount            |
| Verify write succeeded   | Not possible         | **Built-in** (default on)             |
| Agent invocation         | No guardrails        | **3-level safety + dry-run + tokens** |
| Error diagnosis          | Exit code 0 or 1     | **10 structured exit codes + JSON**   |
| Undo                     | Not possible         | **Partition table backup**            |

## Installation

Requires Rust 1.70+.

```bash
git clone https://github.com/nervosys/AgenticBlockTransfer.git
cd AgenticBlockTransfer
cargo build --release
```

Binary: `target/release/abt` (`abt.exe` on Windows).

### Feature Flags

| Feature | Default | Description                               |
| ------- | ------- | ----------------------------------------- |
| `cli`   | ✅       | Command-line interface (always available) |
| `tui`   | ✅       | Terminal UI via ratatui + crossterm       |
| `gui`   | ✅       | Native GUI via egui/eframe                |

CLI-only build:

```bash
cargo build --release --no-default-features --features cli
```

## Commands

| Command       | Aliases                  | Description                                |
| ------------- | ------------------------ | ------------------------------------------ |
| `write`       | `flash`, `dd`            | Write an image to a target device          |
| `verify`      | —                        | Verify written data against source or hash |
| `list`        | `devices`, `ls`          | List available block devices               |
| `info`        | `inspect`                | Show detailed device or image information  |
| `checksum`    | `hash`                   | Compute file/device checksums              |
| `format`      | `mkfs`                   | Format a device with a filesystem          |
| `ontology`    | `schema`, `capabilities` | Export AI-discoverable capability ontology |
| `completions` | —                        | Generate shell completions                 |
| `tui`         | —                        | Launch interactive terminal UI             |
| `gui`         | —                        | Launch native graphical UI                 |
| `multiboot`   | `ventoy`                 | Manage multi-boot USB devices              |

### Write

```bash
# Basic write (auto-detects format, decompresses, verifies)
abt write -i image.iso -o /dev/sdb

# Compressed image (auto-decompressed)
abt write -i firmware.img.xz -o /dev/mmcblk0

# Custom block size, skip verification
abt write -i image.raw -o /dev/sdb -b 1M --no-verify

# Force (skip confirmation)
abt write -i image.img -o /dev/sdb --force

# Dry-run (safety check only, no write)
abt write -i image.iso -o /dev/sdb --dry-run

# Agent-safe write with device token
abt write -i image.iso -o /dev/sdb --safety-level cautious --confirm-token <token>
```

### List Devices

```bash
abt list                 # removable devices
abt list --all           # all devices including system drives
abt list --type usb      # filter by type
abt list --json          # JSON output with device fingerprints (for agents)
```

### Verify

```bash
abt verify -i image.iso -o /dev/sdb
abt verify -o /dev/sdb --expected-hash sha256:abc123...
```

### Checksum

```bash
abt checksum image.iso
abt checksum image.iso -a sha256 -a blake3 -a md5
```

### Format

```bash
abt format /dev/sdb -f fat32 -l "BOOT"
abt format /dev/sdb -f ext4 --quick
```

### Ontology

```bash
abt ontology --full            # full JSON-LD ontology
abt ontology -f json           # JSON format
abt ontology --category verification
```

## AI Ontology

`abt ontology --full` emits a complete [JSON-LD](https://json-ld.org/) capability schema using [schema.org](https://schema.org/) vocabulary. An AI agent can call this once to learn everything it needs to operate `abt` — parameters, types, constraints, defaults, examples, exit codes, preconditions, postconditions — without reading documentation.

```json
{
  "@context": {
    "schema": "https://schema.org/",
    "abt": "https://github.com/nervosys/AgenticBlockTransfer/ontology#"
  },
  "@type": "schema:SoftwareApplication",
  "@id": "abt:AgenticBlockTransfer",
  "abt:capabilities": [
    {
      "@type": "schema:Action",
      "schema:name": "write",
      "abt:cliCommand": "abt write",
      "abt:destructive": true,
      "abt:parameters": [...]
    }
  ]
}
```

The ontology covers:

- **7 capabilities** — write, verify, list, info, checksum, format, ontology
- **Type definitions** — image formats, compression, device types, filesystems, hash algorithms
- **Platform matrix** — OS-specific device paths, elevation methods, requirements
- **Device scope** — microcontroller/embedded, removable media, desktop/workstation, cloud/server
- **Exit codes** — structured error semantics for automation

## Architecture

```shell
src/
├── main.rs              # Entry point, command dispatch, signal handling
├── lib.rs               # Library root with feature gates
├── core/
│   ├── types.rs         # ImageFormat, DeviceType, HashAlgorithm, WriteConfig
│   ├── device.rs        # DeviceInfo, DeviceEnumerator trait
│   ├── safety.rs        # Pre-flight checks, SafetyLevel, DeviceFingerprint, ExitCode
│   ├── image.rs         # Format detection (magic bytes), decompressing reader
│   ├── writer.rs        # Write engine: read → decompress → write → sync → verify
│   ├── download.rs      # HTTP/HTTPS streaming download with progress tracking
│   ├── verifier.rs      # Hash-based and memory-mapped verification
│   ├── hasher.rs        # Multi-algorithm hashing with progress
│   ├── progress.rs      # Thread-safe atomic progress tracking
│   ├── format.rs        # Device formatting (mkfs/diskutil/format.exe)
│   ├── plugin.rs        # FormatPlugin trait and PluginRegistry
│   ├── bench.rs         # I/O benchmarking suite
│   ├── nbd.rs           # Network Block Device client
│   ├── diff.rs          # Differential/incremental writes
│   ├── uring.rs         # io_uring async I/O with fallback
│   ├── zerocopy.rs      # Zero-copy splice/sendfile transfers
│   ├── vmdk.rs          # VMDK sparse extent reader
│   ├── wim.rs           # WIM header and XML metadata parser
│   ├── error.rs         # Error types (thiserror)
│   ├── parallel_decompress.rs  # Multi-threaded decompression pipeline
│   ├── multicast.rs     # UDP multicast imaging sender/receiver
│   ├── multiboot.rs     # Ventoy-style multi-boot registry and GRUB config
│   ├── i18n.rs          # Localization — 12 locales, message catalogs
│   └── a11y.rs          # Accessibility — ARIA roles, WCAG contrast, announcements
├── platform/
│   ├── linux.rs         # sysfs + lsblk enumeration
│   ├── macos.rs         # diskutil enumeration
│   ├── windows.rs       # PowerShell Get-Disk enumeration
│   ├── freebsd.rs       # sysctl + geom enumeration
│   └── stub.rs          # Fallback for unsupported platforms
├── cli/
│   ├── mod.rs           # Clap argument definitions (24 commands)
│   └── commands/        # Command implementations (write, verify, list, multiboot, etc.)
├── ontology/
│   └── mod.rs           # JSON-LD capability schema generator
├── tui/
│   └── mod.rs           # ratatui interactive TUI
└── gui/
    └── mod.rs           # egui/eframe native GUI
```

### Design Principles

1. **Safety first** — system drive protection, confirmation prompts, removable-only defaults
2. **Auto-detection** — image format from magic bytes, compression from headers (not file extensions)
3. **Streaming I/O** — decompress → write → verify in a single pass with configurable block size
4. **Platform abstraction** — trait-based device enumeration with OS-specific implementations
5. **Feature-gated UIs** — CLI always available; TUI and GUI are compile-time optional

### Performance & Reliability Engineering

The I/O engine is designed for production reliability on real hardware:

- **Inline hashing** — hash is computed during write, eliminating the need to re-read and re-decompress the source during verification (halves I/O for compressed images)
- **Buffered I/O** — `BufReader`/`BufWriter` wrapping on all file handles; `BufReader` between raw `File` and decompressors for optimal syscall batching
- **`spawn_blocking`** — all blocking file I/O dispatched off the tokio async runtime to prevent thread starvation
- **Retry with backoff** — transient I/O errors (EINTR, timeout, would-block) are retried up to 3× with exponential backoff
- **Platform-correct sync** — `O_SYNC` + `O_DIRECT` on Linux; `FILE_FLAG_WRITE_THROUGH` + `FILE_FLAG_NO_BUFFERING` on Windows; `FlushFileBuffers` for Windows sync
- **Lock-free progress** — `AtomicU8`-based operation phase tracking (no Mutex), `AtomicU64` for byte counters; fully wait-free snapshot reads
- **Trait-based hashing** — single unified read loop for all 6 hash algorithms via `DynHasher` trait (no code duplication, no per-algorithm buffer allocation)- **Memory-mapped verification** — `memmap2::Mmap` for zero-copy hash verification on regular files; automatic fallback to buffered I/O for block devices
- **Plugin system** — `FormatPlugin` trait allows registering custom image format handlers; `PluginRegistry` with priority-ordered lookup and 4 built-in plugins (QCOW2, VHD, VMDK, WIM)- **No shell injection** — Linux formatting uses `Command::new("mkfs.*").arg(device)` directly instead of `sh -c` with string interpolation
- **SHA-1 correctness** — real SHA-1 via the `sha1` crate (previous versions silently used SHA-256 for SHA-1 requests)
- **HTTP streaming download** — chunked streaming via `reqwest::bytes_stream()` with cancel support; no full-file memory buffering for multi-GB images
- **Sparse write** — all-zero blocks detected via `u64`-aligned word comparison and seeked past instead of written; halves write time for partially-empty disk images
- **Graceful shutdown** — Ctrl+C sets an atomic cancel flag checked on every block/chunk; in-flight writes complete the current block, flush, and sync before exit
- **Structured logging** — JSON-line log output to file via `--log-file` for post-mortem analysis and CI integration

## Platform Support

| Platform | Enumeration         | Write | Format     | Elevation   |
| -------- | ------------------- | ----- | ---------- | ----------- |
| Linux    | sysfs + lsblk       | ✅     | mkfs.*     | uid == 0    |
| macOS    | diskutil            | ✅     | diskutil   | uid == 0    |
| Windows  | PowerShell Get-Disk | ✅     | format.exe | Admin token |
| FreeBSD  | sysctl + geom       | ✅     | newfs      | uid == 0    |

### Device Scope

| Category                 | Examples                         | Use Cases                  |
| ------------------------ | -------------------------------- | -------------------------- |
| Microcontroller/Embedded | SPI flash, I2C EEPROM, eMMC, MTD | Firmware flashing          |
| Removable Media          | USB drives, SD cards, CF cards   | OS installation, live boot |
| Desktop/Workstation      | NVMe, SATA SSD/HDD, loopback     | Disk imaging, cloning      |
| Cloud/Server             | Virtual disks, iSCSI LUNs, NBD   | VM provisioning            |

## Compared to Prior Art

| Feature         | dd  | Etcher | Rufus | MediaWriter | Ventoy | rpi-imager | **abt** |
| --------------- | --- | ------ | ----- | ----------- | ------ | ---------- | ------- |
| CLI             | ✅   | —      | —     | —           | ✅      | —          | ✅       |
| TUI             | —   | —      | —     | —           | —      | —          | ✅       |
| GUI             | —   | ✅      | ✅     | ✅           | ✅      | ✅          | ✅       |
| Cross-platform  | ✅   | ✅      | ❌     | ✅           | ✅      | ✅          | ✅       |
| Auto-decompress | —   | ✅      | ✅     | ✅           | —      | ✅          | ✅       |
| Verification    | —   | ✅      | —     | —           | —      | ✅          | ✅       |
| URL download    | —   | ✅      | —     | ✅           | —      | ✅          | ✅       |
| Sparse write    | ✅   | —      | —     | —           | —      | —          | ✅       |
| AI ontology     | —   | —      | —     | —           | —      | —          | ✅       |
| Agentic safety  | —   | —      | —     | —           | —      | —          | ✅       |
| Dry-run mode    | —   | —      | —     | —           | —      | —          | ✅       |
| No Electron     | ✅   | ❌      | ✅     | ✅           | ✅      | ✅          | ✅       |
| Single binary   | ✅   | ❌      | ✅     | ❌           | ❌      | ❌          | ✅       |
| Memory safe     | —   | ✅      | —     | ✅           | —      | ✅          | ✅       |
| Device cloning  | —   | —      | ✅     | —           | —      | —          | ✅       |
| Secure erase    | —   | —      | ✅     | —           | —      | —          | ✅       |
| Boot validation | —   | —      | —     | —           | —      | —          | ✅       |
| NBD source      | —   | —      | —     | —           | —      | —          | ✅       |
| Differential    | —   | —      | —     | —           | —      | —          | ✅       |
| Benchmarking    | —   | —      | —     | —           | —      | —          | ✅       |
| MCP/AI server   | —   | —      | —     | —           | —      | —          | ✅       |
| Multicast       | —   | —      | —     | —           | —      | —          | ✅       |
| Multi-boot      | —   | —      | —     | —           | ✅      | —          | ✅       |
| i18n            | —   | —      | —     | —           | ✅      | —          | ✅       |
| Accessibility   | —   | —      | —     | —           | —      | —          | ✅       |

## License

Licensed under the [GNU Affero General Public License v3.0 (AGPL-3.0)](https://www.gnu.org/licenses/agpl-3.0.html). Commercial licenses are available — contact [NERVOSYS, LLC](https://nervosys.com) for details.

Copyright 2026 (c) NERVOSYS, LLC
