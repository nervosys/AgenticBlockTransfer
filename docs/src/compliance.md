# Compliance — NIST FIPS, CMMC 2.0, and DoD Standards

This document describes how **abt** (AgenticBlockTransfer) complies with federal
information security standards required for data transfer and storage programs
operating in U.S. Department of Defense (DoD) and federal civilian environments.

## Applicable Standards

| Standard                   | Title                                           | Relevance                          |
| -------------------------- | ----------------------------------------------- | ---------------------------------- |
| **FIPS 140-2 / 140-3**     | Security Requirements for Cryptographic Modules | Hash algorithms, HMAC, TLS         |
| **FIPS 180-4**             | Secure Hash Standard                            | SHA-256, SHA-512 validation        |
| **FIPS 186-5**             | Digital Signature Standard                      | RSA signature verification         |
| **NIST SP 800-52 Rev 2**   | Guidelines for TLS Implementations              | Download transport security        |
| **NIST SP 800-88 Rev 1**   | Guidelines for Media Sanitization               | Secure erase operations            |
| **NIST SP 800-90A Rev 1**  | DRBG for Random Number Generation               | Random-fill erase entropy          |
| **NIST SP 800-131A Rev 2** | Cryptographic Algorithm Transitions             | Algorithm deprecation (MD5, SHA-1) |
| **NIST SP 800-171 Rev 2**  | Protecting CUI                                  | Basis for CMMC Level 2             |
| **CMMC 2.0 Level 2**       | Cybersecurity Maturity Model Certification      | 110 practices (14 domains)         |
| **DoD SRG / STIG**         | Security Requirements Guides                    | Technical implementation details   |

## FIPS Mode

abt provides a **FIPS compliance mode** that restricts operations to
FIPS-approved algorithms and configurations. FIPS mode is **monotonic** — once
enabled for a process, it cannot be disabled.

### Enabling FIPS Mode

```bash
# Via CLI flag (recommended)
abt --fips write -i image.iso -o /dev/sdb

# Via environment variable
export ABT_FIPS_MODE=1
abt write -i image.iso -o /dev/sdb

# Windows
set ABT_FIPS_MODE=1
abt write -i image.iso -o \\.\PhysicalDrive1
```

### Effects of FIPS Mode

| Feature                 | Default Mode                                      | FIPS Mode                              |
| ----------------------- | ------------------------------------------------- | -------------------------------------- |
| Hash algorithms         | All (MD5, SHA-1, SHA-256, SHA-512, BLAKE3, CRC32) | SHA-256, SHA-512 only                  |
| Random erase data       | xorshift64 PRNG (fast, ~6 GiB/s)                  | OS CSPRNG via `getrandom` (SP 800-90A) |
| Device fingerprint      | BLAKE3                                            | SHA-256 (FIPS 180-4)                   |
| TLS minimum version     | TLS 1.0+ (rustls default)                         | TLS 1.2+ (SP 800-52)                   |
| HTTP downloads          | Permitted with warning                            | Rejected (HTTPS only)                  |
| Non-approved algorithms | Warning emitted                                   | Error returned                         |

## Compliance Matrix

### CMMC 2.0 Level 2 Practice Mapping

#### Access Control (AC)

| Practice    | Title                | Implementation                                                                    | Status |
| ----------- | -------------------- | --------------------------------------------------------------------------------- | ------ |
| AC.L2-3.1.5 | Least Privilege      | Privilege detection via `elevate.rs`; operations request only needed capabilities | ✓      |
| AC.L2-3.1.7 | Privileged Functions | Elevation tracked, audit event emitted for all `sudo`/`runas` operations          | ✓      |

#### Audit and Accountability (AU)

| Practice    | Title            | Implementation                                                                          | Status |
| ----------- | ---------------- | --------------------------------------------------------------------------------------- | ------ |
| AU.L2-3.3.1 | System Auditing  | `AuditEvent` struct captures all security-relevant operations                           | ✓      |
| AU.L2-3.3.2 | Audit Content    | Who (operator), What (event_type), When (timestamp), Where (hostname), Outcome captured | ✓      |
| AU.L2-3.3.8 | Audit Protection | HMAC-SHA256 integrity chain prevents undetected tampering                               | ✓      |

#### Identification and Authentication (IA)

| Practice     | Title                                 | Implementation                                                            | Status |
| ------------ | ------------------------------------- | ------------------------------------------------------------------------- | ------ |
| IA.L2-3.5.10 | Cryptographically-protected Passwords | No hardcoded passwords in source; ATA erase password is runtime-generated | ✓      |

#### Media Protection (MP)

| Practice    | Title                | Implementation                                                        | Status |
| ----------- | -------------------- | --------------------------------------------------------------------- | ------ |
| MP.L2-3.8.3 | Media Sanitization   | NIST SP 800-88 Clear/Purge methods with audit records                 | ✓      |
| MP.L2-3.8.5 | Media Accountability | `SanitizationRecord` documents device, method, verification, operator | ✓      |

#### System and Communications Protection (SC)

| Practice      | Title                        | Implementation                                                         | Status |
| ------------- | ---------------------------- | ---------------------------------------------------------------------- | ------ |
| SC.L2-3.13.8  | Transmission Confidentiality | TLS 1.2+ enforced; HTTPS-only in FIPS mode                             | ✓      |
| SC.L2-3.13.11 | FIPS-Validated Cryptography  | FIPS mode restricts to approved algorithms; CMVP limitation documented | ◐      |
| SC.L2-3.13.16 | Data at Rest                 | Sensitive buffers subject to zeroization guidance                      | ◐      |

