# Safety System

`abt` is designed from the ground up to prevent accidental data loss — the #1 risk with disk imaging tools.

## Pre-flight Safety Checks

Every write operation runs a structured analysis before any bytes are written:

1. **Source exists and is readable**
2. **Image format detected** (magic bytes, not extension)
3. **Target device exists** (OS device enumeration)
4. **Not a system drive** (blocks writes to boot/OS drives)
5. **Not read-only** (detects write-protect switches)
6. **Removable media check** (warns on non-removable targets)
7. **No mounted filesystems** (detects in-use partitions)
8. **Image fits on device** (size validation)
9. **Self-write protection** (refuses if source is on target)
10. **Privilege check** (warns if not elevated)

## Safety Levels

| Level      | Default for  | Behavior                                                       |
| ---------- | ------------ | -------------------------------------------------------------- |
| `normal`   | Humans       | Blocks system drives, interactive y/N confirmation             |
| `cautious` | Agents       | + validates image size, warns on non-removable, requires token |
| `paranoid` | Critical ops | + requires `--confirm-token`, backs up partition table         |

## Device Fingerprints (TOCTOU Prevention)

```bash
# Step 1: Enumerate devices, get fingerprints
abt list --json
# → { "devices": [{ "path": "/dev/sdb", "confirm_token": "a1b2c3d4..." }] }

# Step 2: Write using token — verifies device hasn't changed
abt write -i image.iso -o /dev/sdb --confirm-token a1b2c3d4...
```

## Structured Exit Codes

| Code | Meaning                         |
| ---- | ------------------------------- |
| 0    | Success                         |
| 1    | General error                   |
| 2    | Safety check failed             |
| 3    | Verification failed             |
| 4    | Permission denied               |
| 5    | Source not found                |
| 6    | Target not found / read-only    |
| 7    | Image too large                 |
| 8    | Device changed (token mismatch) |
| 130  | Cancelled (Ctrl+C)              |

## Security Audit

`abt` includes a security audit module (`core::security`) that validates:

- **Path traversal** — blocks `..` components, null bytes, control characters
- **Symlink attacks** — detects and reports symbolic links in source/target paths
- **Device path injection** — blocks shell metacharacters in device paths
- **URL validation** — blocks `file://` scheme, warns on credentials in URLs, flags internal networks
- **TOCTOU races** — `FileSnapshot` captures metadata and verifies before use
- **Privilege audit** — reports SUID bits, suspicious environment variables (LD_PRELOAD, etc.)
