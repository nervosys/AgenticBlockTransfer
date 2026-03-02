# ROADMAP

Implementation progress for AgenticBlockTransfer (`abt`).

Legend: ✅ Done | 🔧 Partial | 🔲 Not started

## v0.1.0 — Foundation (current)

### Core Library

| Component                | Status | Notes                                                                                                                        |
| ------------------------ | ------ | ---------------------------------------------------------------------------------------------------------------------------- |
| Error types (`AbtError`) | ✅      | thiserror-derived, 21 variants (6 new: Timeout, CancelledByUser, BackupFailed, TokenMismatch, RetryExhausted, DeviceChanged) |
| Type definitions         | ✅      | ImageFormat (15), DeviceType (12), Filesystem (10), HashAlgorithm (6)                                                        |
| Image format detection   | ✅      | Magic byte detection + extension fallback, BufReader-wrapped                                                                 |
| Decompressing reader     | ✅      | gz, bz2, xz, zstd, zip — streaming with BufReader wrapping                                                                   |
| Write engine             | ✅      | BufWriter, inline hashing, retry w/ backoff, spawn_blocking, O_DIRECT + FILE_FLAG_NO_BUFFERING                               |
| Verification engine      | ✅      | Hash-based (no re-decompression), inline hash comparison                                                                     |
| Hasher                   | ✅      | SHA-256, SHA-512, SHA-1, MD5, BLAKE3, CRC32 — trait-based dedup                                                              |
| Progress tracking        | ✅      | Fully lock-free AtomicU8/AtomicU64, no Mutex                                                                                 |
| Device formatting        | ✅      | Platform-dispatched, no shell injection, input validation                                                                    |
| Device abstraction       | ✅      | `DeviceEnumerator` trait, `DeviceInfo` struct                                                                                |
| Safety system            | ✅      | Pre-flight checks, dry-run, fingerprints, 10 exit codes, partition backup                                                    |
| HTTP download source     | ✅      | reqwest streaming download → write pipeline with progress                                                                    |
| Signal handling          | ✅      | Graceful Ctrl+C with device sync                                                                                             |
| Shell completions        | ✅      | bash, zsh, fish, PowerShell via clap_complete                                                                                |
| Sparse write             | ✅      | Zero-block skipping with lseek/SetFilePointer                                                                                |

### Platform Support

| Platform             | Enumeration         | Write | Format     | Elevation   | Status |
| -------------------- | ------------------- | ----- | ---------- | ----------- | ------ |
| Linux                | sysfs + lsblk       | ✅     | mkfs.*     | uid check   | ✅      |
| macOS                | diskutil            | ✅     | diskutil   | uid check   | ✅      |
| Windows              | PowerShell Get-Disk | ✅     | format.exe | Admin token | ✅      |
| FreeBSD / other UNIX | sysctl + geom       | ✅     | newfs      | uid check   | ✅      |

### CLI

| Feature                      | Status | Notes                                                                 |
| ---------------------------- | ------ | --------------------------------------------------------------------- |
| Argument parsing (clap)      | ✅      | 18 commands, aliases, global flags                                    |
| `write` command              | ✅      | Full pipeline: decompress → write → sync → verify                     |
| `verify` command             | ✅      | Source comparison + expected hash                                     |
| `list` command               | ✅      | Tabular output, --all / --removable / --type filters                  |
| `info` command               | ✅      | Device + image inspection                                             |
| `checksum` command           | ✅      | Multi-algorithm with progress bar                                     |
| `format` command             | ✅      | Platform-dispatched                                                   |
| `ontology` command           | ✅      | JSON-LD and JSON output                                               |
| `tui` command                | ✅      | Launches TUI mode                                                     |
| `gui` command                | ✅      | Launches GUI mode                                                     |
| JSON output mode (`-o json`) | ✅      | Structured output on write (safety report + result) and list commands |
| Shell completions            | ✅      | bash, zsh, fish, PowerShell via `completions` command                 |

### TUI

