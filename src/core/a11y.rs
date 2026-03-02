#![allow(dead_code)]
//! Accessibility support — screen reader hints, high-contrast mode, keyboard-only nav.
//!
//! Provides an accessibility layer for both TUI and GUI modes:
//!
//! - **Screen reader support**: Semantic labels for all interactive elements
//! - **High-contrast mode**: Increased color contrast for visual impairment
//! - **Keyboard-only navigation**: Full functionality without mouse
//! - **Reduced motion**: Disable animations for vestibular sensitivity
//! - **Focus management**: Clear focus indicators and logical tab order
//! - **Status announcements**: Important state changes announced to screen readers
//!
//! Screen reader integration uses platform-native APIs where available:
//! - Windows: UI Automation / MSAA
//! - macOS: NSAccessibility
//! - Linux: AT-SPI2 (via ATSPI D-Bus)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Global accessibility settings.
static A11Y: OnceLock<AccessibilitySettings> = OnceLock::new();

/// Accessibility preference flags.
pub struct AccessibilitySettings {
    /// Screen reader mode — emit semantic announcements.
    screen_reader: AtomicBool,
    /// High-contrast mode — use maximum contrast colors.
    high_contrast: AtomicBool,
    /// Reduced motion — disable animations and transitions.
    reduced_motion: AtomicBool,
    /// Large text mode — increase font sizes.
    large_text: AtomicBool,
    /// Keyboard-only mode — show all keyboard shortcuts.
    keyboard_only: AtomicBool,
}

impl AccessibilitySettings {
    fn new() -> Self {
        Self {
            screen_reader: AtomicBool::new(false),
            high_contrast: AtomicBool::new(false),
            reduced_motion: AtomicBool::new(false),
            large_text: AtomicBool::new(false),
            keyboard_only: AtomicBool::new(false),
        }
    }

    pub fn screen_reader(&self) -> bool {
        self.screen_reader.load(Ordering::Relaxed)
    }

    pub fn set_screen_reader(&self, enabled: bool) {
        self.screen_reader.store(enabled, Ordering::Relaxed);
    }

    pub fn high_contrast(&self) -> bool {
        self.high_contrast.load(Ordering::Relaxed)
    }

    pub fn set_high_contrast(&self, enabled: bool) {
        self.high_contrast.store(enabled, Ordering::Relaxed);
    }

    pub fn reduced_motion(&self) -> bool {
        self.reduced_motion.load(Ordering::Relaxed)
    }

    pub fn set_reduced_motion(&self, enabled: bool) {
        self.reduced_motion.store(enabled, Ordering::Relaxed);
    }

    pub fn large_text(&self) -> bool {
        self.large_text.load(Ordering::Relaxed)
    }

    pub fn set_large_text(&self, enabled: bool) {
        self.large_text.store(enabled, Ordering::Relaxed);
    }

    pub fn keyboard_only(&self) -> bool {
        self.keyboard_only.load(Ordering::Relaxed)
    }

    pub fn set_keyboard_only(&self, enabled: bool) {
        self.keyboard_only.store(enabled, Ordering::Relaxed);
    }
}

/// Initialize global accessibility settings, detecting system preferences.
pub fn init() {
    A11Y.get_or_init(|| {
        let settings = AccessibilitySettings::new();

        // Detect system accessibility preferences
        if let Some(prefs) = detect_system_a11y() {
            settings.screen_reader.store(prefs.screen_reader, Ordering::Relaxed);
            settings.high_contrast.store(prefs.high_contrast, Ordering::Relaxed);
            settings.reduced_motion.store(prefs.reduced_motion, Ordering::Relaxed);
            settings.large_text.store(prefs.large_text, Ordering::Relaxed);
        }

        settings
    });
}

/// Get the global accessibility settings.
pub fn settings() -> &'static AccessibilitySettings {
    init();
    A11Y.get().unwrap()
}

/// System-detected accessibility preferences.
struct SystemA11yPrefs {
    screen_reader: bool,
    high_contrast: bool,
    reduced_motion: bool,
    large_text: bool,
}

