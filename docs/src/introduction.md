# AgenticBlockTransfer (abt)

**Agentic-first CLI successor to `dd`. Human-first GUI/TUI successor to balenaEtcher, Ventoy, Rufus, Fedora Media Writer, and rpi-imager.**

## What is abt?

`abt` is a cross-platform disk image writer that combines the power of `dd` with the safety of modern GUI tools. It's designed to be operated by both humans and AI agents.

### For Humans

- **TUI mode** — interactive terminal interface with file browser, device selection, and progress visualization
- **GUI mode** — native desktop application with drag-and-drop, theme presets, and visual feedback
- **CLI mode** — familiar command-line interface for scripting and automation

### For AI Agents

- **JSON-LD ontology** — machine-readable capability schema via `abt ontology`
- **MCP server** — Model Context Protocol server for direct AI integration via `abt mcp`
- **Structured output** — JSON output mode for all commands
- **Safety guards** — pre-flight checks, dry-run mode, device fingerprints, structured exit codes

## Key Features

- **Multi-format** — ISO, IMG, RAW, DMG, VHD, VHDX, VMDK, QCOW2, WIM, FFU
- **Auto-decompression** — gz, bz2, xz, zstd, zip (detected via magic bytes)
- **Write verification** — byte-for-byte read-back with mismatch offset reporting
- **Safety system** — system drive protection, removable-only defaults, partition backup
- **Cross-platform** — Linux, macOS, Windows, FreeBSD
- **Single binary** — no Electron, no runtime dependencies, ~10 MB

## Heritage

`abt` traces its lineage to IBM's **BLock Transfer** (BLT) — a mainframe utility for moving data between block-addressable devices. When UNIX adapted the concept, it became **Dataset Definition** (`dd`). The name **AgenticBlockTransfer** is a direct nod: *Block Transfer* from IBM's BLT, *Agentic* because the tool is built to be operated by both humans and autonomous AI systems.
