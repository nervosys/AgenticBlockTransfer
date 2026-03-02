# Changelog

All notable changes to AgenticBlockTransfer (abt) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.6.0] - 2026-06-15

### Added

#### Core
- FFU (Full Flash Update) image parser: security header, image header with manifest extraction, store header with block data entries and disk layout mapping, FfuReader streaming Read implementation for raw disk data access, signature validation, block size alignment
- ISOHybrid detection: MBR partition table analysis for embedded boot code (Isolinux, GRUB2, generic MBR), GPT hybrid identification via EFI PART signature, write mode recommendation (raw dd vs. file copy), El Torito boot catalog cross-check, Windows ISO volume ID heuristics
- Process lock detection: cross-platform scanning for processes holding handles on target drives --- Linux /proc/fd + /proc/mounts enumeration, macOS lsof -F parsing, Windows wmic/PowerShell handle queries --- with configurable safe/critical process lists and timeout support
- Privilege elevation: cross-platform re-launch with elevated privileges --- Windows UAC via PowerShell Start-Process -Verb RunAs, Linux pkexec/sudo, macOS osascript with administrator privileges --- status reporting, method detection, and elevation result tracking
- Optical disc reader: CD/DVD/Blu-ray media reading with sector-level access, ISO 9660 PVD parsing for volume label and size, configurable buffer sizes, retry-on-error with zero-fill option, SHA-256 output verification, platform-specific drive enumeration

#### CLI
- `abt ffu` --- parse FFU image metadata (info), detect FFU signatures (detect), extract XML/JSON manifests (manifest)
- `abt isohybrid` --- analyze ISO images for MBR/GPT hybrid boot support (detect), recommend optimal write mode (mode)
- `abt proclock` --- scan for process locks on drives (scan), check busy status with exit codes (busy), generate human-readable lock reports (report)
- `abt elevate` --- check current privilege status (status), attempt privilege elevation (run), list available elevation methods (methods)
- `abt optical` --- enumerate optical drives (list), query disc metadata (info), read disc media to ISO file with progress and verification (read)

## [1.5.0] - 2026-06-15

### Added

#### Core
- Filesystem detection from superblock magic bytes: FAT12/16/32, exFAT, NTFS, ReFS, ext2/3/4, XFS, Btrfs, UDF, ISO9660, HFS+, APFS, Linux swap with confidence scores and metadata extraction (OEM, label, volume ID, block size, total size)
- Asynchronous drive scanner with hot-plug detection via state-diffing, DeviceAdapter trait for extensible device sources (BlockDevice, Usbboot), configurable polling, tokio broadcast event channels, and scan snapshots
- Drive constraint validation engine with system drive protection, source drive overlap detection, size compatibility checks, read-only blocking, mounted drive warnings, large-drive-ratio alerts, and automatic best-drive selection
- Windows To Go drive preparation with ISO analysis (bootmgr/EFI/install.wim detection, edition/architecture heuristics), GPT/MBR partition planning (ESP/MSR/Windows/Recovery), SAN policy generation, and drive attribute checking
- Syslinux/GRUB bootloader detection and installation planning: 8 bootloader types (Syslinux v4/v6, ISOLINUX, EXTLINUX, GRUB2, GRUB4DOS, Windows Boot Manager, ReactOS FreeLoader), binary version parsing, filesystem compatibility validation, syslinux.cfg generation, and boot record action planning

#### CLI
- `abt fs-detect` --- detect filesystem type on a device or image with probe and full-detect modes
- `abt drive-scan` --- scan for attached drives, watch for hot-plug events, or take device snapshots
- `abt drive-constraints` --- validate drive compatibility, auto-select the best target drive, or check all drives
- `abt wintogo` --- analyze Windows ISOs for WinToGo compatibility, plan partitions, check drive attributes, and generate SAN policy
- `abt syslinux` --- detect bootloaders in file listings, parse Syslinux versions from binaries, plan bootloader installations, generate syslinux.cfg, and list supported bootloader types

## [1.4.0] - 2026-06-15

### Added