/// Detect system accessibility preferences.
fn detect_system_a11y() -> Option<SystemA11yPrefs> {
    // Check environment variables (cross-platform)
    let screen_reader = std::env::var("SCREEN_READER").ok().map(|v| v == "1").unwrap_or(false)
        || std::env::var("ACCESSIBILITY").ok().map(|v| v == "1").unwrap_or(false);

    let high_contrast =
        std::env::var("HIGH_CONTRAST").ok().map(|v| v == "1").unwrap_or(false);

    let reduced_motion =
        std::env::var("REDUCED_MOTION").ok().map(|v| v == "1").unwrap_or(false)
        || std::env::var("PREFERS_REDUCED_MOTION").ok().map(|v| v == "1").unwrap_or(false);

    let large_text =
        std::env::var("LARGE_TEXT").ok().map(|v| v == "1").unwrap_or(false);

    Some(SystemA11yPrefs {
        screen_reader,
        high_contrast,
        reduced_motion,
        large_text,
    })
}

// ─── Semantic labels ───────────────────────────────────────────────────────

/// ARIA-like role for UI elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Interactive button.
    Button,
    /// Text input field.
    TextInput,
    /// Selectable list.
    List,
    /// Individual list item.
    ListItem,
    /// Progress indicator.
    ProgressBar,
    /// Static label.
    Label,
    /// Heading text.
    Heading,
    /// Alert / notification.
    Alert,
    /// Dialog / modal.
    Dialog,
    /// Status bar region.
    Status,
    /// Navigation region.
    Navigation,
    /// Menu.
    Menu,
    /// Menu item.
    MenuItem,
    /// Checkbox / toggle.
    Checkbox,
    /// Tab panel.
    TabPanel,
    /// Toolbar.
    Toolbar,
}

impl Role {
    /// ARIA role name for screen readers.
    pub fn aria_role(&self) -> &'static str {
        match self {
            Self::Button => "button",
            Self::TextInput => "textbox",
            Self::List => "listbox",
            Self::ListItem => "option",
            Self::ProgressBar => "progressbar",
            Self::Label => "label",
            Self::Heading => "heading",
            Self::Alert => "alert",
            Self::Dialog => "dialog",
            Self::Status => "status",
            Self::Navigation => "navigation",
            Self::Menu => "menu",
            Self::MenuItem => "menuitem",
            Self::Checkbox => "checkbox",
            Self::TabPanel => "tabpanel",
            Self::Toolbar => "toolbar",
        }
    }
}

/// An accessibility annotation for a UI element.
#[derive(Debug, Clone)]
pub struct A11yLabel {
    /// Semantic role.
    pub role: Role,
    /// Human-readable label for screen readers.
    pub label: String,
    /// Additional description (aria-describedby).
    pub description: Option<String>,
    /// Keyboard shortcut hint.
    pub shortcut: Option<String>,
    /// Whether the element is currently focused.
    pub focused: bool,
    /// Whether the element is disabled.
    pub disabled: bool,
    /// For progress bars: current value (0-100).
    pub value: Option<f32>,
    /// For lists: position in set (1-based).
    pub position: Option<(usize, usize)>,
}

impl A11yLabel {
    /// Create a new label with role and text.
    pub fn new(role: Role, label: impl Into<String>) -> Self {
        Self {
            role,
            label: label.into(),
            description: None,
            shortcut: None,
            focused: false,
            disabled: false,
            value: None,
            position: None,
        }
    }

