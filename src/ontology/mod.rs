// Ontology module — AI-discoverable capability schema
//
// Provides a machine-readable, semantically-annotated description of abt's
// capabilities using JSON-LD and schema.org vocabulary. This enables agentic AI systems
// to discover, understand, and invoke abt operations without human guidance.

pub mod openapi;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Top-level ontology describing the tool's capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOntology {
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Vec<Capability>,
    pub types: Vec<TypeDefinition>,
    pub platform_support: PlatformSupport,
    pub device_scope: DeviceScope,
}

/// A single capability (action) the tool can perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: CapabilityCategory,
    pub cli_command: String,
    pub parameters: Vec<Parameter>,
    pub returns: ReturnType,
    pub requires_elevation: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub examples: Vec<Example>,
    pub preconditions: Vec<String>,
    pub postconditions: Vec<String>,
    pub related: Vec<String>,
}

/// Parameter definition for a capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub cli_flag: String,
    pub description: String,
    pub param_type: ParamType,
    pub required: bool,
    pub default: Option<Value>,
    pub constraints: Option<Value>,
    pub examples: Vec<String>,
    pub semantic_type: Option<String>,
}

/// Parameter data type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    String,
    Integer,
    Boolean,
    FilePath,
    DevicePath,
    Url,
    Enum(Vec<String>),
    ByteSize,
    Hash,
}

/// Return type description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnType {
    pub description: String,
    pub schema: Option<Value>,
    pub exit_codes: Vec<ExitCode>,
}

/// Exit code semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitCode {
    pub code: i32,
    pub meaning: String,
}

/// Capability categories for filtering.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityCategory {
    DataTransfer,
    Verification,
    DeviceManagement,
    Information,
    Formatting,
    Meta,
}

/// Platform support matrix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformSupport {
    pub operating_systems: Vec<OsSupport>,
    pub architectures: Vec<String>,
}

/// Per-OS support details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsSupport {
    pub name: String,
    pub min_version: Option<String>,
    pub elevation_method: String,
    pub device_path_format: String,
    pub notes: Vec<String>,
}

/// Device scope — what hardware targets are supported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceScope {
    pub categories: Vec<DeviceCategoryInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCategoryInfo {
    pub name: String,
    pub description: String,
    pub examples: Vec<String>,
}

/// Type definition for the ontology schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDefinition {
    pub name: String,
    pub description: String,
    pub values: Vec<TypeValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeValue {
    pub value: String,
    pub description: String,
}

/// Example invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Example {
    pub description: String,
    pub command: String,
    pub use_case: String,
}

/// Build the complete ontology for AgenticBlockTransfer.
pub fn build_ontology() -> ToolOntology {
    ToolOntology {
        name: "AgenticBlockTransfer".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        description: "Cross-platform data/disk writer for images, devices, and streams. \
            Writes disk images (ISO, IMG, VHD, QCOW2, raw, compressed) to block devices \
            (USB, SD, NVMe, eMMC, SPI flash). Supports verification, checksumming, formatting, \
            and device enumeration. Scales from microcontrollers to cloud servers."
            .to_string(),
        capabilities: build_capabilities(),
        types: build_type_definitions(),
        platform_support: build_platform_support(),
        device_scope: build_device_scope(),
    }
}