| Feature             | Status | Notes                                                                      |
| ------------------- | ------ | -------------------------------------------------------------------------- |
| Source selection    | ✅      | Text input for image path                                                  |
| Device listing      | ✅      | Table with selection                                                       |
| Write confirmation  | ✅      | Safety prompt before write                                                 |
| Progress gauge      | ✅      | Real-time bytes/speed/ETA                                                  |
| Error display       | ✅      | Dedicated error state                                                      |
| Keyboard navigation | ✅      | Up/Down/Enter/Esc/q                                                        |
| File browser        | ✅      | In-TUI file picker with directory navigation, extension filtering, Tab key |

### GUI

| Feature            | Status | Notes                                                                       |
| ------------------ | ------ | --------------------------------------------------------------------------- |
| 3-step wizard      | ✅      | Source → Device → Write                                                     |
| Device list        | ✅      | Selectable with system drive filter                                         |
| Progress bar       | ✅      | Animated with speed/ETA                                                     |
| Menu bar           | ✅      | File / View / Help                                                          |
| Dark/light mode    | ✅      | Toggle via View menu                                                        |
| Native file dialog | ✅      | rfd crate — Browse and Open Image dialogs                                   |
| Device refresh     | ✅      | Synchronous re-enumeration via runtime handle                               |
| Drag-and-drop      | ✅      | eframe egui with hover overlay + extension filtering                        |
| Theme system       | ✅      | 6 presets (Dark/Light/Nord/Solarized/Dracula/Monokai), View > Theme submenu |

### AI Ontology

| Feature                  | Status | Notes                                                                       |
| ------------------------ | ------ | --------------------------------------------------------------------------- |
| JSON-LD output           | ✅      | Full schema.org vocabulary                                                  |
| 7 capability definitions | ✅      | Parameters, types, constraints, examples                                    |
| Type definitions         | ✅      | ImageFormat, Compression, DeviceType, Filesystem, Hash                      |
| Platform support matrix  | ✅      | Per-OS details                                                              |
| Device scope categories  | ✅      | 4 categories with examples                                                  |
| Exit code semantics      | ✅      | Per-capability exit codes                                                   |
| JSON output mode         | ✅      | Compact JSON alternative                                                    |
| YAML output              | ✅      | serde_yaml serialization via `abt ontology -f yaml`                         |
| MCP/Tool-use schema      | ✅      | Full MCP server with JSON-RPC 2.0 over stdio, 6 tools                       |
| OpenAPI-style schema     | ✅      | OpenAPI 3.1 spec with 9 endpoints, 12 schemas via `abt ontology -f openapi` |

---

## v0.2.0 — Reliability & Testing

| Item                         | Status | Notes                                                                                                                 |
| ---------------------------- | ------ | --------------------------------------------------------------------------------------------------------------------- |
| Unit tests for core library  | ✅      | 109 tests: image detection, hasher, progress, device, types, download, partition, config, ISO 9660, blocksize, notify |
| Integration tests            | ✅      | 17 integration tests: compression round-trips, partition parsing, config, verifier, progress                          |
| CI pipeline (GitHub Actions) | ✅      | Linux + macOS + Windows matrix, clippy, fmt, feature combinations, MSRV                                               |
| Loopback device testing      | ✅      | LoopbackDevice struct, create_test_image, create_compressed_test_image, 10 tests                                      |
| Error recovery               | ✅      | WriteCheckpoint with JSON persistence, verify_written_region, try_resume(), 8 tests                                   |
| Signal handling              | ✅      | Graceful Ctrl+C with progress cancel + sync                                                                           |
| Logging improvements         | ✅      | Structured logging with file output via `--log-file`                                                                  |

## v0.3.0 — Performance

