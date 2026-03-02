# Quick Start

## Write an Image

```bash
# Auto-detects format, decompresses, writes, and verifies
abt write -i ubuntu-24.04.iso -o /dev/sdb

# Windows
abt write -i image.img -o \\.\PhysicalDrive1

# From a URL (streamed download → write)
abt write -i https://releases.ubuntu.com/24.04/ubuntu-24.04-desktop-amd64.iso -o /dev/sdb

# Sparse write (skip zero blocks for faster writes)
abt write -i image.raw -o /dev/sdb --sparse
```

## List Devices

```bash
abt list                 # removable devices
abt list --all           # all including system drives
abt list --type usb      # filter by type
abt list --json          # JSON output with fingerprints
```

## Verify

```bash
abt verify -i ubuntu-24.04.iso -o /dev/sdb
abt verify -o /dev/sdb --expected-hash sha256:abc123...
```

## Checksum

```bash
abt checksum image.iso
abt checksum image.iso -a sha256 -a blake3 -a md5
```

## Safety: Dry-Run

```bash
# See exactly what will happen without writing anything
abt write -i ubuntu.iso -o /dev/sdb --dry-run

# Agent-safe write with device token
abt write -i image.iso -o /dev/sdb --safety-level cautious --confirm-token <token>
```

## Interactive Modes

```bash
# Terminal UI
abt tui

# Graphical UI
abt gui
```

## AI Integration

```bash
# Export ontology for AI agents
abt ontology --full

# Start MCP server
abt mcp
```