#### System and Information Integrity (SI)

| Practice     | Title                     | Implementation                                           | Status |
| ------------ | ------------------------- | -------------------------------------------------------- | ------ |
| SI.L2-3.14.1 | Flaw Remediation          | SHA-256/SHA-512 integrity verification for all downloads | ✓      |
| SI.L2-3.14.2 | Malicious Code Protection | Signature verification for signed artifacts              | ✓      |
| SI.L2-3.14.4 | Update Alerts             | `update` command checks for new versions                 | ✓      |

### Legend

- ✓ = Fully compliant
- ◐ = Partially compliant (known limitation documented)
- ✗ = Not compliant (remediation required)

## Known Limitations

### CRYPTO-05: No CMVP-Validated Cryptographic Module

**Severity:** Non-compliant for FIPS 140-2/3 Level 1 certification

The hash implementations (sha2, blake3 crates) and TLS stack (rustls) are
pure-Rust implementations that have not undergone CMVP validation. They do not
hold FIPS 140-2/3 certificates.

**Remediation path:**
1. Switch `rustls` to use `aws-lc-rs` as its cryptographic backend
   (AWS-LC holds CMVP Certificate #4631)
2. Or use `openssl` crate with the OpenSSL 3.x FIPS provider module

**Mitigation:** Algorithm correctness is verified through the comprehensive test
suite (929 tests). Implementation matches FIPS 180-4 expected outputs.

### TLS-01: rustls Not CMVP-Validated

**Severity:** Advisory

rustls provides strong cryptographic security and has undergone security audits,
but does not hold a CMVP certificate. TLS 1.2 minimum is enforced.

**Remediation:** Same as CRYPTO-05.

## Running the Compliance Assessment

```bash
# Text report to stderr
abt compliance

# JSON report for SIEM integration
abt compliance --json

# With FIPS mode enabled
abt --fips compliance

# Save audit log
abt compliance --save-audit-log /var/log/abt/
```

### Example Output

```
  ── Compliance Self-Assessment (FIPS / CMMC 2.0 / DoD) ──
  FIPS mode: ENABLED
  abt version: 1.6.0

  ✓ [PASS] Hash algorithm restrictions (FIPS 180-4 / SP 800-131A)
    FIPS mode active. Only SHA-256 and SHA-512 are permitted.

  ✗ [NON-COMPLIANT] No CMVP-validated cryptographic module (FIPS 140-2/3)
    Hash implementations are pure-Rust without CMVP certificates.
    Remediation: Integrate aws-lc-rs (CMVP Certificate #4631).

  ✓ [PASS] Structured security audit trail (CMMC AU.L2-3.3.1)
    Events are HMAC-chained for tamper detection.

  Summary: 5 PASS, 2 advisory, 1 non-compliant, 0 critical
```

## Audit Trail Format

Security-relevant events are recorded as structured JSON with HMAC integrity
chaining per CMMC AU.L2-3.3.8:

```json
{
  "sequence": 1,
  "timestamp": "2025-01-15T10:30:00.000Z",
  "event_type": "ERASE_OPERATION",
  "outcome": "SUCCESS",
  "operator": "admin",
  "hostname": "workstation-01",
  "source_component": "erase",
  "description": "Secure erase completed: 16000000000 bytes using zero-fill (1 passes)",
  "target": "/dev/sdb",
  "fips_mode": true,
  "integrity_hash": "a1b2c3d4...",
  "metadata": {
    "method": "zero-fill",
    "passes": "1",
    "bytes_erased": "16000000000",
    "sanitization_level": "Clear"
  }
}
```

## Sanitization Records (SP 800-88 §4.7)

Every erase operation can produce a sanitization certificate per
NIST SP 800-88 Rev 1 Table A-8:

| Field                    | Description                          |
| ------------------------ | ------------------------------------ |
| `record_id`              | Unique UUID                          |
| `timestamp`              | ISO 8601 start time                  |
| `completed_at`           | ISO 8601 completion time             |
| `device_path`            | Target device                        |
| `device_serial`          | Hardware serial number               |
| `device_model`           | Device model name                    |
| `device_size`            | Size in bytes                        |
| `method`                 | Sanitization method used             |
| `sanitization_level`     | Clear / Purge / Destroy              |
| `passes_completed`       | Number of overwrite passes           |
| `verification_performed` | Whether post-erase verification ran  |
| `verification_passed`    | Verification result                  |
| `operator`               | Username who performed the operation |
| `hostname`               | Machine hostname                     |
| `fips_mode`              | Whether FIPS mode was active         |
| `tool_version`           | abt version                          |

## Hash Algorithm Reference

| Algorithm | FIPS Status                | Notes                                         |
| --------- | -------------------------- | --------------------------------------------- |
| SHA-256   | ✓ Approved (FIPS 180-4)    | Default. Recommended for all use.             |
| SHA-512   | ✓ Approved (FIPS 180-4)    | Higher security margin.                       |
| SHA-1     | ⚠ Deprecated (SP 800-131A) | Blocked in FIPS mode. Legacy only.            |
| MD5       | ✗ Not Approved             | Cryptographically broken. Prohibited.         |
| BLAKE3    | ✗ Not Approved             | Not NIST-standardized. Fast but not FIPS.     |
| CRC32     | ✗ Not Applicable           | Error detection only. No security properties. |