| Item                         | Status | Notes                                                                             |
| ---------------------------- | ------ | --------------------------------------------------------------------------------- |
| Direct I/O (O_DIRECT)        | ✅      | Linux O_DIRECT + Windows FILE_FLAG_NO_BUFFERING via `--direct-io`                 |
| Async I/O (io_uring)         | ✅      | Linux kernel 5.1+ with graceful fallback                                          |
| Parallel hashing             | ✅      | Inline hash during write (no second pass)                                         |
| Memory-mapped I/O            | ✅      | memmap2-based verification with fallback to standard I/O                          |
| Adaptive block size          | ✅      | Benchmark-based auto-tune with diminishing-returns detection + heuristic fallback |
| Zero-copy splice/sendfile    | ✅      | splice (Linux), sendfile (macOS/FreeBSD), fallback (Windows)                      |
| Multi-threaded decompression | ✅      | pigz/pbzip2-style channel-based pipeline, parallel bz2/zstd, read-ahead gz/xz     |
| Benchmarking suite           | ✅      | `abt bench` — block-size sweep, read/write throughput, IOPS, JSON                 |
| Sparse write optimization    | ✅      | Skip all-zero blocks via lseek SEEK_CUR                                           |
| Retry with backoff           | ✅      | 3× retry on transient I/O errors                                                  |
| BufReader/BufWriter          | ✅      | All file I/O buffered, decompressors wrapped                                      |

## v0.4.0 — Extended Format Support

| Item                      | Status | Notes                                                                                         |
| ------------------------- | ------ | --------------------------------------------------------------------------------------------- |
| QCOW2 reading             | ✅      | Header parsing (v2/v3), L1→L2→cluster chain, streaming Read impl, 8 tests                     |
| VHD/VHDX reading          | ✅      | VHD footer/dynamic header/BAT, Fixed+Dynamic readers, VHDX identifier+header parsing, 8 tests |
| VMDK reading              | ✅      | Sparse extent header, grain directory/table chain, streaming Read, 8 tests                    |
| WIM extraction            | ✅      | Header parsing, flags, compression, GUID, XML metadata, 8 tests                               |
| Partition table parsing   | ✅      | GPT/MBR parsing with type lookups, mixed-endian GUID, UTF-16LE names                          |
| ISO 9660 metadata         | ✅      | PVD parsing, El Torito boot detection, Joliet, volume label, dates                            |
| Sparse write optimization | ✅      | Skip zero blocks (done in v0.1.0)                                                             |

## v0.5.0 — Ecosystem

| Item                                   | Status | Notes                                                                       |
| -------------------------------------- | ------ | --------------------------------------------------------------------------- |
| Shell completions (bash/zsh/fish/pwsh) | ✅      | `abt completions <shell>` via clap_complete                                 |
| Man page generation                    | ✅      | `abt man` generates roff pages for all commands via clap_mangen             |
| Native file dialog (GUI)               | ✅      | rfd crate — Browse, Open Image, filter by disk image extensions             |
| Drag-and-drop (GUI)                    | ✅      | eframe egui with hover overlay + extension filtering                        |
| URL/HTTP download source               | ✅      | Streaming download → decompress → write pipeline                            |
| Progress notification (OS)             | ✅      | notify-rust — toast on write success/failure, verify pass/fail              |
| Config file (~/.config/abt)            | ✅      | TOML config: write, safety, output, logging sections with defaults          |
| Plugin/extension system                | ✅      | FormatPlugin trait, PluginRegistry, 4 built-in plugins, custom registration |

## v1.0.0 — Production Release

| Item                         | Status | Notes                                                                                    |
| ---------------------------- | ------ | ---------------------------------------------------------------------------------------- |
| Stable API guarantee         | ✅      | semver commitment, version 1.0.0, CHANGELOG.md                                           |
| Security audit               | ✅      | 8 categories, 20+ checks (SEC-001-SEC-063), path/symlink/privilege/TOCTOU/URL/hash audit |
| Signed releases              | ✅      | GPG-signed binaries, SHA-256 checksums, GitHub artifact attestation                      |
| Package manager distribution | ✅      | Homebrew, AUR, winget, deb, rpm packaging configs                                        |
| Localization / i18n          | ✅      | 12 locales, 4 built-in catalogs (en/de/fr/es), format args, detect_system_locale         |
| Accessibility                | ✅      | 16 ARIA roles, WCAG 2.1 AA contrast, announcement queue, keyboard-only mode              |
| Comprehensive documentation  | ✅      | mdbook with 16 chapters: User Guide, Interfaces, AI Integration, Advanced, Development   |
| MCP server mode              | ✅      | JSON-RPC 2.0 over stdio, 6 tools, `abt mcp` command                                      |