#### Core
- Drive restore to factory state with GPT/MBR partition table management, filesystem formatting (ExFAT/FAT32/NTFS/ext4/Btrfs/XFS), sector wiping, and platform-specific command generation
- Performance telemetry with bottleneck detection (network/decompression/storage/verifying/hashing), per-phase throughput tracking, session recording, and JSON export/import
- Write watchdog with stall detection, configurable thresholds (default/lenient/strict presets), automatic queue depth reduction, sync I/O fallback, and escalation chains
- WIM file extraction with header parsing, image enumeration, glob-based file filtering, Windows edition/build/architecture detection, and compression type identification (XPRESS/LZX/LZMS)
- Secure Boot detection with EFI variable reading (Linux /sys/firmware/efi/efivars), firmware mode detection (UEFI/Legacy BIOS), key database enumeration (PK/KEK/db/dbx/MOK), PE/COFF Authenticode signature detection, and known bootloader identification

#### CLI
- `abt restore` --- plan and execute drive restoration with --force guard, partition table selection, filesystem choice, and volume labeling
- `abt telemetry` --- view, demo, and export performance telemetry reports with bottleneck analysis
- `abt watchdog` --- display watchdog configuration presets and simulate stall detection scenarios with escalation chains
- `abt wim-extract` --- inspect WIM headers, list files, extract with glob filters, and validate WIM magic bytes
- `abt secureboot` --- check firmware Secure Boot status, verify bootloader signatures, and list known signed bootloaders

### Fixed
- WIM glob matching now correctly stops at path separators (`*` does not cross directory boundaries)
- Secure Boot PE signature detection handles files shorter than DOS header (64 bytes) without panicking

## [1.3.0] - 2026-06-15

### Added

#### Core
- Proxy configuration with auto-detection (HTTP/HTTPS/SOCKS5), no-proxy lists, and per-profile fetch settings
- RSA signature verification with PEM/DER key parsing, detached .sig/.asc support, and keyring management
- Windows Unattended Setup (WUE) generator with TPM/SecureBoot/RAM/storage bypasses, OOBE customization, and auto-logon
- Generic pluggable OS catalog system with provider registry, hardware tag filtering, cache persistence, and rpi-imager JSON conversion
- UEFI:NTFS dual-partition layout planner with FAT32 file-size analysis, ESP generation, and Windows To Go support
- Fleet (multi-target) write session manager with concurrent device tracking, progress snapshots, and cancellation

#### CLI
- `abt signature` — verify detached signatures, manage keyring, hash files
- `abt wue` — generate Windows unattend.xml with hardware bypasses, accounts, locale, product key
- `abt uefi-ntfs` — analyze directories for FAT32 compatibility, plan UEFI:NTFS layouts
- `abt fleet` — detect USB devices, validate multi-target configs, show fleet status

## [1.2.0] - 2026-06-15

### Added

#### Core
- Resumable HTTP downloads with .part file persistence, ETag/Last-Modified validation
- Mirror selection with latency probing, automatic failover, and metalink (RFC 5854) parsing
- Self-update version checker via GitHub Releases API with semver comparison and platform asset detection
- Checksum file parser (SHA256SUMS, MD5SUMS) with auto-detection of GNU, BSD, and simple formats
- USB speed detection with degraded-speed warnings and write-time estimates (Linux sysfs, Windows/macOS stubs)
- Large FAT32 formatter supporting drives >32 GB with auto or custom cluster sizes (up to 2 TiB)

#### CLI
- `abt update` / `upgrade` — check for updates, dismiss, JSON output
- `abt mirror` / `mirrors` — probe latency, download with failover, list mirrors
- `abt checksum-file` / `checksumfile` — parse, verify, and look up entries in checksum files
- `abt usb-info` / `usbspeed` — display USB device speed info and write-time estimates

### Changed
- reqwest now includes `json` feature for direct response deserialization
- `parse_semver()` correctly handles pre-release suffixes like `1.2.3-beta.1`

## [1.0.0] - 2026-02-28

### Added

