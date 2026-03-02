# Architecture

## Module Organization

```
src/
├── main.rs                     # Entry point, command dispatch, signal handling
├── lib.rs                      # Library root with feature gates
├── core/                       # 38 core modules
│   ├── types.rs                # ImageFormat, DeviceType, HashAlgorithm, WriteConfig
│   ├── device.rs               # DeviceInfo, DeviceEnumerator trait
│   ├── safety.rs               # Pre-flight checks, SafetyLevel, DeviceFingerprint
│   ├── security.rs             # Path traversal, symlink, privilege audit, TOCTOU
│   ├── image.rs                # Format detection, decompressing reader
│   ├── writer.rs               # Write engine: decompress → write → sync → verify
│   ├── download.rs             # HTTP/HTTPS streaming download
│   ├── verifier.rs             # Hash-based and memory-mapped verification
│   ├── hasher.rs               # Multi-algorithm hashing (6 algorithms)
│   ├── progress.rs             # Lock-free atomic progress tracking
│   ├── format.rs               # Device formatting (mkfs/diskutil/format.exe)
│   ├── plugin.rs               # FormatPlugin trait and PluginRegistry
│   ├── config.rs               # TOML config file support
│   ├── error.rs                # Error types (21 variants)
│   ├── partition.rs            # GPT/MBR partition table parsing
│   ├── iso9660.rs              # ISO 9660 / El Torito metadata
│   ├── blocksize.rs            # Adaptive block size auto-tuning
│   ├── notify.rs               # Desktop notifications
│   ├── loopback.rs             # Loopback device testing
│   ├── resume.rs               # Error recovery / checkpoint resume
│   ├── qcow2.rs                # QCOW2 v2/v3 reader
│   ├── vhd.rs                  # VHD/VHDX reader
│   ├── vmdk.rs                 # VMDK sparse extent reader
│   ├── wim.rs                  # WIM parser
│   ├── bench.rs                # I/O benchmarking suite
│   ├── nbd.rs                  # Network Block Device client
│   ├── diff.rs                 # Differential/incremental writes
│   ├── clone.rs                # Device cloning
│   ├── erase.rs                # Secure erase (6 methods)
│   ├── boot.rs                 # Boot sector validation
│   ├── rpicatalog.rs           # Raspberry Pi OS catalog
│   ├── uring.rs                # io_uring async I/O
│   ├── zerocopy.rs             # splice/sendfile zero-copy
│   ├── parallel_decompress.rs  # Multi-threaded decompression
│   ├── multicast.rs            # UDP multicast imaging
│   ├── multiboot.rs            # Multi-boot USB (Ventoy-style)
│   ├── i18n.rs                 # Localization (12 locales)
│   └── a11y.rs                 # Accessibility (WCAG 2.1 AA)
├── platform/                   # OS-specific implementations
│   ├── linux.rs                # sysfs + lsblk
│   ├── macos.rs                # diskutil
│   ├── windows.rs              # PowerShell Get-Disk
│   ├── freebsd.rs              # sysctl + geom
│   └── stub.rs                 # Fallback
├── cli/
│   ├── mod.rs                  # Clap definitions (19 commands)
│   └── commands/               # Command implementations
├── ontology/
│   └── mod.rs                  # JSON-LD / OpenAPI schema generator
├── mcp/
│   └── mod.rs                  # MCP server (JSON-RPC 2.0)
├── tui/
│   └── mod.rs                  # ratatui TUI
└── gui/
    └── mod.rs                  # egui/eframe GUI
```

## Design Principles

1. **Safety first** — system drive protection, confirmation prompts, removable-only defaults
2. **Auto-detection** — image format from magic bytes, compression from headers (not extensions)
3. **Streaming I/O** — decompress → write → verify in a single pass
4. **Platform abstraction** — trait-based device enumeration with OS-specific implementations
5. **Feature-gated UIs** — CLI always available; TUI and GUI are compile-time optional
6. **Lock-free concurrency** — AtomicU8/AtomicU64 for progress, no Mutex in hot path
7. **Security in depth** — path validation, symlink protection, TOCTOU prevention
