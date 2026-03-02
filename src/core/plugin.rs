// Plugin / Extension system — trait-based API for custom format handlers
//
// Provides a registration-based mechanism for adding custom image format
// readers, verifiers, and post-write hooks. Built-in formats (ISO, QCOW2,
// VHD, VMDK, WIM, etc.) are registered as default plugins; users can add
// their own by implementing the `FormatPlugin` trait.
//
// The plugin registry uses a simple Vec-based approach with Arc for
// thread safety. No dynamic library loading (yet) — plugins are compiled in.

#![allow(dead_code)]

use anyhow::Result;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use super::types::ImageFormat;

/// Trait for custom image format plugins.
///
/// Implement this to add support for a new image format. The plugin system
/// queries all registered plugins via `can_handle()`, then uses the first
/// match to open the image.
pub trait FormatPlugin: Send + Sync {
    /// Display name of this plugin (e.g., "QCOW2 Reader").
    fn name(&self) -> &str;

    /// Short description of what this plugin handles.
    fn description(&self) -> &str;

    /// File extensions this plugin can handle (lowercase, without dot).
    fn extensions(&self) -> &[&str];

    /// Check whether this plugin can handle the given format and/or magic bytes.
    /// `magic` is the first 16 bytes of the file.
    fn can_handle(&self, format: ImageFormat, magic: &[u8; 16]) -> bool;

    /// Open the image and return a boxed reader that emits raw data.
    /// The reader should transparently convert the format to raw block data.
    fn open(&self, path: &Path) -> Result<Box<dyn Read + Send>>;

    /// Return metadata about the image as key-value pairs.
    /// Used by `abt info` to display format-specific details.
    fn metadata(&self, path: &Path) -> Result<Vec<(String, String)>> {
        let _ = path;
        Ok(Vec::new())
    }

    /// Priority (higher = checked first). Built-in plugins use 0-99,
    /// user plugins should use 100+.
    fn priority(&self) -> u32 {
        100
    }
}

/// Registry of format plugins.
#[derive(Clone)]
pub struct PluginRegistry {
    plugins: Vec<Arc<dyn FormatPlugin>>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Create a registry pre-loaded with all built-in format plugins.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register(Arc::new(Qcow2Plugin));
        registry.register(Arc::new(VhdPlugin));
        registry.register(Arc::new(VmdkPlugin));
        registry.register(Arc::new(WimPlugin));
        registry
    }

    /// Register a new format plugin.
    pub fn register(&mut self, plugin: Arc<dyn FormatPlugin>) {
        self.plugins.push(plugin);
        // Sort by priority (descending — higher priority first)
        self.plugins
            .sort_by(|a, b| b.priority().cmp(&a.priority()));
    }

    /// Find the first plugin that can handle the given format + magic bytes.
    pub fn find_handler(
        &self,
        format: ImageFormat,
        magic: &[u8; 16],
    ) -> Option<&dyn FormatPlugin> {
        self.plugins
            .iter()
            .find(|p| p.can_handle(format, magic))
            .map(|p| p.as_ref())
    }

    /// Try to open an image using registered plugins.
    /// Returns `None` if no plugin can handle the format.
    pub fn try_open(&self, path: &Path) -> Result<Option<Box<dyn Read + Send>>> {
        // Read magic bytes
        let mut magic = [0u8; 16];
        if let Ok(mut f) = std::fs::File::open(path) {
            use std::io::Read;
            let _ = f.read(&mut magic);
        }

        // Detect format
        let format = super::image::detect_format(path)?;

        if let Some(plugin) = self.find_handler(format, &magic) {
            log::info!(
                "Plugin '{}' handling format {} for {}",
                plugin.name(),
                format,
                path.display()
            );
            Ok(Some(plugin.open(path)?))
        } else {
            Ok(None)
        }
    }

    /// Get metadata for an image from the best matching plugin.
    pub fn get_metadata(&self, path: &Path) -> Result<Option<Vec<(String, String)>>> {
        let mut magic = [0u8; 16];
        if let Ok(mut f) = std::fs::File::open(path) {
            use std::io::Read;
            let _ = f.read(&mut magic);
        }

        let format = super::image::detect_format(path)?;

        if let Some(plugin) = self.find_handler(format, &magic) {
            Ok(Some(plugin.metadata(path)?))
        } else {
            Ok(None)
        }
    }

    /// List all registered plugins.
    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        self.plugins
            .iter()
            .map(|p| PluginInfo {
                name: p.name().to_string(),
                description: p.description().to_string(),
                extensions: p.extensions().iter().map(|s| s.to_string()).collect(),
                priority: p.priority(),
            })
            .collect()
    }
}

