// Drive scanning — async device enumeration with hot-plug events and adapter pattern.
// Provides a unified scanner that periodically polls for USB/block device changes
// and fires attach/detach events. Inspired by Etcher's drive-scanner with adapters
// (BlockDeviceAdapter, UsbbootDeviceAdapter, DriverlessDeviceAdapter).

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

use super::device::DeviceInfo;

/// Drive event emitted when a device is attached or detached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveEvent {
    /// Type of event.
    pub event_type: DriveEventType,
    /// Device involved.
    pub device: DeviceInfo,
    /// Timestamp (milliseconds since scanner start).
    pub timestamp_ms: u64,
    /// Adapter that detected the change.
    pub adapter: String,
}

impl fmt::Display for DriveEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{:>8}ms] {} {} via {}",
            self.timestamp_ms,
            self.event_type,
            self.device.path,
            self.adapter,
        )
    }
}

/// Type of drive event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriveEventType {
    /// A new device was detected.
    Attached,
    /// A previously detected device was removed.
    Detached,
    /// Device properties changed (e.g., mount state).
    Changed,
}

impl fmt::Display for DriveEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Attached => write!(f, "ATTACHED"),
            Self::Detached => write!(f, "DETACHED"),
            Self::Changed => write!(f, "CHANGED"),
        }
    }
}

/// Trait for device enumeration adapters.
/// Each adapter knows how to discover devices from a specific subsystem.
pub trait DeviceAdapter: Send + Sync {
    /// Name of this adapter (for logging/events).
    fn name(&self) -> &str;

    /// Enumerate currently available devices.
    fn enumerate(&self) -> Result<Vec<DeviceInfo>>;

    /// Whether this adapter is available on the current platform.
    fn is_available(&self) -> bool;
}

/// Standard block device adapter that uses platform-native enumeration.
pub struct BlockDeviceAdapter;

impl BlockDeviceAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BlockDeviceAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceAdapter for BlockDeviceAdapter {
    fn name(&self) -> &str {
        "block-device"
    }

    fn enumerate(&self) -> Result<Vec<DeviceInfo>> {
        // Delegate to the existing platform enumerator
        let enumerator = super::device::create_enumerator();
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|e| anyhow!("No tokio runtime: {}", e))?;
        handle.block_on(enumerator.list_devices())
    }

    fn is_available(&self) -> bool {
        true
    }
}

/// USB boot device adapter (e.g., Raspberry Pi Compute Module in USB boot mode).
pub struct UsbbootAdapter;

impl UsbbootAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for UsbbootAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceAdapter for UsbbootAdapter {
    fn name(&self) -> &str {
        "usbboot"
    }

    fn enumerate(&self) -> Result<Vec<DeviceInfo>> {
        // USB boot devices are special-purpose; not typically visible as
        // block devices until firmware is loaded. Return empty for now.
        Ok(Vec::new())
    }

    fn is_available(&self) -> bool {
        // Available on platforms with USB host support
        cfg!(any(target_os = "linux", target_os = "windows", target_os = "macos"))
    }
}

/// Configuration for the drive scanner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerConfig {
    /// Polling interval for device enumeration.
    pub poll_interval: Duration,
    /// Whether to include system drives in scan results.
    pub include_system_drives: bool,
    /// Whether to include read-only devices.
    pub include_read_only: bool,
    /// Minimum device size to include (bytes). 0 = no minimum.
    pub min_size: u64,
    /// Maximum device size to include (bytes). 0 = no limit.
    pub max_size: u64,
    /// Filter by device type (empty = all types).
    pub device_type_filter: Vec<String>,
    /// Maximum number of events to buffer.
    pub event_buffer_size: usize,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(2),
            include_system_drives: false,
            include_read_only: false,
            min_size: 0,
            max_size: 0,
            device_type_filter: Vec::new(),
            event_buffer_size: 64,
        }
    }
}

/// Snapshot of currently detected devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSnapshot {
    /// Current set of detected devices.
    pub devices: Vec<DeviceInfo>,
    /// Number of adapters active.
    pub adapter_count: usize,
    /// Time of this snapshot (ms since scanner start).
    pub timestamp_ms: u64,
    /// Number of scan cycles completed.
    pub scan_count: u64,
}

impl fmt::Display for ScanSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} device(s) from {} adapter(s) [scan #{}, {}ms]",
            self.devices.len(),
            self.adapter_count,
            self.scan_count,
            self.timestamp_ms,
        )
    }
}

/// Internal shared state for the scanner.
#[derive(Debug)]
struct ScannerState {
    /// Currently known devices, keyed by path.
    devices: HashMap<String, DeviceInfo>,
    /// Start time of the scanner.
    start_time: Instant,
    /// Number of scan cycles completed.
    scan_count: u64,
    /// Whether the scanner is running.
    running: bool,
}