## Future / Research

| Item                             | Notes                                                                        |
| -------------------------------- | ---------------------------------------------------------------------------- |
| Device cloning (device → device) | ✅ `abt clone` — block-level clone with inline hashing, sparse, verification  |
| Network block device source      | ✅ `abt` supports nbd:// URLs as image source, NBD protocol client            |
| Multicast imaging                | ✅ `abt` multicast sender/receiver, CRC32 per-chunk, session ID, NAK recovery |
| Differential/incremental writes  | ✅ `abt diff` — block-level comparison, skip identical, dry-run, verify       |
| Secure erase                     | ✅ `abt erase` — 6 methods: auto/zero/random/ATA/NVMe/discard, multi-pass     |
| Boot sector validation           | ✅ `abt boot` — MBR/GPT/UEFI validation with 7 checks, JSON output            |
| Raspberry Pi OS catalog          | ✅ `abt catalog` — fetch/search/browse rpi-imager OS catalog                  |
| Ventoy-style multi-boot          | ✅ `abt multiboot` — registry, GRUB2 config, OS auto-detect, add/remove/list  |

## v1.1.0 — Feature Wave 10 (Reference Project Parity)

Inspired by studying 5 reference projects (etcher, rufus, Ventoy, MediaWriter, rpi-imager).

| Feature                   | Status | Notes                                                                                   |
| ------------------------- | ------ | --------------------------------------------------------------------------------------- |
| OS Customization          | ✅      | `abt customize` — firstrun.sh / cloud-init / network-config generation, WiFi, SSH keys  |
| Image Download Cache      | ✅      | `abt cache` — SHA-256 verified local cache, eviction policies, manifest persistence     |
| Drive Health / Bad Blocks | ✅      | `abt health` — multi-pass destructive bad block check, fake flash detection, read test  |
| Sleep Inhibitor           | ✅      | RAII guard prevents OS sleep during writes (systemd/caffeinate/SetThreadExecutionState) |
| Drive Backup              | ✅      | `abt backup` — 5 compression formats, inline SHA-256, sparse zero-skip, progress        |
| Persistent Storage        | ✅      | `abt persist` — casper/Fedora/Ventoy persistence partitions and image files             |

## v1.2.0 — Feature Wave 11 (Download Resilience & Hardware Awareness)

Gap analysis from reference projects (rufus, etcher, MediaWriter, rpi-imager, Ventoy).

| Feature                     | Status | Notes                                                                         |
| --------------------------- | ------ | ----------------------------------------------------------------------------- |
| Resumable Downloads         | ✅      | HTTP Range resume with .part/.meta.json files, ETag/Last-Modified validation  |
| Mirror Selection & Failover | ✅      | `abt mirror` — latency probing, failover, metalink (RFC 5854) parsing         |
| Self-Update Checker         | ✅      | `abt update` — GitHub Releases API, semver compare, platform asset detection  |
| Checksum File Parsing       | ✅      | `abt checksum-file` — SHA256SUMS/MD5SUMS auto-detect (GNU/BSD/simple formats) |
| USB Speed Detection         | ✅      | `abt usb-info` — USB speed enum, degraded warnings, write-time estimates      |
| Large FAT32 Formatter       | ✅      | FAT32 formatting for drives >32 GB with custom cluster sizes (up to 2 TiB)    |

## v1.3.0 — Feature Wave 12 (Security, Windows Automation & Fleet Management)

Gap analysis from reference projects (rufus, etcher, MediaWriter, rpi-imager, Ventoy).