/// Serializable plugin information for listing.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginInfo {
    pub name: String,
    pub description: String,
    pub extensions: Vec<String>,
    pub priority: u32,
}

// ─── Built-in plugins ──────────────────────────────────────────────────────

/// Built-in QCOW2 format plugin.
struct Qcow2Plugin;

impl FormatPlugin for Qcow2Plugin {
    fn name(&self) -> &str {
        "QCOW2 Reader"
    }

    fn description(&self) -> &str {
        "QEMU Copy-On-Write v2/v3 disk image reader"
    }

    fn extensions(&self) -> &[&str] {
        &["qcow2"]
    }

    fn can_handle(&self, format: ImageFormat, _magic: &[u8; 16]) -> bool {
        format == ImageFormat::Qcow2
    }

    fn open(&self, path: &Path) -> Result<Box<dyn Read + Send>> {
        let reader = super::qcow2::open_qcow2(path)?;
        Ok(Box::new(reader))
    }

    fn metadata(&self, path: &Path) -> Result<Vec<(String, String)>> {
        let header = super::qcow2::parse_qcow2(path)?;
        Ok(vec![
            ("Format".into(), format!("QCOW2 v{}", header.version)),
            (
                "Virtual Size".into(),
                humansize::format_size(header.size, humansize::BINARY),
            ),
            (
                "Cluster Size".into(),
                format!("{} KiB", header.cluster_size() / 1024),
            ),
            ("L1 Entries".into(), header.l1_size.to_string()),
            (
                "Backing File".into(),
                if header.has_backing_file() {
                    "Yes"
                } else {
                    "No"
                }
                .into(),
            ),
        ])
    }

    fn priority(&self) -> u32 {
        10
    }
}

/// Built-in VHD format plugin.
struct VhdPlugin;

impl FormatPlugin for VhdPlugin {
    fn name(&self) -> &str {
        "VHD Reader"
    }

    fn description(&self) -> &str {
        "Microsoft Virtual Hard Disk (legacy) reader"
    }

    fn extensions(&self) -> &[&str] {
        &["vhd"]
    }

    fn can_handle(&self, format: ImageFormat, _magic: &[u8; 16]) -> bool {
        format == ImageFormat::Vhd
    }

    fn open(&self, path: &Path) -> Result<Box<dyn Read + Send>> {
        let reader = super::vhd::open_vhd(path)?;
        Ok(Box::new(reader))
    }

    fn metadata(&self, path: &Path) -> Result<Vec<(String, String)>> {
        let footer = super::vhd::parse_vhd(path)?;
        Ok(vec![
            ("Format".into(), "VHD".into()),
            ("Summary".into(), footer.summary()),
        ])
    }

    fn priority(&self) -> u32 {
        10
    }
}

/// Built-in VMDK format plugin.
struct VmdkPlugin;

impl FormatPlugin for VmdkPlugin {
    fn name(&self) -> &str {
        "VMDK Reader"
    }

    fn description(&self) -> &str {
        "VMware Virtual Machine Disk sparse extent reader"
    }

    fn extensions(&self) -> &[&str] {
        &["vmdk"]
    }

    fn can_handle(&self, format: ImageFormat, _magic: &[u8; 16]) -> bool {
        format == ImageFormat::Vmdk
    }

    fn open(&self, path: &Path) -> Result<Box<dyn Read + Send>> {
        let reader = super::vmdk::open_vmdk(path)?;
        Ok(Box::new(reader))
    }

    fn metadata(&self, path: &Path) -> Result<Vec<(String, String)>> {
        let header = super::vmdk::parse_vmdk(path)?;
        Ok(vec![
            ("Format".into(), format!("VMDK v{}", header.version)),
            (
                "Virtual Size".into(),
                humansize::format_size(header.virtual_size(), humansize::BINARY),
            ),
            (
                "Grain Size".into(),
                format!("{} KiB", header.grain_size() / 1024),
            ),
            (
                "Compressed".into(),
                if header.is_compressed() {
                    "deflate"
                } else {
                    "none"
                }
                .into(),
            ),
        ])
    }

    fn priority(&self) -> u32 {
        10
    }
}

/// Built-in WIM format plugin (metadata only — no block-level streaming).
struct WimPlugin;