fn build_capabilities() -> Vec<Capability> {
    let mut capabilities = vec![
        Capability {
            id: "write".to_string(),
            name: "Write Image".to_string(),
            description: "Write a disk image file to a target block device. Supports automatic \
                decompression of gz/bz2/xz/zstd/zip archives, direct I/O for optimal performance, \
                and optional post-write verification."
                .to_string(),
            category: CapabilityCategory::DataTransfer,
            cli_command: "abt write".to_string(),
            parameters: vec![
                Parameter {
                    name: "source".to_string(),
                    cli_flag: "-i, --input".to_string(),
                    description: "Source image file path or URL".to_string(),
                    param_type: ParamType::String,
                    required: true,
                    default: None,
                    constraints: Some(json!({"formats": ["raw", "iso", "img", "dmg", "vhd", "vhdx", "vmdk", "qcow2", "wim", "ffu", "gz", "bz2", "xz", "zstd", "zip"]})),
                    examples: vec![
                        "/path/to/image.iso".to_string(),
                        "ubuntu-24.04-desktop-amd64.iso".to_string(),
                        "firmware.img.gz".to_string(),
                        "https://example.com/image.img.xz".to_string(),
                    ],
                    semantic_type: Some("schema:MediaObject".to_string()),
                },
                Parameter {
                    name: "target".to_string(),
                    cli_flag: "-o, --output".to_string(),
                    description: "Target device path".to_string(),
                    param_type: ParamType::DevicePath,
                    required: true,
                    default: None,
                    constraints: Some(json!({"must_be": "block_device", "not": "system_drive"})),
                    examples: vec![
                        "/dev/sdb".to_string(),
                        r"\\.\PhysicalDrive1".to_string(),
                        "/dev/disk2".to_string(),
                        "/dev/mmcblk0".to_string(),
                    ],
                    semantic_type: Some("schema:ComputerHardware".to_string()),
                },
                Parameter {
                    name: "block_size".to_string(),
                    cli_flag: "-b, --block-size".to_string(),
                    description: "I/O block size for read/write operations".to_string(),
                    param_type: ParamType::ByteSize,
                    required: false,
                    default: Some(json!("4M")),
                    constraints: Some(json!({"min": "512", "max": "256M", "typical": ["512", "4K", "64K", "1M", "4M", "16M"]})),
                    examples: vec!["4M".to_string(), "1M".to_string(), "64K".to_string()],
                    semantic_type: None,
                },
                Parameter {
                    name: "verify".to_string(),
                    cli_flag: "--verify / --no-verify".to_string(),
                    description: "Verify written data by reading back and comparing with source"
                        .to_string(),
                    param_type: ParamType::Boolean,
                    required: false,
                    default: Some(json!(true)),
                    constraints: None,
                    examples: vec![],
                    semantic_type: None,
                },
                Parameter {
                    name: "force".to_string(),
                    cli_flag: "-f, --force".to_string(),
                    description: "Skip safety confirmation prompts".to_string(),
                    param_type: ParamType::Boolean,
                    required: false,
                    default: Some(json!(false)),
                    constraints: None,
                    examples: vec![],
                    semantic_type: None,
                },
                Parameter {
                    name: "mode".to_string(),
                    cli_flag: "--mode".to_string(),
                    description: "Write mode: raw (dd-style block copy), extract (ISO contents), clone (device-to-device)".to_string(),
                    param_type: ParamType::Enum(vec!["raw".to_string(), "extract".to_string(), "clone".to_string()]),
                    required: false,
                    default: Some(json!("raw")),
                    constraints: None,
                    examples: vec!["raw".to_string()],
                    semantic_type: None,
                },
                Parameter {
                    name: "hash_algorithm".to_string(),
                    cli_flag: "--hash-algorithm".to_string(),
                    description: "Hash algorithm for pre-write and post-write verification".to_string(),
                    param_type: ParamType::Enum(vec!["md5".to_string(), "sha1".to_string(), "sha256".to_string(), "sha512".to_string(), "blake3".to_string(), "crc32".to_string()]),
                    required: false,
                    default: Some(json!("sha256")),
                    constraints: None,
                    examples: vec![],
                    semantic_type: None,
                },
                Parameter {
                    name: "expected_hash".to_string(),
                    cli_flag: "--expected-hash".to_string(),
                    description: "Expected hash of source image for integrity verification before writing".to_string(),
                    param_type: ParamType::Hash,
                    required: false,
                    default: None,
                    constraints: None,
                    examples: vec!["e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string()],
                    semantic_type: None,
                },
                Parameter {
                    name: "safety_level".to_string(),
                    cli_flag: "--safety-level".to_string(),
                    description: "Safety level for pre-flight checks: low (default), medium (recommended for agents), high (maximum safety)".to_string(),
                    param_type: ParamType::Enum(vec!["low".to_string(), "medium".to_string(), "high".to_string()]),
                    required: false,
                    default: Some(json!("low")),
                    constraints: None,
                    examples: vec!["medium".to_string()],
                    semantic_type: None,
                },
                Parameter {
                    name: "dry_run".to_string(),
                    cli_flag: "--dry-run".to_string(),
                    description: "Run all pre-flight safety checks without writing. Returns structured safety report. Essential for agent validation workflows.".to_string(),
                    param_type: ParamType::Boolean,
                    required: false,
                    default: Some(json!(false)),
                    constraints: None,
                    examples: vec![],
                    semantic_type: None,
                },
                Parameter {
                    name: "confirm_token".to_string(),
                    cli_flag: "--confirm-token".to_string(),
                    description: "Device confirmation token from 'abt list --json'. Prevents TOCTOU race between device enumeration and write. Required at high safety level.".to_string(),
                    param_type: ParamType::String,
                    required: false,
                    default: None,
                    constraints: Some(json!({"format": "hex-encoded 128-bit BLAKE3 hash", "source": "abt list --json → devices[].confirm_token"})),
                    examples: vec!["a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6".to_string()],
                    semantic_type: None,
                },
                Parameter {
                    name: "backup_partition_table".to_string(),
                    cli_flag: "--backup-partition-table".to_string(),
                    description: "Back up the first 1 MiB of the target device (MBR/GPT) before writing. Automatic at high safety level.".to_string(),
                    param_type: ParamType::Boolean,
                    required: false,
                    default: Some(json!(false)),
                    constraints: None,
                    examples: vec![],
                    semantic_type: None,
                },
            ],
            returns: ReturnType {
                description: "Structured JSON result on stdout (with --output json), progress on stderr. Pre-flight safety report always included.".to_string(),
                schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "success": {"type": "boolean"},
                        "exit_code": {"type": "integer"},
                        "bytes_written": {"type": "integer"},
                        "duration_secs": {"type": "number"},
                        "verified": {"type": "boolean"},
                        "speed_bytes_per_sec": {"type": "number"},
                        "device_fingerprint": {"type": "string"},
                        "safety_report": {
                            "type": "object",
                            "properties": {
                                "safe_to_proceed": {"type": "boolean"},
                                "safety_level": {"type": "string"},
                                "errors": {"type": "integer"},
                                "warnings": {"type": "integer"},
                                "checks": {"type": "array"}
                            }
                        }
                    }
                })),
                exit_codes: vec![
                    ExitCode { code: 0, meaning: "Success".to_string() },
                    ExitCode { code: 1, meaning: "General error".to_string() },
                    ExitCode { code: 2, meaning: "Pre-flight safety check failed (blocked by safety system)".to_string() },
                    ExitCode { code: 3, meaning: "Verification failed (data mismatch after write)".to_string() },
                    ExitCode { code: 4, meaning: "Permission denied / insufficient privileges".to_string() },
                    ExitCode { code: 5, meaning: "Source image not found or unreadable".to_string() },
                    ExitCode { code: 6, meaning: "Target device not found, read-only, or unavailable".to_string() },
                    ExitCode { code: 7, meaning: "Image too large for target device".to_string() },
                    ExitCode { code: 8, meaning: "Device changed between enumeration and write (token mismatch)".to_string() },
                    ExitCode { code: 130, meaning: "Cancelled by user or agent (Ctrl+C / SIGINT)".to_string() },
                ],
            },
            requires_elevation: true,
            destructive: true,
            idempotent: true,
            examples: vec![
                Example {
                    description: "Write an ISO to a USB drive on Linux".to_string(),
                    command: "sudo abt write -i ubuntu.iso -o /dev/sdb".to_string(),
                    use_case: "Creating a bootable Linux USB installer".to_string(),
                },
                Example {
                    description: "Flash a compressed firmware image on Windows".to_string(),
                    command: r"abt write -i firmware.img.xz -o \\.\PhysicalDrive2 --force".to_string(),
                    use_case: "Flashing embedded device firmware from a CI/CD pipeline".to_string(),
                },
                Example {
                    description: "Write and verify on macOS".to_string(),
                    command: "sudo abt write -i raspberrypi.img.gz -o /dev/disk4 --verify".to_string(),
                    use_case: "Preparing a Raspberry Pi SD card with verification".to_string(),
                },
                Example {
                    description: "Agent dry-run: validate before writing".to_string(),
                    command: "abt write -i image.iso -o /dev/sdb --dry-run --safety-level medium -o json".to_string(),
                    use_case: "AI agent pre-flight check — validates all safety conditions without writing".to_string(),
                },
                Example {
                    description: "Agent write with device confirmation token".to_string(),
                    command: "abt write -i image.iso -o /dev/sdb --confirm-token a1b2c3d4... --safety-level medium -o json".to_string(),
                    use_case: "AI agent confirmed write — token from 'abt list --json' proves agent inspected the device".to_string(),
                },
                Example {
                    description: "High safety mode with partition backup".to_string(),
                    command: "sudo abt write -i image.iso -o /dev/sdb --safety-level high --confirm-token a1b2c3d4...".to_string(),
                    use_case: "Maximum safety: backs up partition table, requires token, only allows removable media".to_string(),
                },
            ],
            preconditions: vec![
                "Source file must exist and be readable".to_string(),
                "Target device must exist and not be a system drive (unless --force)".to_string(),
                "Process must have elevated privileges".to_string(),
            ],
            postconditions: vec![
                "Target device contains exact byte-for-byte copy of source image".to_string(),
                "If --verify: read-back matches source".to_string(),
                "Device is synced and safe to remove".to_string(),
            ],
            related: vec!["verify".to_string(), "list".to_string(), "checksum".to_string()],
        },

        Capability {
            id: "verify".to_string(),
            name: "Verify Image".to_string(),
            description: "Verify that a device or file matches a source image or expected hash. \
                Useful for post-write validation or integrity checking."
                .to_string(),
            category: CapabilityCategory::Verification,
            cli_command: "abt verify".to_string(),
            parameters: vec![
                Parameter {
                    name: "target".to_string(),
                    cli_flag: "-o, --output".to_string(),
                    description: "Device or file to verify".to_string(),
                    param_type: ParamType::String,
                    required: true,
                    default: None,
                    constraints: None,
                    examples: vec!["/dev/sdb".to_string()],
                    semantic_type: None,
                },
                Parameter {
                    name: "source".to_string(),
                    cli_flag: "-i, --input".to_string(),
                    description: "Source image file to compare against".to_string(),
                    param_type: ParamType::FilePath,
                    required: false,
                    default: None,
                    constraints: None,
                    examples: vec!["ubuntu.iso".to_string()],
                    semantic_type: None,
                },
                Parameter {
                    name: "expected_hash".to_string(),
                    cli_flag: "--expected-hash".to_string(),
                    description: "Expected hash value to verify against".to_string(),
                    param_type: ParamType::Hash,
                    required: false,
                    default: None,
                    constraints: None,
                    examples: vec![],
                    semantic_type: None,
                },
                Parameter {
                    name: "hash_algorithm".to_string(),
                    cli_flag: "--hash-algorithm".to_string(),
                    description: "Hash algorithm to use".to_string(),
                    param_type: ParamType::Enum(vec!["md5".to_string(), "sha256".to_string(), "sha512".to_string(), "blake3".to_string()]),
                    required: false,
                    default: Some(json!("sha256")),
                    constraints: None,
                    examples: vec![],
                    semantic_type: None,
                },
            ],
            returns: ReturnType {
                description: "PASS/FAIL result on stdout, detail on stderr".to_string(),
                schema: None,
                exit_codes: vec![
                    ExitCode { code: 0, meaning: "Verification passed".to_string() },
                    ExitCode { code: 1, meaning: "Verification failed".to_string() },
                ],
            },
            requires_elevation: false,
            destructive: false,
            idempotent: true,
            examples: vec![
                Example {
                    description: "Verify a written USB drive against source".to_string(),
                    command: "abt verify -i ubuntu.iso -o /dev/sdb".to_string(),
                    use_case: "Post-write integrity check".to_string(),
                },
            ],
            preconditions: vec!["Either --source or --expected-hash must be provided".to_string()],
            postconditions: vec!["No changes to any device or file".to_string()],
            related: vec!["write".to_string(), "checksum".to_string()],
        },

        Capability {
            id: "list".to_string(),
            name: "List Devices".to_string(),
            description: "Enumerate available block devices (USB drives, SD cards, NVMe, SATA, \
                virtual disks, etc.) with detailed metadata including size, type, mount state, \
                and safety classification."
                .to_string(),
            category: CapabilityCategory::DeviceManagement,
            cli_command: "abt list".to_string(),
            parameters: vec![
                Parameter {
                    name: "all".to_string(),
                    cli_flag: "--all".to_string(),
                    description: "Include system drives in listing".to_string(),
                    param_type: ParamType::Boolean,
                    required: false,
                    default: Some(json!(false)),
                    constraints: None,
                    examples: vec![],
                    semantic_type: None,
                },
                Parameter {
                    name: "removable".to_string(),
                    cli_flag: "--removable".to_string(),
                    description: "Show only removable devices".to_string(),
                    param_type: ParamType::Boolean,
                    required: false,
                    default: Some(json!(false)),
                    constraints: None,
                    examples: vec![],
                    semantic_type: None,
                },
                Parameter {
                    name: "device_type".to_string(),
                    cli_flag: "-t, --type".to_string(),
                    description: "Filter by device type".to_string(),
                    param_type: ParamType::Enum(vec!["usb".to_string(), "sd".to_string(), "nvme".to_string(), "sata".to_string(), "scsi".to_string(), "mmc".to_string(), "virtual".to_string()]),
                    required: false,
                    default: None,
                    constraints: None,
                    examples: vec!["usb".to_string()],
                    semantic_type: None,
                },
                Parameter {
                    name: "json".to_string(),
                    cli_flag: "--json".to_string(),
                    description: "Output as JSON with device fingerprints / confirmation tokens. Essential for agent integration — provides confirm_token for safe write operations.".to_string(),
                    param_type: ParamType::Boolean,
                    required: false,
                    default: Some(json!(false)),
                    constraints: None,
                    examples: vec![],
                    semantic_type: None,
                },
            ],
            returns: ReturnType {
                description: "Table of devices (text) or JSON array with fingerprints (--json)".to_string(),
                schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "devices": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "path": {"type": "string"},
                                    "name": {"type": "string"},
                                    "size": {"type": "integer"},
                                    "size_human": {"type": "string"},
                                    "device_type": {"type": "string"},
                                    "removable": {"type": "boolean"},
                                    "is_system": {"type": "boolean"},
                                    "mount_points": {"type": "array", "items": {"type": "string"}},
                                    "safe_target": {"type": "boolean", "description": "Whether this device can be safely written to"},
                                    "removable_media": {"type": "boolean"},
                                    "confirm_token": {"type": "string", "description": "BLAKE3-based device fingerprint for --confirm-token"}
                                }
                            }
                        },
                        "count": {"type": "integer"}
                    }
                })),
                exit_codes: vec![
                    ExitCode { code: 0, meaning: "Success".to_string() },
                ],
            },
            requires_elevation: false,
            destructive: false,
            idempotent: true,
            examples: vec![
                Example {
                    description: "List USB drives".to_string(),
                    command: "abt list --type usb".to_string(),
                    use_case: "Finding the target USB drive before writing".to_string(),
                },
                Example {
                    description: "List all devices including system".to_string(),
                    command: "abt list --all".to_string(),
                    use_case: "Full system inventory".to_string(),
                },
                Example {
                    description: "Agent device discovery with fingerprints".to_string(),
                    command: "abt list --json".to_string(),
                    use_case: "AI agent enumerates devices and extracts confirm_token for safe write".to_string(),
                },
            ],
            preconditions: vec![],
            postconditions: vec!["No changes to any device".to_string()],
            related: vec!["info".to_string(), "write".to_string()],
        },

        Capability {
            id: "info".to_string(),
            name: "Device/Image Info".to_string(),
            description: "Show detailed information about a specific device or image file, \
                including format detection, size, partition info, and safety classification."
                .to_string(),
            category: CapabilityCategory::Information,
            cli_command: "abt info".to_string(),
            parameters: vec![
                Parameter {
                    name: "path".to_string(),
                    cli_flag: "<path>".to_string(),
                    description: "Device path or image file path to inspect".to_string(),
                    param_type: ParamType::String,
                    required: true,
                    default: None,
                    constraints: None,
                    examples: vec!["/dev/sdb".to_string(), "image.iso".to_string()],
                    semantic_type: None,
                },
            ],
            returns: ReturnType {
                description: "Detailed device or image information".to_string(),
                schema: None,
                exit_codes: vec![
                    ExitCode { code: 0, meaning: "Success".to_string() },
                    ExitCode { code: 1, meaning: "Not found or unreadable".to_string() },
                ],
            },
            requires_elevation: false,
            destructive: false,
            idempotent: true,
            examples: vec![
                Example {
                    description: "Inspect a device".to_string(),
                    command: "abt info /dev/sdb".to_string(),
                    use_case: "Check device details before writing".to_string(),
                },
                Example {
                    description: "Inspect an image file".to_string(),
                    command: "abt info ubuntu.iso".to_string(),
                    use_case: "Determine image format and size".to_string(),
                },
            ],
            preconditions: vec![],
            postconditions: vec!["No changes".to_string()],
            related: vec!["list".to_string()],
        },

        Capability {
            id: "checksum".to_string(),
            name: "Compute Checksum".to_string(),
            description: "Compute cryptographic checksums/hashes (MD5, SHA-256, SHA-512, BLAKE3, CRC32) \
                of files or block devices."
                .to_string(),
            category: CapabilityCategory::Verification,
            cli_command: "abt checksum".to_string(),
            parameters: vec![
                Parameter {
                    name: "path".to_string(),
                    cli_flag: "<path>".to_string(),
                    description: "File or device to hash".to_string(),
                    param_type: ParamType::String,
                    required: true,
                    default: None,
                    constraints: None,
                    examples: vec!["image.iso".to_string()],
                    semantic_type: None,
                },
                Parameter {
                    name: "algorithm".to_string(),
                    cli_flag: "-a, --algorithm".to_string(),
                    description: "Hash algorithm(s) to compute (can specify multiple)".to_string(),
                    param_type: ParamType::Enum(vec!["md5".to_string(), "sha1".to_string(), "sha256".to_string(), "sha512".to_string(), "blake3".to_string(), "crc32".to_string()]),
                    required: false,
                    default: Some(json!("sha256")),
                    constraints: None,
                    examples: vec!["sha256".to_string(), "blake3".to_string()],
                    semantic_type: None,
                },
            ],
            returns: ReturnType {
                description: "Hash value(s) on stdout in 'HASH (ALGORITHM) = VALUE' format".to_string(),
                schema: None,
                exit_codes: vec![
                    ExitCode { code: 0, meaning: "Success".to_string() },
                ],
            },
            requires_elevation: false,
            destructive: false,
            idempotent: true,
            examples: vec![
                Example {
                    description: "SHA-256 of an ISO".to_string(),
                    command: "abt checksum ubuntu.iso -a sha256".to_string(),
                    use_case: "Verify download integrity".to_string(),
                },
                Example {
                    description: "Multiple algorithms".to_string(),
                    command: "abt checksum firmware.bin -a sha256 -a blake3".to_string(),
                    use_case: "Generate multiple checksums for distribution".to_string(),
                },
            ],
            preconditions: vec!["File must exist and be readable".to_string()],
            postconditions: vec!["No changes".to_string()],
            related: vec!["verify".to_string()],
        },

        Capability {
            id: "format".to_string(),
            name: "Format Device".to_string(),
            description: "Format a block device with a specified filesystem (FAT32, exFAT, NTFS, \
                ext4, XFS, Btrfs, etc.). Erases all existing data."
                .to_string(),
            category: CapabilityCategory::Formatting,
            cli_command: "abt format".to_string(),
            parameters: vec![
                Parameter {
                    name: "device".to_string(),
                    cli_flag: "<device>".to_string(),
                    description: "Device to format".to_string(),
                    param_type: ParamType::DevicePath,
                    required: true,
                    default: None,
                    constraints: None,
                    examples: vec!["/dev/sdb".to_string()],
                    semantic_type: None,
                },
                Parameter {
                    name: "filesystem".to_string(),
                    cli_flag: "-f, --fs".to_string(),
                    description: "Filesystem type".to_string(),
                    param_type: ParamType::Enum(vec!["fat16".to_string(), "fat32".to_string(), "exfat".to_string(), "ntfs".to_string(), "ext2".to_string(), "ext3".to_string(), "ext4".to_string(), "xfs".to_string(), "btrfs".to_string()]),
                    required: true,
                    default: None,
                    constraints: None,
                    examples: vec!["fat32".to_string(), "ext4".to_string()],
                    semantic_type: None,
                },
                Parameter {
                    name: "label".to_string(),
                    cli_flag: "-l, --label".to_string(),
                    description: "Volume label".to_string(),
                    param_type: ParamType::String,
                    required: false,
                    default: None,
                    constraints: Some(json!({"max_length": 32})),
                    examples: vec!["BOOT".to_string(), "DATA".to_string()],
                    semantic_type: None,
                },
                Parameter {
                    name: "quick".to_string(),
                    cli_flag: "-q, --quick".to_string(),
                    description: "Quick format (skip full overwrite)".to_string(),
                    param_type: ParamType::Boolean,
                    required: false,
                    default: Some(json!(false)),
                    constraints: None,
                    examples: vec![],
                    semantic_type: None,
                },
            ],
            returns: ReturnType {
                description: "Success message on completion".to_string(),
                schema: None,
                exit_codes: vec![
                    ExitCode { code: 0, meaning: "Formatted successfully".to_string() },
                    ExitCode { code: 1, meaning: "Format failed".to_string() },
                ],
            },
            requires_elevation: true,
            destructive: true,
            idempotent: true,
            examples: vec![
                Example {
                    description: "Format USB as FAT32".to_string(),
                    command: "sudo abt format /dev/sdb -f fat32 -l BOOT".to_string(),
                    use_case: "Prepare a USB drive for general use".to_string(),
                },
            ],
            preconditions: vec!["Device must exist".to_string(), "Elevated privileges required".to_string()],
            postconditions: vec!["Device formatted with specified filesystem".to_string(), "All previous data erased".to_string()],
            related: vec!["write".to_string(), "list".to_string()],
        },

        Capability {
            id: "ontology".to_string(),
            name: "Export Ontology".to_string(),
            description: "Export the machine-readable capability ontology as JSON-LD for AI agent \
                discovery and integration. Enables agentic systems to understand and invoke \
                abt operations programmatically."
                .to_string(),
            category: CapabilityCategory::Meta,
            cli_command: "abt ontology".to_string(),
            parameters: vec![
                Parameter {
                    name: "format".to_string(),
                    cli_flag: "-f, --format".to_string(),
                    description: "Output format".to_string(),
                    param_type: ParamType::Enum(vec!["json-ld".to_string(), "json".to_string(), "yaml".to_string()]),
                    required: false,
                    default: Some(json!("json-ld")),
                    constraints: None,
                    examples: vec![],
                    semantic_type: None,
                },
                Parameter {
                    name: "full".to_string(),
                    cli_flag: "--full".to_string(),
                    description: "Include full parameter schemas and examples".to_string(),
                    param_type: ParamType::Boolean,
                    required: false,
                    default: Some(json!(false)),
                    constraints: None,
                    examples: vec![],
                    semantic_type: None,
                },
            ],
            returns: ReturnType {
                description: "JSON-LD ontology document on stdout".to_string(),
                schema: None,
                exit_codes: vec![
                    ExitCode { code: 0, meaning: "Success".to_string() },
                ],
            },
            requires_elevation: false,
            destructive: false,
            idempotent: true,
            examples: vec![
                Example {
                    description: "Export full ontology".to_string(),
                    command: "abt ontology --full".to_string(),
                    use_case: "AI agent capability discovery".to_string(),
                },
            ],
            preconditions: vec![],
            postconditions: vec![],
            related: vec![],
        },
    ];
    capabilities.extend(remaining_capabilities());
    capabilities
}