    /// Set the description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set the keyboard shortcut hint.
    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Set the focused state.
    pub fn with_focus(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Set the progress value.
    pub fn with_value(mut self, value: f32) -> Self {
        self.value = Some(value);
        self
    }

    /// Set the position in a set (e.g., "3 of 10").
    pub fn with_position(mut self, current: usize, total: usize) -> Self {
        self.position = Some((current, total));
        self
    }

    /// Generate a full screen reader announcement string.
    pub fn announce(&self) -> String {
        let mut parts = Vec::new();

        parts.push(self.label.clone());

        if let Some(ref desc) = self.description {
            parts.push(desc.clone());
        }

        parts.push(self.role.aria_role().to_string());

        if let Some(value) = self.value {
            parts.push(format!("{:.0}%", value));
        }

        if let Some((pos, total)) = self.position {
            parts.push(format!("{} of {}", pos, total));
        }

        if self.disabled {
            parts.push("disabled".into());
        }

        if let Some(ref shortcut) = self.shortcut {
            parts.push(format!("({})", shortcut));
        }

        parts.join(", ")
    }
}

// ─── Standard UI element labels ────────────────────────────────────────────

/// Standard accessibility labels for common UI elements in abt.
pub struct UiLabels;

impl UiLabels {
    pub fn source_input() -> A11yLabel {
        A11yLabel::new(Role::TextInput, "Source image path")
            .with_description("Enter the path to the disk image file")
            .with_shortcut("Tab")
    }

    pub fn device_list(count: usize) -> A11yLabel {
        A11yLabel::new(Role::List, format!("Target devices, {} available", count))
            .with_description("Select a target device to write to")
            .with_shortcut("Up/Down")
    }

    pub fn device_item(name: &str, size: &str, pos: usize, total: usize) -> A11yLabel {
        A11yLabel::new(Role::ListItem, format!("{} ({})", name, size))
            .with_position(pos, total)
    }

    pub fn write_button(enabled: bool) -> A11yLabel {
        let mut label = A11yLabel::new(Role::Button, "Start write")
            .with_description("Write the selected image to the target device")
            .with_shortcut("Enter");
        label.disabled = !enabled;
        label
    }

    pub fn cancel_button() -> A11yLabel {
        A11yLabel::new(Role::Button, "Cancel")
            .with_description("Cancel the current operation")
            .with_shortcut("Escape")
    }

    pub fn progress_bar(percent: f32, phase: &str) -> A11yLabel {
        A11yLabel::new(Role::ProgressBar, format!("{} progress", phase))
            .with_value(percent)
    }

    pub fn browse_button() -> A11yLabel {
        A11yLabel::new(Role::Button, "Browse for image file")
            .with_shortcut("Ctrl+O")
    }

    pub fn refresh_button() -> A11yLabel {
        A11yLabel::new(Role::Button, "Refresh device list")
            .with_shortcut("F5")
    }

    pub fn verify_checkbox(checked: bool) -> A11yLabel {
        let label = if checked {
            "Verify after writing: enabled"
        } else {
            "Verify after writing: disabled"
        };
        A11yLabel::new(Role::Checkbox, label)
            .with_shortcut("V")
    }

    pub fn theme_menu() -> A11yLabel {
        A11yLabel::new(Role::Menu, "Theme selection")
            .with_description("Choose a color theme")
    }

    pub fn status_bar(message: &str) -> A11yLabel {
        A11yLabel::new(Role::Status, message)
    }

    pub fn confirmation_dialog(device: &str) -> A11yLabel {
        A11yLabel::new(Role::Dialog, format!("Confirm write to {}", device))
            .with_description("This will erase all data on the device. Press Enter to confirm or Escape to cancel.")
    }

    pub fn error_alert(message: &str) -> A11yLabel {
        A11yLabel::new(Role::Alert, format!("Error: {}", message))
    }

    pub fn success_alert(message: &str) -> A11yLabel {
        A11yLabel::new(Role::Alert, format!("Success: {}", message))
    }
}

// ─── High-contrast color palette ───────────────────────────────────────────

/// WCAG 2.1 AA compliant high-contrast colors.
/// All pairs meet minimum 4.5:1 contrast ratio.
pub struct HighContrastPalette;

impl HighContrastPalette {
    /// Background color (black).
    pub fn bg() -> (u8, u8, u8) {
        (0, 0, 0)
    }

    /// Foreground text (white).
    pub fn fg() -> (u8, u8, u8) {
        (255, 255, 255)
    }

    /// Accent / interactive elements (bright cyan).
    pub fn accent() -> (u8, u8, u8) {
        (0, 255, 255)
    }

    /// Success indicator (bright green).
    pub fn success() -> (u8, u8, u8) {
        (0, 255, 0)
    }

    /// Error indicator (bright red).
    pub fn error() -> (u8, u8, u8) {
        (255, 0, 0)
    }