impl FormatPlugin for WimPlugin {
    fn name(&self) -> &str {
        "WIM Parser"
    }

    fn description(&self) -> &str {
        "Windows Imaging Format metadata parser"
    }

    fn extensions(&self) -> &[&str] {
        &["wim"]
    }

    fn can_handle(&self, format: ImageFormat, _magic: &[u8; 16]) -> bool {
        format == ImageFormat::Wim
    }

    fn open(&self, _path: &Path) -> Result<Box<dyn Read + Send>> {
        anyhow::bail!(
            "WIM files cannot be written as raw block images. \
             Use extraction mode (--mode extract) instead."
        )
    }

    fn metadata(&self, path: &Path) -> Result<Vec<(String, String)>> {
        let header = super::wim::parse_wim(path)?;
        Ok(vec![
            (
                "Format".into(),
                format!("WIM v{}.{}", header.version_major, header.version_minor),
            ),
            ("Images".into(), header.image_count.to_string()),
            (
                "Compression".into(),
                header.flags.compression_name().into(),
            ),
            ("Bootable".into(), header.is_bootable().to_string()),
            ("GUID".into(), header.guid_string()),
        ])
    }

    fn priority(&self) -> u32 {
        10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = PluginRegistry::new();
        assert_eq!(registry.list_plugins().len(), 0);
    }

    #[test]
    fn test_registry_with_builtins() {
        let registry = PluginRegistry::with_builtins();
        let plugins = registry.list_plugins();
        assert_eq!(plugins.len(), 4);
        assert!(plugins.iter().any(|p| p.name == "QCOW2 Reader"));
        assert!(plugins.iter().any(|p| p.name == "VHD Reader"));
        assert!(plugins.iter().any(|p| p.name == "VMDK Reader"));
        assert!(plugins.iter().any(|p| p.name == "WIM Parser"));
    }

    #[test]
    fn test_find_handler_qcow2() {
        let registry = PluginRegistry::with_builtins();
        let magic = [0u8; 16];
        let handler = registry.find_handler(ImageFormat::Qcow2, &magic);
        assert!(handler.is_some());
        assert_eq!(handler.unwrap().name(), "QCOW2 Reader");
    }

    #[test]
    fn test_find_handler_vmdk() {
        let registry = PluginRegistry::with_builtins();
        let magic = [0u8; 16];
        let handler = registry.find_handler(ImageFormat::Vmdk, &magic);
        assert!(handler.is_some());
        assert_eq!(handler.unwrap().name(), "VMDK Reader");
    }

    #[test]
    fn test_find_handler_unknown() {
        let registry = PluginRegistry::with_builtins();
        let magic = [0u8; 16];
        let handler = registry.find_handler(ImageFormat::Raw, &magic);
        assert!(handler.is_none());
    }

    #[test]
    fn test_custom_plugin_registration() {
        struct CustomPlugin;
        impl FormatPlugin for CustomPlugin {
            fn name(&self) -> &str {
                "Custom Reader"
            }
            fn description(&self) -> &str {
                "A custom format"
            }
            fn extensions(&self) -> &[&str] {
                &["custom"]
            }
            fn can_handle(&self, _format: ImageFormat, magic: &[u8; 16]) -> bool {
                magic[0] == 0xAB && magic[1] == 0xCD
            }
            fn open(&self, _path: &Path) -> Result<Box<dyn Read + Send>> {
                Ok(Box::new(std::io::empty()))
            }
            fn priority(&self) -> u32 {
                200 // higher than builtins
            }
        }

        let mut registry = PluginRegistry::with_builtins();
        registry.register(Arc::new(CustomPlugin));
        assert_eq!(registry.list_plugins().len(), 5);

        // Custom plugin should be first (highest priority)
        let plugins = registry.list_plugins();
        assert_eq!(plugins[0].name, "Custom Reader");
        assert_eq!(plugins[0].priority, 200);
    }

    #[test]
    fn test_plugin_priority_ordering() {
        let registry = PluginRegistry::with_builtins();
        let plugins = registry.list_plugins();
        // All built-in plugins have priority 10
        for p in &plugins {
            assert_eq!(p.priority, 10);
        }
    }

    #[test]
    fn test_wim_plugin_blocks_raw_open() {
        let registry = PluginRegistry::with_builtins();
        let magic = [0u8; 16];
        let handler = registry.find_handler(ImageFormat::Wim, &magic).unwrap();
        let result = handler.open(Path::new("nonexistent.wim"));
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("extraction mode"));
    }
}