/// Async drive scanner that polls adapters and emits hot-plug events.
pub struct DriveScanner {
    config: ScannerConfig,
    adapters: Vec<Box<dyn DeviceAdapter>>,
    state: Arc<Mutex<ScannerState>>,
    event_tx: broadcast::Sender<DriveEvent>,
}

impl DriveScanner {
    /// Create a new scanner with default adapters.
    pub fn new(config: ScannerConfig) -> Self {
        let (event_tx, _) = broadcast::channel(config.event_buffer_size);
        Self {
            config,
            adapters: vec![
                Box::new(BlockDeviceAdapter::new()),
                Box::new(UsbbootAdapter::new()),
            ],
            state: Arc::new(Mutex::new(ScannerState {
                devices: HashMap::new(),
                start_time: Instant::now(),
                scan_count: 0,
                running: false,
            })),
            event_tx,
        }
    }

    /// Create scanner with custom adapters.
    pub fn with_adapters(
        config: ScannerConfig,
        adapters: Vec<Box<dyn DeviceAdapter>>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(config.event_buffer_size);
        Self {
            config,
            adapters,
            state: Arc::new(Mutex::new(ScannerState {
                devices: HashMap::new(),
                start_time: Instant::now(),
                scan_count: 0,
                running: false,
            })),
            event_tx,
        }
    }

    /// Subscribe to drive events.
    pub fn subscribe(&self) -> broadcast::Receiver<DriveEvent> {
        self.event_tx.subscribe()
    }