/// Build a capability from the fields that always differ, leaving the
/// descriptive extras empty.
///
/// The seven capabilities above carry full parameter schemas and worked
/// examples. These do not, and that is deliberate staging rather than an
/// oversight: an agent's first question about a command is whether it will
/// destroy anything and whether it needs privilege, and answering that for
/// every command beats answering everything for a seventh of them. Parameter
/// schemas can be filled in per command over time. What must not happen again
/// is a command existing with no ontology entry at all, which is what
/// `every_subcommand_is_declared` now prevents.
#[allow(clippy::too_many_arguments)]
fn cap(
    id: &str,
    name: &str,
    description: &str,
    category: CapabilityCategory,
    requires_elevation: bool,
    destructive: bool,
    idempotent: bool,
    preconditions: &[&str],
) -> Capability {
    Capability {
        id: id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        category,
        cli_command: format!("abt {id}"),
        parameters: vec![],
        returns: ReturnType {
            description: "See `abt <command> --help` for the output shape.".to_string(),
            schema: None,
            exit_codes: vec![
                ExitCode {
                    code: 0,
                    meaning: "Success".to_string(),
                },
                ExitCode {
                    code: 1,
                    meaning: "Failure".to_string(),
                },
            ],
        },
        requires_elevation,
        destructive,
        idempotent,
        examples: vec![],
        preconditions: preconditions.iter().map(|s| s.to_string()).collect(),
        postconditions: vec![],
        related: vec![],
    }
}

