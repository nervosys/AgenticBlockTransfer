// CLI module — argument parsing and command dispatch

pub mod commands;

use clap::{Parser, Subcommand, ValueEnum};

/// abt — AgenticBlockTransfer: agentic-first CLI block transfer tool.
///
/// Successor to UNIX dd (Dataset Definition) and IBM BLT (BLock Transfer).
/// Supports writing disk images (ISO, IMG, VHD, QCOW2, raw, and compressed variants)
/// to block devices (USB, SD, NVMe, eMMC, SPI flash). Provides verification, checksumming,
/// formatting, and device enumeration. Designed for use from microcontrollers to cloud servers.
///
/// Includes an AI-discoverable ontology for agentic system integration.
#[derive(Parser, Debug)]
#[command(
    name = "abt",
    version,
    about = "AgenticBlockTransfer — agentic CLI + human GUI/TUI block transfer tool",
    long_about = None,
    after_help = "Use 'abt ontology' to export the machine-readable capability ontology for AI agent integration."
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,

    /// Verbosity level (0=error, 1=warn, 2=info, 3=debug, 4=trace)
    #[arg(short, long, default_value = "2", global = true)]
    pub verbose: u8,

    /// Output format for structured data
    #[arg(short, long, default_value = "text", global = true)]
    pub output: OutputFormat,

    /// Write log output to a file (JSON-structured, one entry per line)
    #[arg(long, global = true)]
    pub log_file: Option<String>,

    /// Enable FIPS 140 compliance mode.
    /// Restricts to FIPS-approved algorithms only (SHA-256, SHA-512).
    /// Enforces TLS 1.2+, CSPRNG for erase, HTTPS-only downloads.
    /// Also settable via ABT_FIPS_MODE=1 environment variable.
    #[arg(long, global = true, env = "ABT_FIPS_MODE")]
    pub fips: bool,
}