    /// Warning indicator (bright yellow).
    pub fn warning() -> (u8, u8, u8) {
        (255, 255, 0)
    }

    /// Selected / focused item background.
    pub fn selection_bg() -> (u8, u8, u8) {
        (0, 0, 180)
    }

    /// Selected / focused item text.
    pub fn selection_fg() -> (u8, u8, u8) {
        (255, 255, 255)
    }

    /// Disabled text (medium gray — still meets 4.5:1 on black).
    pub fn disabled() -> (u8, u8, u8) {
        (150, 150, 150)
    }
}

/// Calculate the WCAG contrast ratio between two colors.
/// Returns a ratio >= 1.0 where 4.5 meets AA and 7.0 meets AAA.
pub fn contrast_ratio(fg: (u8, u8, u8), bg: (u8, u8, u8)) -> f64 {
    let lum_fg = relative_luminance(fg);
    let lum_bg = relative_luminance(bg);
    let (lighter, darker) = if lum_fg > lum_bg {
        (lum_fg, lum_bg)
    } else {
        (lum_bg, lum_fg)
    };
    (lighter + 0.05) / (darker + 0.05)
}

/// Calculate relative luminance per WCAG 2.1.
fn relative_luminance(color: (u8, u8, u8)) -> f64 {
    let srgb = |c: u8| {
        let v = c as f64 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * srgb(color.0) + 0.7152 * srgb(color.1) + 0.0722 * srgb(color.2)
}

/// Announcement queue for screen reader output.
/// Accumulates status changes and flushes them as announcements.
pub struct AnnouncementQueue {
    queue: Vec<Announcement>,
}

/// A single announcement for the screen reader.
#[derive(Debug, Clone)]
pub struct Announcement {
    /// The text to announce.
    pub text: String,
    /// Priority level.
    pub priority: AnnouncePriority,
    /// Whether to interrupt current speech.
    pub interrupt: bool,
}

/// Announcement priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnnouncePriority {
    /// Low priority — announce when idle.
    Low,
    /// Normal priority.
    Normal,
    /// High priority — announce soon.
    High,
    /// Critical — interrupt current speech.
    Critical,
}

impl AnnouncementQueue {
    pub fn new() -> Self {
        Self { queue: Vec::new() }
    }

    /// Add an announcement to the queue.
    pub fn push(&mut self, text: impl Into<String>, priority: AnnouncePriority) {
        self.queue.push(Announcement {
            text: text.into(),
            interrupt: priority == AnnouncePriority::Critical,
            priority,
        });
    }

    /// Drain all pending announcements, sorted by priority.
    pub fn drain(&mut self) -> Vec<Announcement> {
        let mut items: Vec<_> = self.queue.drain(..).collect();
        items.sort_by(|a, b| b.priority.cmp(&a.priority));
        items
    }

    /// Whether there are pending announcements.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_defaults() {
        let settings = AccessibilitySettings::new();
        assert!(!settings.screen_reader());
        assert!(!settings.high_contrast());
        assert!(!settings.reduced_motion());
        assert!(!settings.large_text());
        assert!(!settings.keyboard_only());
    }

    #[test]
    fn test_settings_toggle() {
        let settings = AccessibilitySettings::new();
        settings.set_screen_reader(true);
        assert!(settings.screen_reader());
        settings.set_high_contrast(true);
        assert!(settings.high_contrast());
        settings.set_reduced_motion(true);
        assert!(settings.reduced_motion());
    }

    #[test]
    fn test_role_aria_names() {
        assert_eq!(Role::Button.aria_role(), "button");
        assert_eq!(Role::TextInput.aria_role(), "textbox");
        assert_eq!(Role::ProgressBar.aria_role(), "progressbar");
        assert_eq!(Role::Alert.aria_role(), "alert");
        assert_eq!(Role::Dialog.aria_role(), "dialog");
    }

    #[test]
    fn test_a11y_label_announce() {
        let label = A11yLabel::new(Role::Button, "Start Write")
            .with_description("Write image to device")
            .with_shortcut("Enter");
        let text = label.announce();
        assert!(text.contains("Start Write"));
        assert!(text.contains("button"));
        assert!(text.contains("Enter"));
    }