/// Every command that is not one of the seven fully-described capabilities.
///
/// Before this existed the ontology declared 7 of 48 commands, so an agent
/// that trusted it as complete would have concluded abt cannot wipe a drive.
fn remaining_capabilities() -> Vec<Capability> {
    vec![
        cap("completions", "Shell Completions", "Generate shell completions for bash, zsh, fish, or PowerShell.", CapabilityCategory::Meta, false, false, true, &[]),
        cap("man", "Man Pages", "Generate man pages to a directory.", CapabilityCategory::Meta, false, false, true, &[]),
        cap("tui", "Interactive TUI", "Launch interactive terminal UI mode.", CapabilityCategory::Meta, false, false, false, &["Holds the terminal until the user exits."]),
        cap("gui", "Graphical UI", "Launch graphical UI mode.", CapabilityCategory::Meta, false, false, false, &["Holds the process until the window is closed."]),
        cap("mcp", "MCP Server", "Start an MCP (Model Context Protocol) server so an agent can drive abt over JSON-RPC.", CapabilityCategory::Meta, false, false, false, &["Serves until stopped.", "Exposes the destructive capabilities in this ontology to whatever connects."]),
        cap("clone", "Clone Device", "Clone a device or image to another device (block-level copy).", CapabilityCategory::DataTransfer, true, true, true, &["The target is overwritten in full."]),
        cap("erase", "Secure Erase", "Securely erase a device by zero-fill, random overwrite, ATA secure erase, or NVMe sanitize.", CapabilityCategory::DeviceManagement, true, true, true, &["All data on the target is destroyed and is not recoverable."]),
        cap("boot", "Validate Boot Sector", "Validate the boot sector (MBR / GPT / UEFI) of a device or image.", CapabilityCategory::Verification, true, false, true, &[]),
        cap("catalog", "Image Catalog", "Browse the Raspberry Pi OS image catalog.", CapabilityCategory::Information, false, false, true, &[]),
        cap("bench", "Benchmark I/O", "Benchmark I/O throughput to choose a block size and compare devices.", CapabilityCategory::Information, true, true, false, &["Declared destructive because write-throughput measurement writes to the target. A read-only benchmark does not, but the flag is the conservative one."]),
        cap("diff", "Differential Write", "Differential write: only write blocks that differ between source and target.", CapabilityCategory::DataTransfer, true, true, true, &["The target is modified in place."]),
        cap("multiboot", "Multi-boot USB", "Manage a multi-boot USB device: add, remove, and list ISOs, Ventoy-style.", CapabilityCategory::DeviceManagement, true, true, true, &["Adding or removing an ISO rewrites device structures."]),
        cap("customize", "OS Customization", "Apply OS customization -- hostname, SSH, WiFi, users -- for firstrun or cloud-init.", CapabilityCategory::DeviceManagement, true, true, true, &["Writes configuration onto the target's filesystem.", "Credentials supplied here land on the device in whatever form the target OS expects."]),
        cap("cache", "Image Cache", "Manage the local image download cache: list, verify, clean, evict.", CapabilityCategory::Meta, false, true, true, &["clean and evict delete cached images; list and verify do not."]),
        cap("health", "Drive Health", "Check drive health: bad blocks, fake-flash detection, read test.", CapabilityCategory::Verification, true, true, true, &["The default read test is non-destructive.", "--force runs a destructive bad-block test that overwrites the device. Declared destructive because that mode exists."]),
        cap("backup", "Back Up Drive", "Back up a drive or partition to a compressed image file.", CapabilityCategory::DataTransfer, true, false, true, &["Reads the source device; writes only the image file."]),
        cap("persist", "Persistence Partition", "Create a persistent storage partition for a live Linux USB.", CapabilityCategory::DeviceManagement, true, true, true, &["Repartitions the target."]),
        cap("update", "Self Update", "Check for abt updates and optionally self-update.", CapabilityCategory::Meta, true, false, false, &["Replaces the installed abt binary, which may require elevation depending on the install path."]),
        cap("mirror", "Download Mirrors", "Manage download mirrors: probe latency, failover, metalink.", CapabilityCategory::Meta, false, false, true, &[]),
        cap("checksum-file", "Checksum Files", "Parse and verify checksum files such as SHA256SUMS and MD5SUMS.", CapabilityCategory::Verification, false, false, true, &[]),
        cap("usb-info", "USB Speed Info", "Show USB device speed information and write-time estimates.", CapabilityCategory::Information, true, false, true, &[]),
        cap("signature", "Signature Verification", "Verify signed downloads using RSA PKCS#1 v1.5 signature verification.", CapabilityCategory::Verification, false, false, true, &[]),
        cap("wue", "Windows Unattend", "Generate a Windows Unattended Setup file (unattend.xml) for automated installation.", CapabilityCategory::Meta, false, false, true, &["Any credentials placed in the answer file are stored in it."]),
        cap("uefi-ntfs", "UEFI:NTFS Layout", "Plan a UEFI:NTFS dual-partition boot layout for large Windows images.", CapabilityCategory::DeviceManagement, false, false, true, &["Plans a layout; writing it is a separate operation."]),
        cap("fleet", "Fleet Write", "Multi-target fleet write: flash the same image to several USB devices at once.", CapabilityCategory::DataTransfer, true, true, true, &["Every target in the fleet is overwritten in full."]),
        cap("restore", "Factory Restore", "Restore a drive to factory state: wipe, repartition, format.", CapabilityCategory::DeviceManagement, true, true, true, &["All data on the target is destroyed."]),
        cap("telemetry", "Performance Telemetry", "View and export performance telemetry data.", CapabilityCategory::Information, false, false, true, &[]),
        cap("watchdog", "Write Watchdog", "Configure and test the write watchdog: stall detection and recovery.", CapabilityCategory::DeviceManagement, false, false, true, &[]),
        cap("wim-extract", "WIM Extract", "Extract files from WIM archives (Windows installation images).", CapabilityCategory::DataTransfer, false, false, true, &["Writes extracted files to the output path."]),
        cap("secure-boot", "Secure Boot Check", "Check Secure Boot status and verify bootloader signatures.", CapabilityCategory::Verification, false, false, true, &[]),
        cap("fs-detect", "Filesystem Detect", "Detect filesystem type from a device or image superblock magic.", CapabilityCategory::Information, true, false, true, &[]),
        cap("drive-scan", "Drive Scan", "Scan for attached drives with hot-plug detection.", CapabilityCategory::Information, true, false, true, &[]),
        cap("drive-constraints", "Drive Constraints", "Validate drive compatibility and constraints for an operation.", CapabilityCategory::Information, true, false, true, &[]),
        cap("win-to-go", "Windows To Go", "Create a Windows To Go bootable USB drive.", CapabilityCategory::DeviceManagement, true, true, true, &["The target is repartitioned and overwritten."]),
        cap("syslinux", "Bootloader Install", "Detect, plan, and configure a Syslinux or GRUB bootloader installation.", CapabilityCategory::DeviceManagement, true, true, true, &["Detection and planning are read-only.", "Configuring writes a bootloader to the target. Declared destructive because that mode exists."]),
        cap("ffu", "FFU Inspect", "Parse and inspect FFU (Full Flash Update) images.", CapabilityCategory::Information, false, false, true, &[]),
        cap("iso-hybrid", "ISOHybrid Detect", "Detect ISOHybrid (MBR/GPT embedded) ISO images and recommend a write mode.", CapabilityCategory::Information, false, false, true, &[]),
        cap("proc-lock", "Process Locks", "Detect processes holding locks on target drives.", CapabilityCategory::Information, true, false, true, &[]),
        cap("elevate", "Privilege Elevation", "Check and manage privilege elevation.", CapabilityCategory::DeviceManagement, false, false, true, &["Reports and requests elevation; it does not itself modify a device."]),
        cap("optical", "Optical Media", "Read optical disc media (CD/DVD/Blu-ray) to ISO files.", CapabilityCategory::DataTransfer, true, false, true, &["Reads the disc; writes only the ISO file."]),
        cap("compliance", "Compliance Audit", "Run a FIPS / CMMC 2.0 / DoD compliance self-assessment.", CapabilityCategory::Verification, false, false, true, &[]),
    ]
}