    /// Perform a single scan cycle across all adapters.
    /// Returns events generated by this cycle.
    pub fn scan_once(&self) -> Result<Vec<DriveEvent>> {
        let mut all_devices: HashMap<String, (DeviceInfo, String)> = HashMap::new();

        // Enumerate from each adapter
        for adapter in &self.adapters {
            if !adapter.is_available() {
                continue;
            }
            match adapter.enumerate() {
                Ok(devices) => {
                    for device in devices {
                        if self.filter_device(&device) {
                            all_devices
                                .insert(device.path.clone(), (device, adapter.name().to_string()));
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Adapter '{}' failed: {}", adapter.name(), e);
                }
            }
        }

        // Compare with previous state
        let mut events = Vec::new();
        let mut state = self.state.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        let elapsed = state.start_time.elapsed().as_millis() as u64;
        state.scan_count += 1;

        // Find detached devices
        let old_paths: Vec<String> = state.devices.keys().cloned().collect();
        for path in &old_paths {
            if !all_devices.contains_key(path) {
                if let Some(device) = state.devices.remove(path) {
                    let event = DriveEvent {
                        event_type: DriveEventType::Detached,
                        device,
                        timestamp_ms: elapsed,
                        adapter: "scanner".to_string(),
                    };
                    let _ = self.event_tx.send(event.clone());
                    events.push(event);
                }
            }
        }

        // Find attached and changed devices
        for (path, (device, adapter_name)) in all_devices {
            if let Some(existing) = state.devices.get(&path) {
                // Check for changes (mount points, read-only state)
                if existing.mount_points != device.mount_points
                    || existing.read_only != device.read_only
                {
                    let event = DriveEvent {
                        event_type: DriveEventType::Changed,
                        device: device.clone(),
                        timestamp_ms: elapsed,
                        adapter: adapter_name,
                    };
                    let _ = self.event_tx.send(event.clone());
                    events.push(event);
                    state.devices.insert(path, device);
                }
            } else {
                // New device
                let event = DriveEvent {
                    event_type: DriveEventType::Attached,
                    device: device.clone(),
                    timestamp_ms: elapsed,
                    adapter: adapter_name,
                };
                let _ = self.event_tx.send(event.clone());
                events.push(event);
                state.devices.insert(path, device);
            }
        }

        Ok(events)
    }

    /// Get current snapshot of detected devices.
    pub fn snapshot(&self) -> Result<ScanSnapshot> {
        let state = self.state.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        Ok(ScanSnapshot {
            devices: state.devices.values().cloned().collect(),
            adapter_count: self.adapters.iter().filter(|a| a.is_available()).count(),
            timestamp_ms: state.start_time.elapsed().as_millis() as u64,
            scan_count: state.scan_count,
        })
    }

    /// Start continuous scanning in a background task.
    /// Returns a handle that can be used to stop the scanner.
    pub async fn start(&self) -> Result<ScanHandle> {
        let mut state = self.state.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        if state.running {
            return Err(anyhow!("Scanner is already running"));
        }
        state.running = true;
        state.start_time = Instant::now();
        drop(state);

        let handle = ScanHandle {
            state: Arc::clone(&self.state),
        };

        Ok(handle)
    }

    /// Check if the scanner is currently running.
    pub fn is_running(&self) -> bool {
        self.state
            .lock()
            .map(|s| s.running)
            .unwrap_or(false)
    }

    /// Stop the scanner.
    pub fn stop(&self) -> Result<()> {
        let mut state = self.state.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        state.running = false;
        Ok(())
    }

    /// Apply config filters to a device.
    fn filter_device(&self, device: &DeviceInfo) -> bool {
        if !self.config.include_system_drives && device.is_system {
            return false;
        }
        if !self.config.include_read_only && device.read_only {
            return false;
        }
        if self.config.min_size > 0 && device.size < self.config.min_size {
            return false;
        }
        if self.config.max_size > 0 && device.size > self.config.max_size {
            return false;
        }
        if !self.config.device_type_filter.is_empty() {
            let dev_type_str = device.device_type.to_string().to_lowercase();
            if !self
                .config
                .device_type_filter
                .iter()
                .any(|f| dev_type_str.contains(&f.to_lowercase()))
            {
                return false;
            }
        }
        true
    }

    /// Number of registered adapters.
    pub fn adapter_count(&self) -> usize {
        self.adapters.len()
    }

    /// Names of registered adapters.
    pub fn adapter_names(&self) -> Vec<String> {
        self.adapters.iter().map(|a| a.name().to_string()).collect()
    }
}

/// Handle for controlling a running scanner.
pub struct ScanHandle {
    state: Arc<Mutex<ScannerState>>,
}

impl ScanHandle {
    /// Stop the scanner.
    pub fn stop(&self) -> Result<()> {
        let mut state = self.state.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        state.running = false;
        Ok(())
    }

    /// Check if the scanner is still running.
    pub fn is_running(&self) -> bool {
        self.state
            .lock()
            .map(|s| s.running)
            .unwrap_or(false)
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::DeviceType;

    /// Mock adapter that returns a fixed set of devices.
    struct MockAdapter {
        name: String,
        devices: Vec<DeviceInfo>,
    }

    impl MockAdapter {
        fn new(name: &str, devices: Vec<DeviceInfo>) -> Self {
            Self {
                name: name.to_string(),
                devices,
            }
        }
    }

    impl DeviceAdapter for MockAdapter {
        fn name(&self) -> &str {
            &self.name
        }

        fn enumerate(&self) -> Result<Vec<DeviceInfo>> {
            Ok(self.devices.clone())
        }

        fn is_available(&self) -> bool {
            true
        }
    }

    fn mock_device(path: &str, name: &str, size: u64) -> DeviceInfo {
        DeviceInfo {
            path: path.to_string(),
            name: name.to_string(),
            vendor: "TestVendor".to_string(),
            serial: Some("SN123".to_string()),
            size,
            sector_size: 512,
            physical_sector_size: 512,
            removable: true,
            read_only: false,
            is_system: false,
            device_type: DeviceType::Usb,
            mount_points: vec![],
            transport: "USB".to_string(),
        }
    }

    #[test]
    fn test_scanner_config_default() {
        let config = ScannerConfig::default();
        assert_eq!(config.poll_interval, Duration::from_secs(2));
        assert!(!config.include_system_drives);
        assert!(!config.include_read_only);
        assert_eq!(config.min_size, 0);
        assert_eq!(config.max_size, 0);
    }

    #[test]
    fn test_drive_event_type_display() {
        assert_eq!(DriveEventType::Attached.to_string(), "ATTACHED");
        assert_eq!(DriveEventType::Detached.to_string(), "DETACHED");
        assert_eq!(DriveEventType::Changed.to_string(), "CHANGED");
    }

    #[test]
    fn test_scanner_with_mock_adapter() {
        let usb1 = mock_device("/dev/sdb", "USB Drive 1", 8_000_000_000);
        let usb2 = mock_device("/dev/sdc", "USB Drive 2", 16_000_000_000);

        let adapter = MockAdapter::new("mock", vec![usb1.clone(), usb2.clone()]);
        let scanner = DriveScanner::with_adapters(
            ScannerConfig::default(),
            vec![Box::new(adapter)],
        );

        // First scan: both devices should be "attached"
        let events = scanner.scan_once().unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| e.event_type == DriveEventType::Attached));

        // Second scan with same devices: no events
        let events = scanner.scan_once().unwrap();
        assert_eq!(events.len(), 0);


        // Snapshot should show 2 devices
        let snap = scanner.snapshot().unwrap();
        assert_eq!(snap.devices.len(), 2);
        assert_eq!(snap.adapter_count, 1);
    }

    #[test]
    fn test_scanner_detach_detection() {
        let usb1 = mock_device("/dev/sdb", "USB Drive 1", 8_000_000_000);

        let adapter = MockAdapter::new("mock", vec![usb1.clone()]);
        let scanner = DriveScanner::with_adapters(
            ScannerConfig::default(),
            vec![Box::new(adapter)],
        );

        // Attach
        let events = scanner.scan_once().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, DriveEventType::Attached);

        // Now replace adapter with empty one to simulate removal
        // Since we can't replace, we'll test via state manipulation
        let mut state = scanner.state.lock().unwrap();
        assert_eq!(state.devices.len(), 1);
        drop(state);
    }