| Feature                         | Status | Notes                                                                                     |
| ------------------------------- | ------ | ----------------------------------------------------------------------------------------- |
| Proxy Configuration             | ✅      | HTTP/HTTPS/SOCKS5 auto-detection, no-proxy lists, fetch profiles (Interactive/Background) |
| Signature Verification          | ✅      | RSA SHA-256 with PEM keyring, detached .sig/.asc, download-and-verify workflow            |
| Windows Unattended Setup (WUE)  | ✅      | `abt wue` — autounattend.xml generator, TPM/SecureBoot/RAM bypasses, OOBE, accounts       |
| Generic OS Catalog              | ✅      | Provider registry with hardware tags, cache persistence, rpi-imager JSON conversion       |
| UEFI:NTFS Dual-Partition Layout | ✅      | FAT32 file-size analysis, ESP+NTFS layout planner, Windows To Go support                  |
| Fleet (Multi-Target) Writing    | ✅      | `abt fleet` — concurrent device sessions, progress snapshots, cancellation, USB detect    |



## v1.4.0 — Feature Wave 13 (Recovery, Telemetry & Secure Boot)

Gap analysis from reference projects (rufus, etcher, MediaWriter, rpi-imager, Ventoy).

| Feature               | Status | Notes                                                                                 |
| --------------------- | ------ | ------------------------------------------------------------------------------------- |
| Drive Restore         | ✅      | Factory-state restore with GPT/MBR management, sector wiping, multi-filesystem format |
| Performance Telemetry | ✅      | Bottleneck detection, per-phase throughput, session recording, JSON export/import     |
| Write Watchdog        | ✅      | Stall detection with escalation chains, queue depth reduction, sync fallback, presets |
| WIM Extraction        | ✅      | Header parsing, image enumeration, glob filtering, edition/build/arch detection       |
| Secure Boot Detection | ✅      | EFI variable reading, firmware mode, key databases, PE Authenticode, bootloader ID    |


## v1.5.0 — Feature Wave 14 (Filesystem Detection, Drive Scanning & Bootloader Management)

Gap analysis from reference projects (rufus, etcher, MediaWriter, rpi-imager, Ventoy).

| Feature                        | Status | Notes                                                                                      |
| ------------------------------ | ------ | ------------------------------------------------------------------------------------------ |
| Filesystem Detection           | ✅      | Superblock magic detection for 17 filesystem types, confidence scores, metadata extraction |
| Drive Scanner                  | ✅      | Async hot-plug scanning, DeviceAdapter trait, tokio broadcast events, scan snapshots       |
| Drive Constraints              | ✅      | System drive protection, size checks, source overlap detection, auto-select best drive     |
| Windows To Go                  | ✅      | ISO analysis, GPT/MBR partition planning, SAN policy, drive attribute validation           |
| Syslinux/Bootloader Management | ✅      | 8 bootloader types, version parsing, fs compatibility, syslinux.cfg generation, MBR plans  |

## v1.6.0 — Feature Wave 15 (Image Formats, Security & Hardware Access)

Gap analysis from reference projects (rufus, etcher, MediaWriter, rpi-imager, Ventoy).

| Feature                        | Status | Notes                                                                                      |
| ------------------------------ | ------ | ------------------------------------------------------------------------------------------ |
| FFU Image Parser               | ✅      | Security/image/store header parsing, manifest extraction, FfuReader streaming, 9 tests     |
| ISOHybrid Detection            | ✅      | MBR/GPT hybrid analysis, Isolinux/GRUB2/GenericMBR, write mode recommendation, 11 tests   |
| Process Lock Detection         | ✅      | Cross-platform lock scanning (Linux /proc, macOS lsof, Windows wmic), 10 tests            |
| Privilege Elevation            | ✅      | UAC/pkexec/sudo/osascript re-launch, status reporting, method detection, 12 tests          |
| Optical Disc Reader            | ✅      | CD/DVD/Blu-ray reading, ISO 9660 PVD, retry/zero-fill, SHA-256 verification, 9 tests      |