fn build_type_definitions() -> Vec<TypeDefinition> {
    vec![
        TypeDefinition {
            name: "ImageFormat".to_string(),
            description: "Supported disk image formats".to_string(),
            values: vec![
                TypeValue { value: "raw".to_string(), description: "Raw binary disk image".to_string() },
                TypeValue { value: "iso".to_string(), description: "ISO 9660 optical disc image".to_string() },
                TypeValue { value: "img".to_string(), description: "Raw disk image (alternative extension)".to_string() },
                TypeValue { value: "dmg".to_string(), description: "Apple Disk Image".to_string() },
                TypeValue { value: "vhd".to_string(), description: "Microsoft Virtual Hard Disk".to_string() },
                TypeValue { value: "vhdx".to_string(), description: "Microsoft Virtual Hard Disk v2".to_string() },
                TypeValue { value: "vmdk".to_string(), description: "VMware Virtual Disk".to_string() },
                TypeValue { value: "qcow2".to_string(), description: "QEMU Copy-On-Write v2".to_string() },
                TypeValue { value: "wim".to_string(), description: "Windows Imaging Format".to_string() },
                TypeValue { value: "ffu".to_string(), description: "Full Flash Update (Windows IoT)".to_string() },
            ],
        },
        TypeDefinition {
            name: "CompressionFormat".to_string(),
            description: "Supported compression wrappers (auto-detected and decompressed)".to_string(),
            values: vec![
                TypeValue { value: "gz".to_string(), description: "Gzip compression".to_string() },
                TypeValue { value: "bz2".to_string(), description: "Bzip2 compression".to_string() },
                TypeValue { value: "xz".to_string(), description: "XZ/LZMA compression".to_string() },
                TypeValue { value: "zstd".to_string(), description: "Zstandard compression".to_string() },
                TypeValue { value: "zip".to_string(), description: "ZIP archive".to_string() },
            ],
        },
        TypeDefinition {
            name: "DeviceType".to_string(),
            description: "Block device type classification".to_string(),
            values: vec![
                TypeValue { value: "usb".to_string(), description: "USB mass storage device".to_string() },
                TypeValue { value: "sd".to_string(), description: "SD/microSD card".to_string() },
                TypeValue { value: "nvme".to_string(), description: "NVMe SSD".to_string() },
                TypeValue { value: "sata".to_string(), description: "SATA drive".to_string() },
                TypeValue { value: "scsi".to_string(), description: "SCSI device".to_string() },
                TypeValue { value: "mmc".to_string(), description: "MMC/eMMC storage".to_string() },
                TypeValue { value: "emmc".to_string(), description: "Embedded MMC".to_string() },
                TypeValue { value: "spi".to_string(), description: "SPI flash memory".to_string() },
                TypeValue { value: "i2c_eeprom".to_string(), description: "I2C EEPROM".to_string() },
                TypeValue { value: "virtual".to_string(), description: "Virtual block device".to_string() },
                TypeValue { value: "network".to_string(), description: "Network block device (iSCSI, NBD)".to_string() },
            ],
        },
        TypeDefinition {
            name: "Filesystem".to_string(),
            description: "Supported filesystem types for formatting".to_string(),
            values: vec![
                TypeValue { value: "fat16".to_string(), description: "FAT16 (legacy, small devices)".to_string() },
                TypeValue { value: "fat32".to_string(), description: "FAT32 (universal compatibility, <4GB files)".to_string() },
                TypeValue { value: "exfat".to_string(), description: "exFAT (large files, cross-platform)".to_string() },
                TypeValue { value: "ntfs".to_string(), description: "NTFS (Windows native)".to_string() },
                TypeValue { value: "ext2".to_string(), description: "ext2 (Linux, no journaling)".to_string() },
                TypeValue { value: "ext3".to_string(), description: "ext3 (Linux, journaled)".to_string() },
                TypeValue { value: "ext4".to_string(), description: "ext4 (Linux, modern default)".to_string() },
                TypeValue { value: "xfs".to_string(), description: "XFS (high-performance Linux)".to_string() },
                TypeValue { value: "btrfs".to_string(), description: "Btrfs (Linux, COW, snapshots)".to_string() },
            ],
        },
        TypeDefinition {
            name: "HashAlgorithm".to_string(),
            description: "Supported hash/checksum algorithms".to_string(),
            values: vec![
                TypeValue { value: "md5".to_string(), description: "MD5 (legacy, fast, not cryptographically secure)".to_string() },
                TypeValue { value: "sha1".to_string(), description: "SHA-1 (deprecated, backward compat)".to_string() },
                TypeValue { value: "sha256".to_string(), description: "SHA-256 (recommended default)".to_string() },
                TypeValue { value: "sha512".to_string(), description: "SHA-512 (maximum security)".to_string() },
                TypeValue { value: "blake3".to_string(), description: "BLAKE3 (fastest, modern, secure)".to_string() },
                TypeValue { value: "crc32".to_string(), description: "CRC32 (fast error detection, not cryptographic)".to_string() },
            ],
        },
        TypeDefinition {
            name: "SafetyLevel".to_string(),
            description: "Pre-flight safety check intensity levels. Higher levels add more checks and require more explicit confirmation.".to_string(),
            values: vec![
                TypeValue { value: "low".to_string(), description: "Default for humans. Blocks system drive writes, interactive confirmation prompt.".to_string() },
                TypeValue { value: "medium".to_string(), description: "Recommended for AI agents. All low checks + requires removable media or confirm-token, validates image size, refuses in-use devices.".to_string() },
                TypeValue { value: "high".to_string(), description: "Maximum safety. All medium checks + requires confirm-token (no interactive fallback), backs up partition table, only allows removable media.".to_string() },
            ],
        },
        TypeDefinition {
            name: "ExitCode".to_string(),
            description: "Structured exit codes with well-defined semantics for agent consumption. dd uses only 0/1; abt provides granular error classification.".to_string(),
            values: vec![
                TypeValue { value: "0".to_string(), description: "Success — operation completed without errors".to_string() },
                TypeValue { value: "1".to_string(), description: "General / unspecified error".to_string() },
                TypeValue { value: "2".to_string(), description: "Pre-flight safety check failed (blocked by safety system)".to_string() },
                TypeValue { value: "3".to_string(), description: "Verification failed (data mismatch after write)".to_string() },
                TypeValue { value: "4".to_string(), description: "Permission denied / insufficient privileges".to_string() },
                TypeValue { value: "5".to_string(), description: "Source image not found or unreadable".to_string() },
                TypeValue { value: "6".to_string(), description: "Target device not found, read-only, or unavailable".to_string() },
                TypeValue { value: "7".to_string(), description: "Image too large for target device".to_string() },
                TypeValue { value: "8".to_string(), description: "Device changed between enumeration and write (confirm-token mismatch — TOCTOU prevented)".to_string() },
                TypeValue { value: "130".to_string(), description: "Cancelled by user or agent (Ctrl+C / SIGINT)".to_string() },
            ],
        },
    ]
}