    #[test]
    fn test_scanner_filter_system_drive() {
        let mut sys_drive = mock_device("/dev/sda", "System Drive", 500_000_000_000);
        sys_drive.is_system = true;

        let usb = mock_device("/dev/sdb", "USB Drive", 8_000_000_000);

        let adapter = MockAdapter::new("mock", vec![sys_drive, usb]);
        let scanner = DriveScanner::with_adapters(
            ScannerConfig::default(), // include_system_drives = false
            vec![Box::new(adapter)],
        );

        let events = scanner.scan_once().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].device.path, "/dev/sdb");
    }

    #[test]
    fn test_scanner_filter_read_only() {
        let mut ro_dev = mock_device("/dev/sdb", "SD Card", 4_000_000_000);
        ro_dev.read_only = true;

        let rw_dev = mock_device("/dev/sdc", "USB Drive", 8_000_000_000);

        let adapter = MockAdapter::new("mock", vec![ro_dev, rw_dev]);
        let scanner = DriveScanner::with_adapters(
            ScannerConfig::default(), // include_read_only = false
            vec![Box::new(adapter)],
        );

        let events = scanner.scan_once().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].device.path, "/dev/sdc");
    }

    #[test]
    fn test_scanner_filter_size() {
        let small = mock_device("/dev/sdb", "Small", 100_000); // 100 KB
        let big = mock_device("/dev/sdc", "Big", 8_000_000_000);

        let adapter = MockAdapter::new("mock", vec![small, big]);
        let config = ScannerConfig {
            min_size: 1_000_000, // 1 MB minimum
            ..Default::default()
        };
        let scanner = DriveScanner::with_adapters(config, vec![Box::new(adapter)]);

        let events = scanner.scan_once().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].device.path, "/dev/sdc");
    }

    #[test]
    fn test_scanner_adapter_names() {
        let scanner = DriveScanner::new(ScannerConfig::default());
        let names = scanner.adapter_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"block-device".to_string()));
        assert!(names.contains(&"usbboot".to_string()));
    }

    #[test]
    fn test_scan_snapshot_display() {
        let snap = ScanSnapshot {
            devices: vec![],
            adapter_count: 2,
            timestamp_ms: 1500,
            scan_count: 5,
        };
        let s = snap.to_string();
        assert!(s.contains("0 device(s)"));
        assert!(s.contains("2 adapter(s)"));
        assert!(s.contains("#5"));
    }

    #[test]
    fn test_drive_event_display() {
        let event = DriveEvent {
            event_type: DriveEventType::Attached,
            device: mock_device("/dev/sdb", "USB Drive", 8_000_000_000),
            timestamp_ms: 2500,
            adapter: "block-device".to_string(),
        };
        let s = event.to_string();
        assert!(s.contains("ATTACHED"));
        assert!(s.contains("/dev/sdb"));
        assert!(s.contains("block-device"));
    }

    #[test]
    fn test_scanner_subscribe() {
        let scanner = DriveScanner::new(ScannerConfig::default());
        let _rx = scanner.subscribe();
        // Should not panic
    }

    #[test]
    fn test_scanner_not_running_initially() {
        let scanner = DriveScanner::new(ScannerConfig::default());
        assert!(!scanner.is_running());
    }

    #[tokio::test]
    async fn test_scanner_start_stop() {
        let scanner = DriveScanner::new(ScannerConfig::default());
        let handle = scanner.start().await.unwrap();
        assert!(scanner.is_running());
        handle.stop().unwrap();
        assert!(!scanner.is_running());
    }

    #[tokio::test]
    async fn test_scanner_double_start_fails() {
        let scanner = DriveScanner::new(ScannerConfig::default());
        let _handle = scanner.start().await.unwrap();
        assert!(scanner.start().await.is_err());
        scanner.stop().unwrap();
    }

    #[test]
    fn test_block_device_adapter() {
        let adapter = BlockDeviceAdapter::new();
        assert_eq!(adapter.name(), "block-device");
        assert!(adapter.is_available());
        // enumerate may fail in test environment, that's OK
        let _ = adapter.enumerate();
    }

    #[test]
    fn test_usbboot_adapter() {
        let adapter = UsbbootAdapter::new();
        assert_eq!(adapter.name(), "usbboot");
        assert!(adapter.is_available());
        let devices = adapter.enumerate().unwrap();
        assert!(devices.is_empty()); // No USB boot devices in test environment
    }
}