#### Core
- Multi-format image support: ISO, IMG, RAW, DMG, VHD, VHDX, VMDK, QCOW2, WIM, FFU
- Auto-decompression: gz, bz2, xz, zstd, zip (magic byte detection)
- Write verification: byte-for-byte read-back with mismatch offset reporting
- Multi-algorithm hashing: SHA-256, SHA-512, SHA-1, MD5, BLAKE3, CRC32
- Safety system: 3 levels (normal/cautious/paranoid), device fingerprints, 10 exit codes
- HTTP/HTTPS streaming download source
- Sparse write optimization (zero-block skipping)
- Signal handling with graceful Ctrl+C shutdown
- Error recovery with JSON checkpoint resume
- Device formatting: ext2/3/4, FAT16/32, exFAT, NTFS, APFS, HFS+, btrfs, XFS, F2FS
- Plugin/extension system for custom image format handlers
- Config file support (~/.config/abt/config.toml)
- Structured JSON file logging

#### Performance
- Direct I/O (O_DIRECT / FILE_FLAG_NO_BUFFERING)
- Async I/O via io_uring on Linux 5.1+
- Zero-copy transfers via splice(2) / sendfile(2)
- Adaptive block size auto-tuning
- Memory-mapped verification via memmap2
- Parallel decompression pipeline (pigz/pbzip2-style)
- I/O benchmarking suite (`abt bench`)

#### Platform Support
- Linux: sysfs + lsblk enumeration
- macOS: diskutil enumeration
- Windows: PowerShell Get-Disk enumeration
- FreeBSD: sysctl + geom enumeration

#### CLI (19 commands)
- `write` / `flash` / `dd` — image write pipeline
- `verify` — post-write verification
- `list` / `devices` — device enumeration
- `info` / `inspect` — device and image inspection
- `checksum` / `hash` — multi-algorithm checksums
- `format` / `mkfs` — device formatting
- `ontology` / `schema` — AI capability ontology
- `completions` — shell completions (bash/zsh/fish/pwsh)
- `man` — man page generation
- `tui` — terminal UI
- `gui` — graphical UI
- `mcp` — Model Context Protocol server
- `clone` — device-to-device block copy
- `erase` — secure erase (6 methods)
- `boot` — boot sector validation
- `catalog` — Raspberry Pi OS catalog browser
- `bench` — I/O benchmarking
- `diff` — differential/incremental writes
- `multiboot` / `ventoy` — multi-boot USB management

#### TUI
- Interactive terminal UI with ratatui
- File browser with directory navigation and extension filtering
- Device table with selection
- Write confirmation with safety report
- Real-time progress gauge with speed/ETA

#### GUI
- Native GUI with egui/eframe
- 3-step wizard (Source → Device → Write)
- 6 theme presets (Dark/Light/Nord/Solarized/Dracula/Monokai)
- Drag-and-drop with hover overlay
- Native file dialog (rfd)
- Desktop notifications (notify-rust)

#### AI Integration
- JSON-LD ontology with schema.org vocabulary
- OpenAPI 3.1 schema generation
- YAML ontology output
- MCP server mode (JSON-RPC 2.0 over stdio, 6 tools)

#### Networking
- Network Block Device (NBD) source support
- UDP multicast imaging (sender/receiver)
- Ventoy-style multi-boot with GRUB2 config generation

#### Accessibility & Localization
- 16 ARIA-like roles for screen reader support
- WCAG 2.1 AA high-contrast palette
- Announcement queue with priority levels
- Keyboard-only navigation mode
- 12 locale support with 4 built-in message catalogs (en/de/fr/es)
- Runtime locale detection from environment variables

#### Security
- Path traversal prevention
- Symlink attack detection
- Device path injection blocking
- URL validation (SSRF prevention, credential detection)
- TOCTOU race detection via FileSnapshot
- Privilege audit (SUID, LD_PRELOAD detection)
- Hash integrity validation

#### Distribution
- Homebrew formula
- Winget manifest
- AUR PKGBUILD
- Debian packaging (control + rules)
- RPM spec
- GitHub Actions release workflow with GPG signing and attestation
- crates.io publishing pipeline

#### Documentation
- mdbook documentation (16 chapters)
- Man page generation
- Shell completion generation

### Security
- SEC-001 through SEC-063: Comprehensive security finding IDs
- TOCTOU race prevention with metadata snapshots
- Path containment validation
- Shell metacharacter blocking in device paths

## [0.1.0] - Initial development

- Foundation implementation (not released)