fn build_platform_support() -> PlatformSupport {
    PlatformSupport {
        operating_systems: vec![
            OsSupport {
                name: "Linux".to_string(),
                min_version: Some("4.0+".to_string()),
                elevation_method: "sudo / pkexec".to_string(),
                device_path_format: "/dev/sdX, /dev/nvmeXnY, /dev/mmcblkX".to_string(),
                notes: vec![
                    "Uses sysfs for device enumeration".to_string(),
                    "Supports O_DIRECT for unbuffered I/O".to_string(),
                    "Works on x86_64, ARM, ARM64, RISC-V".to_string(),
                ],
            },
            OsSupport {
                name: "macOS".to_string(),
                min_version: Some("10.15+".to_string()),
                elevation_method: "sudo".to_string(),
                device_path_format: "/dev/diskN, /dev/rdiskN".to_string(),
                notes: vec![
                    "Uses diskutil for device enumeration".to_string(),
                    "Use /dev/rdiskN for faster raw access".to_string(),
                ],
            },
            OsSupport {
                name: "Windows".to_string(),
                min_version: Some("10".to_string()),
                elevation_method: "Run as Administrator".to_string(),
                device_path_format: r"\\.\PhysicalDriveN".to_string(),
                notes: vec![
                    "Uses PowerShell Get-Disk for enumeration".to_string(),
                    "Requires Administrator privileges for device access".to_string(),
                ],
            },
        ],
        architectures: vec![
            "x86_64".to_string(),
            "aarch64".to_string(),
            "x86".to_string(),
            "armv7".to_string(),
            "riscv64".to_string(),
        ],
    }
}

