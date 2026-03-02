# Security

`abt` includes a comprehensive security audit module that goes beyond the pre-flight safety system.

## Security Audit Module

The `core::security` module validates all inputs for:

### Path Traversal Prevention (SEC-001 through SEC-007)
- Blocks `..` components in file paths
- Detects null bytes that could truncate paths in C-based syscalls
- Flags control characters and excessively long paths
- Windows reserved device name detection (CON, PRN, NUL, COM1-9, LPT1-9)
- Path containment validation (prevents escaping base directories)

### Symlink Protection (SEC-010 through SEC-012)
- Detects symbolic links in source and target paths
- Flags absolute symlink targets that could escape containment
- Reports unresolvable (broken) symlinks

### Device Path Validation (SEC-020 through SEC-022)
- Unix: verifies device paths are under `/dev/`
- Windows: validates `\\.\PhysicalDriveN` pattern
- Blocks shell metacharacters (`|`, `;`, `&`, `$`, `` ` ``, etc.)

### Privilege Audit (SEC-030 through SEC-032)
- Reports running as root/SYSTEM/Administrator
- Detects SUID bits (real UID ≠ effective UID)
- Flags suspicious environment variables (`LD_PRELOAD`, `DYLD_INSERT_LIBRARIES`)

### URL Validation (SEC-040 through SEC-042)
- Blocks `file://` scheme (local file access bypass)
- Warns on embedded credentials in URLs
- Flags internal/private network addresses (SSRF prevention)

### TOCTOU Race Detection (SEC-050 through SEC-054)
- `FileSnapshot` captures metadata (size, mtime, symlink status, inode/device)
- Verify before use to detect file swaps between check and write
- Detects file deletion between check and use

### Hash Integrity (SEC-060 through SEC-063)
- Validates hash format (`algorithm:hex`)
- Checks algorithm support and hex character validity
- Verifies hash length matches expected algorithm output

## Running a Security Audit

```bash
# Via the safety pre-flight system
abt write -i image.iso -o /dev/sdb --dry-run --safety-level paranoid
```

The security module is automatically invoked during pre-flight checks at `cautious` and `paranoid` safety levels.
