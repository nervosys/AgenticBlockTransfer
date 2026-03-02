# Commands

`abt` provides 19 commands covering disk imaging, device management, AI integration, and more.

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
| `man`         | —                        | Generate man pages                         |
| `tui`         | —                        | Launch interactive terminal UI             |
| `gui`         | —                        | Launch native graphical UI                 |
| `mcp`         | —                        | Start Model Context Protocol server        |
| `clone`       | —                        | Block-level device-to-device copy          |
| `erase`       | —                        | Securely erase a device                    |
| `boot`        | —                        | Validate boot sector                       |
| `catalog`     | —                        | Browse Raspberry Pi OS catalog             |
| `bench`       | —                        | Benchmark I/O throughput                   |
| `diff`        | —                        | Differential/incremental write             |
| `multiboot`   | `ventoy`                 | Manage multi-boot USB devices              |

## Global Flags

| Flag              | Description                             |
| ----------------- | --------------------------------------- |
| `-v`, `--verbose` | Increase log verbosity                  |
| `-q`, `--quiet`   | Suppress non-essential output           |
| `--log-file`      | Write JSON-structured logs to file      |
| `-o`, `--output`  | Output format: `text` (default), `json` |

## write

```bash
abt write -i <source> -o <device> [options]
```

| Option               | Description                              |
| -------------------- | ---------------------------------------- |
| `-i`, `--input`      | Source image path or URL                 |
| `-o`, `--output`     | Target device path                       |
| `-b`, `--block-size` | Block size (e.g., `4K`, `1M`)            |
| `--no-verify`        | Skip post-write verification             |
| `--sparse`           | Skip all-zero blocks                     |
| `--force`            | Skip confirmation prompt                 |
| `--dry-run`          | Safety analysis only, no write           |
| `--direct-io`        | Use O_DIRECT / FILE_FLAG_NO_BUFFERING    |
| `--safety-level`     | `normal`, `cautious`, `paranoid`         |
| `--confirm-token`    | Device fingerprint token (for agent use) |

## clone

```bash
abt clone -i <source-device> -o <target-device> [options]
```

Block-level device-to-device copy with inline hashing, sparse optimization, and post-clone verification.

## erase

```bash
abt erase <device> [options]
```

| Option     | Description                                        |
| ---------- | -------------------------------------------------- |
| `--method` | `auto`, `zero`, `random`, `ata`, `nvme`, `discard` |
| `--passes` | Number of overwrite passes                         |

## multiboot

```bash
abt multiboot <action> [options]
```

| Action   | Description                         |
| -------- | ----------------------------------- |
| `add`    | Add an ISO to a multi-boot USB      |
| `remove` | Remove an ISO from a multi-boot USB |
| `list`   | List ISOs on a multi-boot USB       |
| `grub`   | Generate GRUB2 configuration        |