fn build_device_scope() -> DeviceScope {
    DeviceScope {
        categories: vec![
            DeviceCategoryInfo {
                name: "Microcontroller / Embedded".to_string(),
                description: "SPI flash, I2C EEPROM, eMMC on embedded boards".to_string(),
                examples: vec![
                    "ESP32 SPI flash".to_string(),
                    "Raspberry Pi eMMC".to_string(),
                    "STM32 internal flash".to_string(),
                    "BeagleBone eMMC".to_string(),
                ],
            },
            DeviceCategoryInfo {
                name: "Removable Media".to_string(),
                description: "USB drives, SD/microSD cards, external SSDs".to_string(),
                examples: vec![
                    "USB 3.0 flash drives".to_string(),
                    "microSD cards (Raspberry Pi, phones)".to_string(),
                    "USB external SSDs".to_string(),
                    "CF cards (industrial)".to_string(),
                ],
            },
            DeviceCategoryInfo {
                name: "Desktop / Workstation".to_string(),
                description: "Internal SATA/NVMe drives, virtual disks".to_string(),
                examples: vec![
                    "NVMe SSDs".to_string(),
                    "SATA HDDs/SSDs".to_string(),
                    "Device mapper targets".to_string(),
                ],
            },
            DeviceCategoryInfo {
                name: "Cloud / Server".to_string(),
                description: "Virtual block devices, iSCSI targets, cloud volumes".to_string(),
                examples: vec![
                    "AWS EBS volumes".to_string(),
                    "Azure managed disks".to_string(),
                    "GCP persistent disks".to_string(),
                    "iSCSI/NBD targets".to_string(),
                    "Virtio block devices".to_string(),
                ],
            },
        ],
    }
}