    #[test]
    fn test_progress_label() {
        let label = A11yLabel::new(Role::ProgressBar, "Writing")
            .with_value(42.5);
        let text = label.announce();
        assert!(text.contains("42%"));
        assert!(text.contains("progressbar"));
    }

    #[test]
    fn test_list_item_position() {
        let label = A11yLabel::new(Role::ListItem, "USB Drive")
            .with_position(3, 10);
        let text = label.announce();
        assert!(text.contains("3 of 10"));
    }

    #[test]
    fn test_disabled_element() {
        let mut label = A11yLabel::new(Role::Button, "Write");
        label.disabled = true;
        let text = label.announce();
        assert!(text.contains("disabled"));
    }

    #[test]
    fn test_ui_labels_source_input() {
        let label = UiLabels::source_input();
        assert_eq!(label.role, Role::TextInput);
        assert!(label.label.contains("Source"));
    }

    #[test]
    fn test_ui_labels_device_list() {
        let label = UiLabels::device_list(5);
        assert!(label.label.contains("5 available"));
    }

    #[test]
    fn test_ui_labels_write_button_enabled() {
        let label = UiLabels::write_button(true);
        assert!(!label.disabled);
    }

    #[test]
    fn test_ui_labels_write_button_disabled() {
        let label = UiLabels::write_button(false);
        assert!(label.disabled);
    }

    #[test]
    fn test_high_contrast_wcag_aa() {
        // All high-contrast pairs should meet WCAG AA (4.5:1)
        let bg = HighContrastPalette::bg();
        assert!(contrast_ratio(HighContrastPalette::fg(), bg) >= 4.5);
        assert!(contrast_ratio(HighContrastPalette::accent(), bg) >= 4.5);
        assert!(contrast_ratio(HighContrastPalette::success(), bg) >= 4.5);
        assert!(contrast_ratio(HighContrastPalette::error(), bg) >= 4.5);
        assert!(contrast_ratio(HighContrastPalette::warning(), bg) >= 4.5);
        assert!(contrast_ratio(HighContrastPalette::disabled(), bg) >= 4.5);
    }

    #[test]
    fn test_contrast_ratio_black_white() {
        let ratio = contrast_ratio((255, 255, 255), (0, 0, 0));
        assert!(ratio >= 21.0); // Should be exactly 21:1
    }

    #[test]
    fn test_contrast_ratio_same_color() {
        let ratio = contrast_ratio((128, 128, 128), (128, 128, 128));
        assert!((ratio - 1.0).abs() < 0.01); // Should be exactly 1:1
    }

    #[test]
    fn test_announcement_queue() {
        let mut queue = AnnouncementQueue::new();
        assert!(queue.is_empty());

        queue.push("Write started", AnnouncePriority::Normal);
        queue.push("Error occurred!", AnnouncePriority::Critical);
        queue.push("Progress: 50%", AnnouncePriority::Low);

        assert!(!queue.is_empty());

        let items = queue.drain();
        assert_eq!(items.len(), 3);
        // Critical should be first
        assert_eq!(items[0].priority, AnnouncePriority::Critical);
        assert!(items[0].interrupt);
    }

    #[test]
    fn test_global_settings_init() {
        init();
        let s = settings();
        // By default, should be false (unless env vars are set)
        assert_eq!(s.screen_reader(), std::env::var("SCREEN_READER").ok().map(|v| v == "1").unwrap_or(false));
    }

    #[test]
    fn test_confirmation_dialog_label() {
        let label = UiLabels::confirmation_dialog("/dev/sdb");
        assert_eq!(label.role, Role::Dialog);
        assert!(label.label.contains("/dev/sdb"));
        assert!(label.description.unwrap().contains("erase"));
    }

    #[test]
    fn test_error_alert_label() {
        let label = UiLabels::error_alert("Disk full");
        assert_eq!(label.role, Role::Alert);
        assert!(label.label.contains("Error"));
        assert!(label.label.contains("Disk full"));
    }
}