impl Args {
    /// Convert verbosity level to log filter.
    pub fn log_level(&self) -> log::LevelFilter {
        match self.verbose {
            0 => log::LevelFilter::Error,
            1 => log::LevelFilter::Warn,
            2 => log::LevelFilter::Info,
            3 => log::LevelFilter::Debug,
            _ => log::LevelFilter::Trace,
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable text (default)
    Text,
    /// JSON for machine consumption
    Json,
    /// JSON-LD with semantic annotations
    JsonLd,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Write an image to a target device or file
    #[command(visible_alias = "flash", visible_alias = "dd")]
    Write(WriteOpts),

    /// Verify written data against source image or checksum
    Verify(VerifyOpts),

    /// List available block devices / storage targets
    #[command(visible_alias = "devices", visible_alias = "ls")]
    List(ListOpts),

    /// Show detailed information about a device or image file
    #[command(visible_alias = "inspect")]
    Info(InfoOpts),

    /// Compute checksums/hashes of files or devices
    #[command(visible_alias = "hash")]
    Checksum(ChecksumOpts),

    /// Format a device with a filesystem
    #[command(visible_alias = "mkfs")]
    Format(FormatOpts),

    /// Export the AI-discoverable ontology (JSON-LD / schema.org)
    #[command(visible_alias = "schema", visible_alias = "capabilities")]
    Ontology(OntologyOpts),

    /// Generate shell completions for bash, zsh, fish, or PowerShell
    Completions(CompletionsOpts),

    /// Generate man pages to a directory
    #[command(visible_alias = "manpage")]
    Man(ManOpts),

    /// Launch interactive TUI mode
    #[cfg(feature = "tui")]
    Tui,

    /// Launch graphical UI mode
    #[cfg(feature = "gui")]
    Gui,

    /// Start MCP (Model Context Protocol) server for AI agent integration
    #[command(visible_alias = "server")]
    Mcp(McpOpts),

    /// Clone a device or image to another device (block-level copy)
    #[command(visible_alias = "copy")]
    Clone(CloneOpts),

    /// Securely erase a device (zero-fill, random, ATA secure erase, NVMe sanitize)
    #[command(visible_alias = "wipe")]
    Erase(EraseOpts),

    /// Validate boot sector (MBR / GPT / UEFI) of a device or image
    #[command(visible_alias = "bootcheck")]
    Boot(BootOpts),

    /// Browse Raspberry Pi OS image catalog
    #[command(visible_alias = "rpi")]
    Catalog(CatalogOpts),

    /// Benchmark I/O throughput for block size selection and comparison
    #[command(visible_alias = "benchmark")]
    Bench(BenchOpts),

    /// Differential write — only write blocks that differ between source and target
    #[command(visible_alias = "incremental", visible_alias = "delta")]
    Diff(DiffOpts),

    /// Manage multi-boot USB device (add/remove/list ISOs, Ventoy-style)
    #[command(visible_alias = "ventoy")]
    Multiboot(MultibootOpts),

    /// Apply OS customization (hostname, SSH, WiFi, users) for firstrun or cloud-init
    #[command(visible_alias = "firstrun")]
    Customize(CustomizeOpts),

    /// Manage local image download cache (list, verify, clean, evict)
    #[command(visible_alias = "imgcache")]
    Cache(CacheOpts),

    /// Check drive health: bad blocks, fake flash detection, read test
    #[command(visible_alias = "badblocks")]
    Health(HealthOpts),

    /// Back up a drive or partition to a compressed image file
    #[command(visible_alias = "save")]
    Backup(BackupOpts),

    /// Create persistent storage partition for a live Linux USB
    #[command(visible_alias = "casper")]
    Persist(PersistOpts),

    /// Check for abt updates and optionally self-update
    #[command(visible_alias = "upgrade")]
    Update(UpdateOpts),

    /// Manage download mirrors: probe latency, failover, metalink
    #[command(visible_alias = "mirrors")]
    Mirror(MirrorOpts),

    /// Parse and verify checksum files (SHA256SUMS, MD5SUMS, etc.)
    #[command(visible_alias = "checksumfile")]
    ChecksumFile(ChecksumFileOpts),

    /// Show USB device speed info and write-time estimates
    #[command(visible_alias = "usbspeed")]
    UsbInfo(UsbInfoOpts),

    /// Verify signed downloads (RSA PKCS#1v1.5 signature verification)
    #[command(visible_alias = "sig")]
    Signature(SignatureOpts),

    /// Generate Windows Unattended Setup (unattend.xml) for automated installation
    #[command(visible_alias = "unattend")]
    Wue(WueOpts),

    /// Plan UEFI:NTFS dual-partition boot layout for large Windows ISOs
    #[command(visible_alias = "uefi")]
    UefiNtfs(UefiNtfsOpts),

    /// Multi-target fleet write - flash same image to multiple USB drives simultaneously
    #[command(visible_alias = "multi")]
    Fleet(FleetOpts),

    /// Restore a drive to factory state (wipe, repartition, format)
    #[command(visible_alias = "factory-reset")]
    Restore(RestoreOpts),

    /// View and export performance telemetry data
    #[command(visible_alias = "perf")]
    Telemetry(TelemetryOpts),

    /// Configure and test write watchdog (stall detection + recovery)
    #[command(visible_alias = "stall")]
    Watchdog(WatchdogOpts),

    /// Extract files from WIM archives (Windows installation images)
    #[command(visible_alias = "wimextract")]
    WimExtract(WimExtractOpts),

    /// Check Secure Boot status and verify bootloader signatures
    #[command(visible_alias = "sb")]
    SecureBoot(SecureBootOpts),

    /// Detect filesystem type from device or image superblock magic bytes
    #[command(visible_alias = "fstype")]
    FsDetect(FsDetectOpts),

    /// Scan for attached drives with hot-plug detection
    #[command(visible_alias = "drives")]
    DriveScan(DriveScanOpts),

    /// Validate drive compatibility and constraints for an operation
    #[command(visible_alias = "constraints")]
    DriveConstraints(DriveConstraintsOpts),

    /// Create Windows To Go bootable USB drives
    #[command(visible_alias = "wtg")]
    WinToGo(WinToGoOpts),

    /// Detect, plan, and configure Syslinux/GRUB bootloader installation
    #[command(visible_alias = "bootloader")]
    Syslinux(SyslinuxOpts),

    /// Parse and inspect FFU (Full Flash Update) images
    #[command(visible_alias = "fullflash")]
    Ffu(FfuOpts),

    /// Detect ISOHybrid (MBR/GPT embedded) ISO images and recommend write mode
    #[command(visible_alias = "hybrid")]
    IsoHybrid(IsoHybridOpts),

    /// Detect processes holding locks on target drives
    #[command(visible_alias = "locks")]
    ProcLock(ProcLockOpts),

    /// Check and manage privilege elevation
    #[command(visible_alias = "sudo")]
    Elevate(ElevateOpts),

    /// Read optical disc media (CD/DVD/Blu-ray) to ISO files
    #[command(visible_alias = "disc")]
    Optical(OpticalOpts),

    /// Run FIPS / CMMC 2.0 / DoD compliance self-assessment
    #[command(visible_alias = "audit")]
    Compliance(ComplianceOpts),
}

#[derive(Parser, Debug)]
pub struct McpOpts {
    /// Process a single request and exit (instead of running as a persistent server)
    #[arg(long)]
    pub oneshot: bool,
}

#[derive(Parser, Debug)]
pub struct WriteOpts {
    /// Source image file path or URL
    #[arg(short = 'i', long = "input", alias = "source")]
    pub source: String,

    /// Target device path (e.g., /dev/sdb, \\.\PhysicalDrive1)
    #[arg(short = 'o', long = "output", alias = "target")]
    pub target: String,

    /// Block size for I/O operations (e.g., 4M, 1M, 512K)
    #[arg(short = 'b', long, default_value = "4M")]
    pub block_size: String,

    /// Verify after writing by reading back and comparing
    #[arg(long, default_value = "true")]
    pub verify: bool,

    /// Skip verification after writing
    #[arg(long, conflicts_with = "verify")]
    pub no_verify: bool,

    /// Expected hash of the source image (for integrity check before writing)
    #[arg(long)]
    pub expected_hash: Option<String>,

    /// Hash algorithm for verification
    #[arg(long, default_value = "sha256")]
    pub hash_algorithm: String,

    /// Force write without safety confirmation prompts
    #[arg(short = 'f', long)]
    pub force: bool,

    /// Use direct/unbuffered I/O (bypass OS cache)
    #[arg(long, default_value = "true")]
    pub direct_io: bool,

    /// Sync device after writing
    #[arg(long, default_value = "true")]
    pub sync: bool,

    /// Auto-detect and decompress compressed images
    #[arg(long, default_value = "true")]
    pub decompress: bool,

    /// Sparse write: skip all-zero blocks by seeking past them (like dd conv=sparse).
    /// Dramatically speeds up images with large empty regions.
    #[arg(long)]
    pub sparse: bool,

    /// Write mode: raw (dd-style), extract (ISO contents), clone (device clone)
    #[arg(long, default_value = "raw")]
    pub mode: String,

    /// Safety level: low, medium (recommended for agents), high (max)
    #[arg(long, default_value = "low")]
    pub safety_level: String,

    /// Dry-run: run pre-flight safety checks without writing anything
    #[arg(long)]
    pub dry_run: bool,

    /// Device confirmation token from `abt list --json` (prevents TOCTOU between list and write)
    #[arg(long)]
    pub confirm_token: Option<String>,

    /// Back up partition table (first 1 MiB) before writing
    #[arg(long)]
    pub backup_partition_table: bool,
}

#[derive(Parser, Debug)]
pub struct VerifyOpts {
    /// Source image file for comparison
    #[arg(short = 'i', long = "input", alias = "source")]
    pub source: Option<String>,

    /// Target device or file to verify
    #[arg(short = 'o', long = "output", alias = "target")]
    pub target: String,

    /// Expected hash value (alternative to source file comparison)
    #[arg(long)]
    pub expected_hash: Option<String>,

    /// Hash algorithm
    #[arg(long, default_value = "sha256")]
    pub hash_algorithm: String,
}

#[derive(Parser, Debug)]
pub struct ListOpts {
    /// Show all devices including system drives
    #[arg(long)]
    pub all: bool,

    /// Filter by device type (usb, sd, nvme, sata, etc.)
    #[arg(short = 't', long = "type")]
    pub device_type: Option<String>,

    /// Show only removable devices
    #[arg(long)]
    pub removable: bool,

    /// Output as JSON with device fingerprints (for agent use)
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct InfoOpts {
    /// Device path or image file path to inspect
    pub path: String,
}

#[derive(Parser, Debug)]
pub struct ChecksumOpts {
    /// File or device to hash
    pub path: String,

    /// Hash algorithm(s) to compute
    #[arg(short = 'a', long, default_value = "sha256")]
    pub algorithm: Vec<String>,
}

#[derive(Parser, Debug)]
pub struct FormatOpts {
    /// Device to format
    pub device: String,

    /// Filesystem type
    #[arg(short = 'f', long = "fs", alias = "filesystem")]
    pub filesystem: String,

    /// Volume label
    #[arg(short = 'l', long)]
    pub label: Option<String>,

    /// Quick format
    #[arg(short = 'q', long)]
    pub quick: bool,

    /// Force without confirmation
    #[arg(long)]
    pub force: bool,
}

#[derive(Parser, Debug)]
pub struct OntologyOpts {
    /// Output format: json-ld, json, yaml, openapi, openapi-yaml
    #[arg(short = 'f', long, default_value = "json-ld")]
    pub format: String,

    /// Include full parameter schemas
    #[arg(long)]
    pub full: bool,

    /// Filter by capability category
    #[arg(long)]
    pub category: Option<String>,
}

#[derive(Parser, Debug)]
pub struct CompletionsOpts {
    /// Shell to generate completions for
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
}

#[derive(Parser, Debug)]
pub struct ManOpts {
    /// Output directory for generated man pages
    #[arg(short, long, default_value = ".")]
    pub output_dir: String,
}

#[derive(Parser, Debug)]
pub struct CloneOpts {
    /// Source device or image path
    #[arg(short = 'i', long = "input", alias = "source")]
    pub source: String,

    /// Target device path
    #[arg(short = 'o', long = "output", alias = "target")]
    pub target: String,

    /// Block size for I/O operations
    #[arg(short = 'b', long, default_value = "4M")]
    pub block_size: String,

    /// Verify clone by reading back and comparing hashes
    #[arg(long, default_value = "true")]
    pub verify: bool,

    /// Hash algorithm for verification
    #[arg(long, default_value = "sha256")]
    pub hash_algorithm: String,

    /// Skip zero blocks (sparse clone)
    #[arg(long)]
    pub sparse: bool,

    /// Force without confirmation
    #[arg(short = 'f', long)]
    pub force: bool,
}

#[derive(Parser, Debug)]
pub struct EraseOpts {
    /// Device to erase
    pub device: String,

    /// Erase method: auto, zero, random, ata, nvme, discard
    #[arg(short = 'm', long, default_value = "auto")]
    pub method: String,

    /// Number of overwrite passes (1-35)
    #[arg(short = 'p', long, default_value = "1")]
    pub passes: u32,

    /// Force without confirmation
    #[arg(short = 'f', long)]
    pub force: bool,
}

#[derive(Parser, Debug)]
pub struct BootOpts {
    /// Device or image file to validate
    pub path: String,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct CatalogOpts {
    /// Search filter (matches OS name/description)
    #[arg(short = 's', long)]
    pub search: Option<String>,

    /// Show only directly downloadable entries (skip categories)
    #[arg(long)]
    pub flat: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct BenchOpts {
    /// Target file or device to benchmark
    pub target: String,

    /// Test size in MiB (default: 64)
    #[arg(short = 's', long, default_value = "64")]
    pub test_size: u64,

    /// Block sizes to test (e.g., 4K 64K 1M 4M). If omitted, uses defaults (4K-16M).
    #[arg(short = 'b', long)]
    pub block_sizes: Vec<String>,

    /// Number of iterations per block size
    #[arg(short = 'n', long, default_value = "3")]
    pub iterations: u32,

    /// Only benchmark reads
    #[arg(long)]
    pub read_only: bool,

    /// Only benchmark writes
    #[arg(long)]
    pub write_only: bool,

    /// Use direct I/O (O_DIRECT / FILE_FLAG_NO_BUFFERING)
    #[arg(long)]
    pub direct_io: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct DiffOpts {
    /// Source image file
    #[arg(short = 'i', long = "input", alias = "source")]
    pub source: String,

    /// Target device or file
    #[arg(short = 'o', long = "output", alias = "target")]
    pub target: String,

    /// Block size for comparison
    #[arg(short = 'b', long, default_value = "4M")]
    pub block_size: String,

    /// Skip verification after writing
    #[arg(long)]
    pub no_verify: bool,

    /// Dry run: report changes without writing
    #[arg(long)]
    pub dry_run: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct MultibootOpts {
    /// Action: add, remove, list, grub
    pub action: String,

    /// Mount point of the multi-boot USB data partition
    #[arg(short = 'm', long)]
    pub mount_point: Option<String>,

    /// ISO file path (for add/remove)
    #[arg(short = 'i', long)]
    pub iso: Option<String>,

    /// Display name for the boot menu entry (for add)
    #[arg(short = 'n', long)]
    pub name: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct CustomizeOpts {
    /// Action: generate, detect-wifi, detect-ssh, save-preset, load-preset
    pub action: String,

    /// Output directory for generated files (firstrun.sh, cloud-init, etc.)
    #[arg(short = 'd', long, default_value = ".")]
    pub output_dir: String,

    /// Hostname to set on first boot
    #[arg(long)]
    pub hostname: Option<String>,

    /// Username to create
    #[arg(long)]
    pub username: Option<String>,

    /// Password for the new user (will be prompted if omitted)
    #[arg(long)]
    pub password: Option<String>,

    /// Enable SSH with password authentication
    #[arg(long)]
    pub enable_ssh: bool,

    /// SSH public key to authorize
    #[arg(long)]
    pub ssh_key: Option<String>,

    /// WiFi SSID to configure
    #[arg(long)]
    pub wifi_ssid: Option<String>,

    /// WiFi password
    #[arg(long)]
    pub wifi_password: Option<String>,

    /// WiFi country code (e.g., US, GB, DE)
    #[arg(long)]
    pub wifi_country: Option<String>,

    /// Timezone (e.g., America/New_York)
    #[arg(long)]
    pub timezone: Option<String>,

    /// Locale (e.g., en_US.UTF-8)
    #[arg(long)]
    pub locale: Option<String>,

    /// Output format: firstrun, cloud-init, network-config
    #[arg(short = 'f', long, default_value = "firstrun")]
    pub format: String,

    /// Preset file to save or load (JSON)
    #[arg(long)]
    pub preset: Option<String>,
}

#[derive(Parser, Debug)]
pub struct CacheOpts {
    /// Action: list, verify, clean, evict, stats, clear
    pub action: String,

    /// Cache directory (default: ~/.cache/abt/images)
    #[arg(short = 'd', long)]
    pub cache_dir: Option<String>,

    /// URL to look up in cache
    #[arg(long)]
    pub url: Option<String>,

    /// Eviction policy: max-age, max-entries, max-size
    #[arg(long)]
    pub evict_policy: Option<String>,

    /// Eviction threshold value (days for max-age, count for max-entries, GiB for max-size)
    #[arg(long)]
    pub evict_threshold: Option<u64>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct HealthOpts {
    /// Device or file to check
    pub device: String,

    /// Check type: badblocks, fake, quick, full
    #[arg(short = 't', long = "test", default_value = "quick")]
    pub test_type: String,

    /// Test pattern: quick (1 pass), standard (2), slc (2), mlc (4), tlc (8)
    #[arg(short = 'p', long, default_value = "quick")]
    pub pattern: String,

    /// Block size for bad block testing (e.g., 128K, 1M)
    #[arg(short = 'b', long, default_value = "128K")]
    pub block_size: String,

    /// Force destructive bad block test without confirmation
    #[arg(short = 'f', long)]
    pub force: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct BackupOpts {
    /// Source device or partition to back up
    #[arg(short = 'i', long = "input", alias = "source")]
    pub source: String,

    /// Output image file path (auto-named with timestamp if omitted)
    #[arg(short = 'o', long = "output")]
    pub output: Option<String>,

    /// Compression: none, gzip, zstd, bzip2, xz
    #[arg(short = 'c', long, default_value = "zstd")]
    pub compression: String,

    /// Compression level (format-specific)
    #[arg(long)]
    pub compression_level: Option<i32>,

    /// Block size for read I/O
    #[arg(short = 'b', long, default_value = "4M")]
    pub block_size: String,

    /// Skip zero blocks (sparse backup)
    #[arg(long)]
    pub sparse: bool,

    /// Compute SHA-256 hash of the raw data during backup
    #[arg(long, default_value = "true")]
    pub compute_hash: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct PersistOpts {
    /// Target device containing the live Linux image
    pub device: String,

    /// Size of the persistent partition (e.g., 4G, 1G). 0 = use all remaining space.
    #[arg(short = 's', long, default_value = "0")]
    pub size: String,

    /// Filesystem for persistent partition: ext4, ext3, btrfs
    #[arg(short = 'f', long = "fs", default_value = "ext4")]
    pub filesystem: String,

    /// Partition label (default: auto-detect from mode)
    #[arg(short = 'l', long)]
    pub label: Option<String>,

    /// Persistence mode: casper, fedora, generic, ventoy
    #[arg(short = 'm', long, default_value = "casper")]
    pub mode: String,

    /// Encrypt the persistent partition with LUKS
    #[arg(long)]
    pub encrypt: bool,

    /// Force without confirmation
    #[arg(long)]
    pub force: bool,

    /// Create a persistence image file instead of a partition
    #[arg(long)]
    pub image_file: Option<String>,
}
#[derive(Parser, Debug)]
pub struct UpdateOpts {
    /// Force update check (ignore cache interval)
    #[arg(short = 'f', long)]
    pub force: bool,

    /// Include pre-release versions
    #[arg(long)]
    pub prerelease: bool,

    /// GitHub repo (owner/name) to check for updates
    #[arg(long, default_value = "nervosys/AgenticBlockTransfer")]
    pub repo: String,

    /// Dismiss the current latest version (hide until a newer version)
    #[arg(long)]
    pub dismiss: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct MirrorOpts {
    /// Action: probe, download, list
    pub action: String,

    /// URL of a JSON mirror list to fetch
    #[arg(short = 'm', long)]
    pub mirror_list: Option<String>,

    /// Metalink (.meta4 / .metalink) URL or file path
    #[arg(long)]
    pub metalink: Option<String>,

    /// File path or URL to download (for download action)
    #[arg(short = 'p', long)]
    pub path: Option<String>,

    /// Output directory for downloads
    #[arg(short = 'o', long, default_value = ".")]
    pub output_dir: String,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct ChecksumFileOpts {
    /// Path or URL of the checksum file (e.g., SHA256SUMS)
    pub checksum_file: String,

    /// Files to verify against the checksum file
    #[arg(short = 'f', long)]
    pub files: Vec<String>,

    /// Look up a specific filename in the checksum file
    #[arg(short = 'l', long)]
    pub lookup: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct UsbInfoOpts {
    /// Device path to inspect (e.g., /dev/sdb, \\.\PhysicalDrive1)
    pub device: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct SignatureOpts {
    /// File to verify or hash
    pub file: String,

    /// Detached signature file (.sig or .asc)
    #[arg(short = 's', long)]
    pub signature: Option<String>,

    /// Key ring file (JSON) for verification
    #[arg(short = 'k', long)]
    pub keyring: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct WueOpts {
    /// Username for the local account
    #[arg(short = 'u', long, default_value = "User")]
    pub username: String,

    /// Password for the local account
    #[arg(short = 'p', long)]
    pub password: Option<String>,

    /// Target architecture: amd64, x86, arm64
    #[arg(short = 'a', long, default_value = "amd64")]
    pub arch: String,

    /// Generate for Windows 10 (no hardware bypasses)
    #[arg(long)]
    pub win10: bool,

    /// Timezone
    #[arg(long)]
    pub timezone: Option<String>,

    /// UI language / locale
    #[arg(long)]
    pub locale: Option<String>,

    /// Computer name
    #[arg(long)]
    pub computer_name: Option<String>,

    /// Product key
    #[arg(long)]
    pub product_key: Option<String>,

    /// Disable hardware bypasses (even for Windows 11)
    #[arg(long)]
    pub no_bypass: bool,

    /// Output file path (default: stdout)
    #[arg(short = 'o', long)]
    pub output: Option<String>,
}

#[derive(Parser, Debug)]
pub struct UefiNtfsOpts {
    /// Action: analyze, plan
    pub action: String,

    /// Path to directory or mount point (for analyze)
    #[arg(short = 'p', long)]
    pub path: Option<String>,

    /// Disk size in GB (for plan, default: 16)
    #[arg(long)]
    pub disk_size_gb: Option<u64>,

    /// Boot mode: uefi, bios, dual
    #[arg(long)]
    pub boot_mode: Option<String>,

    /// Whether source has files > 4 GB
    #[arg(long)]
    pub large_files: bool,

    /// Windows-To-Go mode
    #[arg(long)]
    pub wtg: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct FleetOpts {
    /// Action: detect, validate, status
    pub action: String,

    /// Source image path (for validate)
    #[arg(short = 's', long)]
    pub source: Option<String>,

    /// Target device paths (for validate; can be repeated)
    #[arg(short = 't', long)]
    pub targets: Vec<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct RestoreOpts {
    /// Action: plan, execute
    pub action: String,

    /// Device path to restore (e.g., /dev/sdb, \\.\PhysicalDrive1)
    #[arg(short = 'd', long)]
    pub device: String,

    /// Partition table type: gpt, mbr, auto
    #[arg(short = 't', long = "table", default_value = "auto")]
    pub table_type: String,

    /// Filesystem: exfat, fat32, ntfs, ext4, btrfs, xfs
    #[arg(short = 'f', long = "fs", default_value = "exfat")]
    pub filesystem: String,

    /// Volume label
    #[arg(short = 'l', long, default_value = "USB DRIVE")]
    pub label: String,

    /// Skip partition table wipe
    #[arg(long)]
    pub no_wipe: bool,

    /// Full format (not quick)
    #[arg(long)]
    pub full: bool,

    /// Force without confirmation
    #[arg(long)]
    pub force: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct TelemetryOpts {
    /// Action: show, demo, export
    pub action: String,

    /// Telemetry report file (JSON)
    #[arg(short = 'f', long)]
    pub file: Option<String>,

    /// Output file path (for export)
    #[arg(long = "output-file")]
    pub output_file: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct WatchdogOpts {
    /// Action: show, config, test, simulate
    pub action: String,

    /// Configuration preset: default, lenient, strict
    #[arg(short = 'p', long)]
    pub preset: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct WimExtractOpts {
    /// Action: info, list, extract, check
    pub action: String,

    /// WIM file path
    #[arg(short = 'w', long = "wim")]
    pub wim_file: String,

    /// Image index (1-based, for multi-edition WIMs)
    #[arg(short = 'i', long, default_value = "1")]
    pub image_index: u32,

    /// Output directory (for extract)
    #[arg(short = 'o', long)]
    pub output_dir: Option<String>,

    /// Include patterns (glob)
    #[arg(long)]
    pub include: Vec<String>,

    /// Exclude patterns (glob)
    #[arg(long)]
    pub exclude: Vec<String>,

    /// Overwrite existing files
    #[arg(long)]
    pub overwrite: bool,

    /// Flatten directory structure
    #[arg(long)]
    pub flatten: bool,

    /// Max files to extract (0 = unlimited)
    #[arg(long, default_value = "0")]
    pub max_files: u64,

    /// Dry run (list only, don't extract)
    #[arg(long)]
    pub dry_run: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct SecureBootOpts {
    /// Action: status, check-file, bootloaders
    pub action: String,

    /// File to check (for check-file/verify)
    #[arg(short = 'f', long)]
    pub file: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct FsDetectOpts {
    /// Action: detect, probe
    pub action: String,

    /// Device or image path to detect filesystem on
    #[arg(short = 'd', long)]
    pub device: String,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct DriveScanOpts {
    /// Action: scan, watch, snapshot
    pub action: String,

    /// Include system/boot drives
    #[arg(long)]
    pub include_system: bool,

    /// Include read-only drives
    #[arg(long)]
    pub include_readonly: bool,

    /// Minimum drive size in bytes
    #[arg(long)]
    pub min_size: Option<u64>,

    /// Maximum drive size in bytes
    #[arg(long)]
    pub max_size: Option<u64>,

    /// Poll interval in milliseconds (for watch)
    #[arg(long)]
    pub poll_interval: Option<u64>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct DriveConstraintsOpts {
    /// Action: validate, auto-select, check-all
    pub action: String,

    /// Device path (for validate)
    #[arg(short = 'd', long)]
    pub device: Option<String>,

    /// Source image path (for size checking)
    #[arg(short = 's', long)]
    pub source: Option<String>,

    /// Minimum required size in bytes
    #[arg(long)]
    pub min_size: Option<u64>,

    /// Allow system drive selection
    #[arg(long)]
    pub allow_system: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct WinToGoOpts {
    /// Action: analyze, plan, check-drive, san-policy
    pub action: String,

    /// Windows ISO path
    #[arg(short = 'i', long)]
    pub iso: Option<String>,

    /// Target device path
    #[arg(short = 'd', long)]
    pub device: Option<String>,

    /// Partition scheme: gpt, mbr
    #[arg(long)]
    pub scheme: Option<String>,

    /// Enable UEFI:NTFS support partition
    #[arg(long)]
    pub uefi_ntfs: bool,

    /// Skip SAN policy
    #[arg(long)]
    pub no_san_policy: bool,

    /// Skip recovery partition
    #[arg(long)]
    pub no_recovery: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct SyslinuxOpts {
    /// Action: detect, version, plan, config, types
    pub action: String,

    /// File paths for bootloader detection
    #[arg(long)]
    pub files: Vec<String>,

    /// Single file path (for version detection)
    #[arg(short = 'f', long)]
    pub file: Option<String>,

    /// Bootloader type: syslinux-v4, syslinux-v6, isolinux, extlinux, grub2, grub4dos
    #[arg(short = 'b', long)]
    pub bootloader: Option<String>,

    /// Target filesystem (for plan)
    #[arg(long = "fs")]
    pub filesystem: Option<String>,

    /// Boot label (for config generation)
    #[arg(short = 'l', long)]
    pub label: Option<String>,

    /// Kernel path (for config generation)
    #[arg(short = 'k', long)]
    pub kernel: Option<String>,

    /// Initrd path (for config generation)
    #[arg(long)]
    pub initrd: Option<String>,

    /// Append line (for config generation)
    #[arg(short = 'a', long)]
    pub append: Option<String>,

    /// Menu title (for config generation)
    #[arg(short = 't', long)]
    pub title: Option<String>,

    /// Boot timeout in 1/10 seconds (for config generation)
    #[arg(long)]
    pub timeout: Option<u32>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct FfuOpts {
    /// Action: info, detect, manifest
    pub action: String,

    /// FFU image file path
    #[arg(short = 'f', long)]
    pub file: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct IsoHybridOpts {
    /// Action: detect, mode
    pub action: String,

    /// ISO image file path
    #[arg(short = 'f', long)]
    pub file: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct ProcLockOpts {
    /// Action: scan, busy, report
    pub action: String,

    /// Device or path to check for locks
    #[arg(short = 'd', long)]
    pub device: String,

    /// Include system/kernel processes
    #[arg(long)]
    pub include_system: bool,

    /// Don't resolve mount points to device paths
    #[arg(long)]
    pub no_resolve_mounts: bool,

    /// Scan timeout in milliseconds
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct ElevateOpts {
    /// Action: status, run, methods
    pub action: String,

    /// Elevation method: uac, pkexec, sudo, osascript
    #[arg(short = 'm', long)]
    pub method: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct OpticalOpts {
    /// Action: list, info, read
    pub action: String,

    /// Optical drive device path (e.g., /dev/sr0, \\\\.\CdRom0)
    #[arg(short = 'd', long)]
    pub device: Option<String>,

    /// Output ISO file path (for read action)
    #[arg(short = 'o', long)]
    pub output: Option<String>,

    /// Number of sectors per read buffer (default: 64)
    #[arg(long)]
    pub buffer_sectors: Option<u32>,

    /// Max retries per sector on read error
    #[arg(long)]
    pub retries: Option<u32>,

    /// Skip unreadable sectors (fill with zeros)
    #[arg(long)]
    pub skip_errors: bool,

    /// Skip SHA-256 verification of output
    #[arg(long)]
    pub no_verify: bool,

    /// Overwrite existing output file
    #[arg(long)]
    pub overwrite: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct ComplianceOpts {
    /// Output as JSON for automated processing or SIEM integration
    #[arg(long)]
    pub json: bool,

    /// Save audit log to the specified directory
    #[arg(long)]
    pub save_audit_log: Option<String>,
}

/// Parse a human-readable block size string into bytes.
pub fn parse_block_size(s: &str) -> anyhow::Result<usize> {
    let s = s.trim().to_uppercase();
    if let Some(num) = s.strip_suffix('K') {
        Ok(num.parse::<usize>()? * 1024)
    } else if let Some(num) = s.strip_suffix('M') {
        Ok(num.parse::<usize>()? * 1024 * 1024)
    } else if let Some(num) = s.strip_suffix('G') {
        Ok(num.parse::<usize>()? * 1024 * 1024 * 1024)
    } else {
        Ok(s.parse::<usize>()?)
    }
}