/// Convert the ontology to JSON-LD format with schema.org annotations.
pub fn to_json_ld(ontology: &ToolOntology, full: bool) -> Value {
    let mut capabilities = Vec::new();
    for cap in &ontology.capabilities {
        let mut cap_json = json!({
            "@type": "schema:Action",
            "@id": format!("abt:{}", cap.id),
            "schema:name": cap.name,
            "schema:description": cap.description,
            "abt:category": format!("{:?}", cap.category),
            "abt:cliCommand": cap.cli_command,
            "abt:requiresElevation": cap.requires_elevation,
            "abt:destructive": cap.destructive,
            "abt:idempotent": cap.idempotent,
            "abt:preconditions": cap.preconditions,
            "abt:postconditions": cap.postconditions,
            "abt:relatedCapabilities": cap.related,
        });

        if full {
            let params: Vec<Value> = cap
                .parameters
                .iter()
                .map(|p| {
                    json!({
                        "@type": "schema:PropertyValueSpecification",
                        "schema:name": p.name,
                        "schema:description": p.description,
                        "abt:cliFlag": p.cli_flag,
                        "abt:paramType": format!("{:?}", p.param_type),
                        "abt:required": p.required,
                        "abt:default": p.default,
                        "abt:constraints": p.constraints,
                        "abt:examples": p.examples,
                        "schema:additionalType": p.semantic_type,
                    })
                })
                .collect();
            cap_json["abt:parameters"] = json!(params);

            let examples: Vec<Value> = cap
                .examples
                .iter()
                .map(|e| {
                    json!({
                        "@type": "schema:HowToStep",
                        "schema:description": e.description,
                        "schema:text": e.command,
                        "abt:useCase": e.use_case,
                    })
                })
                .collect();
            cap_json["schema:potentialAction"] = json!(examples);

            cap_json["abt:returnType"] = json!({
                "schema:description": cap.returns.description,
                "abt:schema": cap.returns.schema,
                "abt:exitCodes": cap.returns.exit_codes.iter().map(|ec| json!({
                    "abt:code": ec.code,
                    "schema:description": ec.meaning,
                })).collect::<Vec<_>>(),
            });
        }

        capabilities.push(cap_json);
    }

    let types_json: Vec<Value> = if full {
        ontology
            .types
            .iter()
            .map(|t| {
                json!({
                    "@type": "schema:Enumeration",
                    "schema:name": t.name,
                    "schema:description": t.description,
                    "abt:values": t.values.iter().map(|v| json!({
                        "schema:value": v.value,
                        "schema:description": v.description,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect()
    } else {
        ontology
            .types
            .iter()
            .map(|t| {
                json!({
                    "schema:name": t.name,
                    "schema:description": t.description,
                    "abt:valueCount": t.values.len(),
                })
            })
            .collect()
    };

    json!({
        "@context": {
            "schema": "https://schema.org/",
            "abt": "https://github.com/nervosys/AgenticBlockTransfer/ontology#",
            "xsd": "http://www.w3.org/2001/XMLSchema#"
        },
        "@type": "schema:SoftwareApplication",
        "@id": "abt:AgenticBlockTransfer",
        "schema:name": ontology.name,
        "schema:version": ontology.version,
        "schema:description": ontology.description,
        "schema:applicationCategory": "DeveloperApplication",
        "schema:operatingSystem": ontology.platform_support.operating_systems.iter()
            .map(|os| os.name.clone()).collect::<Vec<_>>(),
        "schema:processorRequirements": ontology.platform_support.architectures,
        "abt:capabilities": capabilities,
        "abt:typeDefinitions": types_json,
        "abt:platformSupport": {
            "operatingSystems": ontology.platform_support.operating_systems.iter().map(|os| json!({
                "schema:name": os.name,
                "abt:minVersion": os.min_version,
                "abt:elevationMethod": os.elevation_method,
                "abt:devicePathFormat": os.device_path_format,
                "abt:notes": os.notes,
            })).collect::<Vec<_>>(),
        },
        "abt:deviceScope": ontology.device_scope.categories.iter().map(|c| json!({
            "schema:name": c.name,
            "schema:description": c.description,
            "schema:examples": c.examples,
        })).collect::<Vec<_>>(),
    })
}

/// Convert ontology to plain JSON (no semantic annotations).
pub fn to_json(ontology: &ToolOntology, _full: bool) -> Value {
    serde_json::to_value(ontology).unwrap_or(json!(null))
}

#[cfg(test)]
mod completeness {
    use super::*;
    use clap::CommandFactory;

    /// Every command clap knows about has an ontology entry.
    ///
    /// Walks clap's own parsed `Command` rather than scanning text, so a
    /// subcommand cannot be added without either declaring it here or being
    /// exempted below with a reason.
    ///
    /// This test exists because the ontology once declared 7 of 48 commands
    /// while presenting itself as the complete capability index. The 41 it
    /// omitted included `erase`, `clone` and `restore`, so an agent trusting
    /// it would have concluded abt cannot destroy anything.
    #[test]
    fn every_subcommand_is_declared() {
        let declared: std::collections::HashSet<String> =
            build_capabilities().into_iter().map(|c| c.id).collect();

        let command = crate::cli::Args::command();
        let missing: Vec<String> = command
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .filter(|name| !declared.contains(name))
            .collect();

        assert!(
            missing.is_empty(),
            "these commands exist but have no ontology entry: {missing:?}"
        );
    }

    /// The reverse: nothing is declared that cannot be run.
    ///
    /// Without this the ontology could drift the other way and offer an agent
    /// a command that errors when chosen, which is worse than omitting it --
    /// the agent has no way to tell a stale entry from a real one.
    #[test]
    fn nothing_declared_is_missing_from_the_cli() {
        let command = crate::cli::Args::command();
        let real: std::collections::HashSet<String> = command
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .collect();

        let phantom: Vec<String> = build_capabilities()
            .into_iter()
            .map(|c| c.id)
            .filter(|id| !real.contains(id))
            .collect();

        assert!(
            phantom.is_empty(),
            "these ontology entries name no real command: {phantom:?}"
        );
    }

    /// Anything that destroys data requires elevation, except where the data
    /// destroyed is the caller's own and needs no privilege to remove.
    ///
    /// A destructive operation reachable without privilege is the combination
    /// worth failing a build over, so the exemptions are listed here by name
    /// with a reason rather than being waved through by a weaker rule.
    #[test]
    fn destructive_capabilities_require_elevation() {
        // `cache clean` and `cache evict` delete downloaded images from the
        // user's own cache directory. That is destruction -- the bytes are
        // gone and must be re-downloaded -- but it needs no more privilege
        // than deleting any other file the user owns. Marking it
        // non-destructive to satisfy this rule would be the dishonest fix.
        const EXEMPT: &[&str] = &["cache"];

        let offenders: Vec<String> = build_capabilities()
            .into_iter()
            .filter(|c| c.destructive && !c.requires_elevation)
            .map(|c| c.id)
            .filter(|id| !EXEMPT.contains(&id.as_str()))
            .collect();

        assert!(
            offenders.is_empty(),
            "destructive without requiring elevation: {offenders:?}"
        );
    }
}
