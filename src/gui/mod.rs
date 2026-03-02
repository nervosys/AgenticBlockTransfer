// GUI mode — Native cross-platform GUI using egui/eframe
// Cyberpunk-inspired theme with neon accents and deep backgrounds

use anyhow::Result;
use eframe::egui;

use crate::core::device::DeviceInfo;
use crate::core::progress::{OperationPhase, Progress, ProgressSnapshot};

// ─── Theme system ──────────────────────────────────────────────────────────

/// Named color themes for the GUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemePreset {
    Cyberpunk,
    Neon,
    Midnight,
    Ember,
    Arctic,
    Synthwave,
}

impl ThemePreset {
    const ALL: &'static [ThemePreset] = &[
        Self::Cyberpunk,
        Self::Neon,
        Self::Midnight,
        Self::Ember,
        Self::Arctic,
        Self::Synthwave,
    ];

    fn label(&self) -> &'static str {
        match self {
            Self::Cyberpunk => "⚡ Cyberpunk",
            Self::Neon => "♦ Neon",
            Self::Midnight => "☽ Midnight",
            Self::Ember => "♨ Ember",
            Self::Arctic => "✧ Arctic",
            Self::Synthwave => "♫ Synthwave",
        }
    }

    fn build(&self) -> Theme {
        match self {
            Self::Cyberpunk => Theme {
                bg_deep: egui::Color32::from_rgb(10, 10, 18),
                bg_panel: egui::Color32::from_rgb(16, 18, 30),
                bg_card: egui::Color32::from_rgb(22, 26, 42),
                bg_card_hover: egui::Color32::from_rgb(28, 34, 54),
                accent: egui::Color32::from_rgb(0, 255, 234),         // neon cyan
                accent_dim: egui::Color32::from_rgb(0, 180, 165),
                secondary: egui::Color32::from_rgb(255, 0, 170),      // hot pink
                success: egui::Color32::from_rgb(0, 255, 136),        // neon green
                error: egui::Color32::from_rgb(255, 42, 80),          // neon red
                warning: egui::Color32::from_rgb(255, 214, 0),        // electric yellow
                text_primary: egui::Color32::from_rgb(224, 228, 240),
                text_secondary: egui::Color32::from_rgb(130, 140, 170),
                text_muted: egui::Color32::from_rgb(70, 80, 110),
                border: egui::Color32::from_rgb(40, 50, 80),
                border_glow: egui::Color32::from_rgb(0, 255, 234),
                selection: egui::Color32::from_rgb(0, 255, 234),
            },
            Self::Neon => Theme {
                bg_deep: egui::Color32::from_rgb(8, 6, 20),
                bg_panel: egui::Color32::from_rgb(14, 12, 32),
                bg_card: egui::Color32::from_rgb(24, 20, 48),
                bg_card_hover: egui::Color32::from_rgb(34, 28, 62),
                accent: egui::Color32::from_rgb(180, 100, 255),       // violet
                accent_dim: egui::Color32::from_rgb(130, 70, 200),
                secondary: egui::Color32::from_rgb(255, 80, 180),     // pink
                success: egui::Color32::from_rgb(100, 255, 160),
                error: egui::Color32::from_rgb(255, 60, 100),
                warning: egui::Color32::from_rgb(255, 200, 60),
                text_primary: egui::Color32::from_rgb(230, 220, 250),
                text_secondary: egui::Color32::from_rgb(150, 130, 180),
                text_muted: egui::Color32::from_rgb(80, 65, 110),
                border: egui::Color32::from_rgb(50, 35, 80),
                border_glow: egui::Color32::from_rgb(180, 100, 255),
                selection: egui::Color32::from_rgb(180, 100, 255),
            },
            Self::Midnight => Theme {
                bg_deep: egui::Color32::from_rgb(8, 12, 22),
                bg_panel: egui::Color32::from_rgb(12, 18, 34),
                bg_card: egui::Color32::from_rgb(18, 28, 50),
                bg_card_hover: egui::Color32::from_rgb(24, 38, 66),
                accent: egui::Color32::from_rgb(60, 160, 255),        // bright blue
                accent_dim: egui::Color32::from_rgb(40, 120, 200),
                secondary: egui::Color32::from_rgb(100, 200, 255),
                success: egui::Color32::from_rgb(80, 220, 140),
                error: egui::Color32::from_rgb(255, 80, 80),
                warning: egui::Color32::from_rgb(255, 180, 40),
                text_primary: egui::Color32::from_rgb(200, 215, 240),
                text_secondary: egui::Color32::from_rgb(120, 140, 180),
                text_muted: egui::Color32::from_rgb(60, 75, 105),
                border: egui::Color32::from_rgb(30, 45, 75),
                border_glow: egui::Color32::from_rgb(60, 160, 255),
                selection: egui::Color32::from_rgb(60, 160, 255),
            },
            Self::Ember => Theme {
                bg_deep: egui::Color32::from_rgb(16, 8, 6),
                bg_panel: egui::Color32::from_rgb(26, 14, 10),
                bg_card: egui::Color32::from_rgb(40, 22, 16),
                bg_card_hover: egui::Color32::from_rgb(52, 30, 22),
                accent: egui::Color32::from_rgb(255, 120, 30),        // orange
                accent_dim: egui::Color32::from_rgb(200, 90, 20),
                secondary: egui::Color32::from_rgb(255, 180, 60),
                success: egui::Color32::from_rgb(120, 220, 80),
                error: egui::Color32::from_rgb(255, 50, 50),
                warning: egui::Color32::from_rgb(255, 200, 40),
                text_primary: egui::Color32::from_rgb(240, 220, 210),
                text_secondary: egui::Color32::from_rgb(170, 140, 120),
                text_muted: egui::Color32::from_rgb(100, 75, 60),
                border: egui::Color32::from_rgb(70, 40, 28),
                border_glow: egui::Color32::from_rgb(255, 120, 30),
                selection: egui::Color32::from_rgb(255, 120, 30),
            },
            Self::Arctic => Theme {
                bg_deep: egui::Color32::from_rgb(12, 16, 22),
                bg_panel: egui::Color32::from_rgb(18, 24, 34),
                bg_card: egui::Color32::from_rgb(28, 38, 52),
                bg_card_hover: egui::Color32::from_rgb(36, 48, 66),
                accent: egui::Color32::from_rgb(100, 220, 255),       // ice blue
                accent_dim: egui::Color32::from_rgb(70, 180, 220),
                secondary: egui::Color32::from_rgb(160, 230, 255),
                success: egui::Color32::from_rgb(80, 230, 160),
                error: egui::Color32::from_rgb(255, 100, 110),
                warning: egui::Color32::from_rgb(255, 210, 80),
                text_primary: egui::Color32::from_rgb(210, 225, 240),
                text_secondary: egui::Color32::from_rgb(140, 160, 190),
                text_muted: egui::Color32::from_rgb(70, 85, 110),
                border: egui::Color32::from_rgb(40, 55, 75),
                border_glow: egui::Color32::from_rgb(100, 220, 255),
                selection: egui::Color32::from_rgb(100, 220, 255),
            },
            Self::Synthwave => Theme {
                bg_deep: egui::Color32::from_rgb(14, 6, 22),
                bg_panel: egui::Color32::from_rgb(22, 10, 36),
                bg_card: egui::Color32::from_rgb(36, 16, 56),
                bg_card_hover: egui::Color32::from_rgb(48, 22, 72),
                accent: egui::Color32::from_rgb(255, 40, 200),        // magenta
                accent_dim: egui::Color32::from_rgb(200, 30, 150),
                secondary: egui::Color32::from_rgb(100, 200, 255),    // neon blue
                success: egui::Color32::from_rgb(80, 255, 180),
                error: egui::Color32::from_rgb(255, 60, 60),
                warning: egui::Color32::from_rgb(255, 220, 60),
                text_primary: egui::Color32::from_rgb(240, 220, 250),
                text_secondary: egui::Color32::from_rgb(160, 130, 180),
                text_muted: egui::Color32::from_rgb(90, 60, 110),
                border: egui::Color32::from_rgb(60, 30, 85),
                border_glow: egui::Color32::from_rgb(255, 40, 200),
                selection: egui::Color32::from_rgb(255, 40, 200),
            },
        }
    }
}

/// Active color theme with cyberpunk-aware palette.
#[derive(Debug, Clone)]
struct Theme {
    bg_deep: egui::Color32,
    bg_panel: egui::Color32,
    bg_card: egui::Color32,
    bg_card_hover: egui::Color32,
    accent: egui::Color32,
    accent_dim: egui::Color32,
    secondary: egui::Color32,
    success: egui::Color32,
    error: egui::Color32,
    warning: egui::Color32,
    text_primary: egui::Color32,
    text_secondary: egui::Color32,
    text_muted: egui::Color32,
    border: egui::Color32,
    border_glow: egui::Color32,
    selection: egui::Color32,
}

impl Theme {
    /// Apply this theme to the egui context with full visual customization.
    fn apply(&self, ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();

        // Backgrounds
        visuals.panel_fill = self.bg_panel;
        visuals.window_fill = self.bg_panel;
        visuals.extreme_bg_color = self.bg_deep;
        visuals.faint_bg_color = self.bg_card;

        // Selection
        visuals.selection.bg_fill = self.selection.linear_multiply(0.3);
        visuals.selection.stroke = egui::Stroke::new(1.5, self.selection);

        // Hyperlinks
        visuals.hyperlink_color = self.accent;

        // Window styling
        visuals.window_rounding = egui::Rounding::same(8.0);
        visuals.window_shadow = egui::epaint::Shadow {
            offset: egui::vec2(0.0, 4.0),
            blur: 16.0,
            spread: 2.0,
            color: egui::Color32::from_black_alpha(120),
        };
        visuals.window_stroke = egui::Stroke::new(1.0, self.border);

        // Widget styling — inactive
        visuals.widgets.inactive.bg_fill = self.bg_card;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, self.border);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, self.text_secondary);
        visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);

        // Widget styling — hovered
        visuals.widgets.hovered.bg_fill = self.bg_card_hover;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, self.accent_dim);
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, self.text_primary);
        visuals.widgets.hovered.rounding = egui::Rounding::same(6.0);

        // Widget styling — active (pressed)
        visuals.widgets.active.bg_fill = self.accent.linear_multiply(0.2);
        visuals.widgets.active.bg_stroke = egui::Stroke::new(2.0, self.accent);
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, self.accent);
        visuals.widgets.active.rounding = egui::Rounding::same(6.0);

        // Widget styling — open (popups/menus)
        visuals.widgets.open.bg_fill = self.bg_card;
        visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, self.accent_dim);
        visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, self.text_primary);
        visuals.widgets.open.rounding = egui::Rounding::same(6.0);

        // Non-interactive elements
        visuals.widgets.noninteractive.bg_fill = self.bg_panel;
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, self.border);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, self.text_secondary);
        visuals.widgets.noninteractive.rounding = egui::Rounding::same(6.0);

        // Separator
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, self.border);

        // Popup shadow
        visuals.popup_shadow = egui::epaint::Shadow {
            offset: egui::vec2(0.0, 6.0),
            blur: 20.0,
            spread: 4.0,
            color: egui::Color32::from_black_alpha(140),
        };

        // Menu rounding
        visuals.menu_rounding = egui::Rounding::same(8.0);

        ctx.set_visuals(visuals);

        // Typography — configure fonts
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.window_margin = egui::Margin::same(16.0);
        style.visuals.striped = true;
        ctx.set_style(style);
    }
}

// ─── App ───────────────────────────────────────────────────────────────────

struct AbtApp {
    state: GuiState,
    devices: Vec<DeviceInfo>,
    selected_device: Option<usize>,
    source_path: String,
    status: String,
    progress: Option<Progress>,
    verify_after_write: bool,
    show_all_devices: bool,
    // Theme
    theme_preset: ThemePreset,
    theme: Theme,
    /// Async runtime handle for spawning device enumeration.
    rt: Option<tokio::runtime::Handle>,
    /// One-time init flag for applying theme on first frame.
    first_frame: bool,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum GuiState {
    Idle,
    Writing,
    Verifying,
    Complete,
    Error(String),
}

impl Default for AbtApp {
    fn default() -> Self {
        let rt = tokio::runtime::Handle::try_current().ok();
        let preset = ThemePreset::Cyberpunk;

        Self {
            state: GuiState::Idle,
            devices: Vec::new(),
            selected_device: None,
            source_path: String::new(),
            status: "Ready — select an image and target device".to_string(),
            progress: None,
            verify_after_write: true,
            show_all_devices: false,
            theme_preset: preset,
            theme: preset.build(),
            rt,
            first_frame: true,
        }
    }
}

impl eframe::App for AbtApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply theme on first frame and whenever it changes
        if self.first_frame {
            self.theme.apply(ctx);
            self.first_frame = false;
        }

        // ── Handle drag-and-drop ───────────────────────────────────────────
        if !ctx.input(|i| i.raw.hovered_files.is_empty()) {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop_overlay"),
            ));
            let screen = ctx.screen_rect();
            painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(180));

            // Pulsing border effect
            let t = ctx.input(|i| i.time) as f32;
            let pulse = ((t * 3.0).sin() * 0.3 + 0.7).clamp(0.4, 1.0);
            let glow = self.theme.accent.linear_multiply(pulse);
            painter.rect_stroke(
                screen.shrink(8.0),
                12.0,
                egui::Stroke::new(3.0, glow),
            );

            // Drop icon and text
            painter.text(
                screen.center() - egui::vec2(0.0, 16.0),
                egui::Align2::CENTER_CENTER,
                "⬇",
                egui::FontId::proportional(48.0),
                self.theme.accent,
            );
            painter.text(
                screen.center() + egui::vec2(0.0, 28.0),
                egui::Align2::CENTER_CENTER,
                "Drop image file here",
                egui::FontId::proportional(18.0),
                self.theme.text_secondary,
            );
        }

        // Accept dropped files
        let dropped_files: Vec<_> = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped_files.is_empty() {
            for file in &dropped_files {
                if let Some(ref path) = file.path {
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let valid_extensions = [
                        "iso", "img", "raw", "bin", "dmg", "vhd", "vhdx", "vmdk",
                        "qcow2", "wim", "ffu", "gz", "bz2", "xz", "zst", "zip",
                        "dd", "dsk",
                    ];
                    if valid_extensions.contains(&ext.as_str()) || path.is_file() {
                        self.source_path = path.to_string_lossy().to_string();
                        self.status = format!("✓ Loaded: {}", self.source_path);
                        break;
                    }
                }
            }
        }

        // ── Top panel — branding bar ───────────────────────────────────────
        egui::TopBottomPanel::top("top_bar")
            .frame(egui::Frame::default()
                .fill(self.theme.bg_deep)
                .inner_margin(egui::Margin::symmetric(16.0, 6.0))
                .stroke(egui::Stroke::new(1.0, self.theme.border)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Logo / branding
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("◆")
                            .color(self.theme.accent)
                            .size(18.0),
                    );
                    ui.label(
                        egui::RichText::new("ABT")
                            .color(self.theme.accent)
                            .size(16.0)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new("AgenticBlockTransfer")
                            .color(self.theme.text_muted)
                            .size(12.0),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Theme selector
                        ui.menu_button(
                            egui::RichText::new("◐ Theme").color(self.theme.text_secondary).size(12.0),
                            |ui| {
                                for &preset in ThemePreset::ALL {
                                    let selected = self.theme_preset == preset;
                                    if ui.selectable_label(selected, preset.label()).clicked() {
                                        self.theme_preset = preset;
                                        self.theme = preset.build();
                                        self.theme.apply(ctx);
                                        ui.close_menu();
                                    }
                                }
                            },
                        );

                        ui.separator();

                        // Menu items
                        ui.menu_button(
                            egui::RichText::new("Help").color(self.theme.text_secondary).size(12.0),
                            |ui| {
                                if ui.button("About").clicked() {
                                    ui.close_menu();
                                }
                            },
                        );

                        ui.menu_button(
                            egui::RichText::new("View").color(self.theme.text_secondary).size(12.0),
                            |ui| {
                                ui.checkbox(&mut self.show_all_devices, "Show System Drives");
                            },
                        );

                        ui.menu_button(
                            egui::RichText::new("File").color(self.theme.text_secondary).size(12.0),
                            |ui| {
                                if ui.button("⊞ Open Image...").clicked() {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .set_title("Select Disk Image")
                                        .add_filter("Disk Images", &[
                                            "iso", "img", "raw", "bin", "dmg", "vhd", "vhdx",
                                            "vmdk", "qcow2", "wim", "ffu", "gz", "bz2", "xz",
                                            "zst", "zip", "dd", "dsk",
                                        ])
                                        .add_filter("All Files", &["*"])
                                        .pick_file()
                                    {
                                        self.source_path = path.to_string_lossy().to_string();
                                        self.status = format!("✓ Opened: {}", self.source_path);
                                    }
                                    ui.close_menu();
                                }
                                ui.separator();
                                if ui.button("✕ Quit").clicked() {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                            },
                        );
                    });
                });
            });

        // ── Bottom panel — status bar ──────────────────────────────────────
        egui::TopBottomPanel::bottom("status_bar")
            .frame(egui::Frame::default()
                .fill(self.theme.bg_deep)
                .inner_margin(egui::Margin::symmetric(16.0, 4.0))
                .stroke(egui::Stroke::new(1.0, self.theme.border)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Status indicator dot
                    let dot_color = match &self.state {
                        GuiState::Idle => self.theme.accent,
                        GuiState::Writing | GuiState::Verifying => self.theme.warning,
                        GuiState::Complete => self.theme.success,
                        GuiState::Error(_) => self.theme.error,
                    };
                    ui.label(egui::RichText::new("●").color(dot_color).size(10.0));
                    ui.label(
                        egui::RichText::new(&self.status)
                            .color(self.theme.text_secondary)
                            .size(11.0),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .color(self.theme.text_muted)
                                .size(10.0),
                        );
                    });
                });
            });

        // ── Central panel ──────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::default()
                .fill(self.theme.bg_panel)
                .inner_margin(egui::Margin::same(20.0)))
            .show(ctx, |ui| {
                match &self.state.clone() {
                    GuiState::Idle => self.render_main_form(ui, ctx),
                    GuiState::Writing | GuiState::Verifying => self.render_progress(ui, ctx),
                    GuiState::Complete => self.render_complete(ui),
                    GuiState::Error(msg) => self.render_error(ui, &msg.clone()),
                }
            });

        // Request repaint during active operations
        if matches!(self.state, GuiState::Writing | GuiState::Verifying) {
            ctx.request_repaint();
        }
    }
}


// ─── Card helper ───────────────────────────────────────────────────────────

/// Draw a styled card/section with a colored border.
fn card_frame(theme: &Theme, glow_accent: bool) -> egui::Frame {
    let stroke_color = if glow_accent {
        theme.accent_dim
    } else {
        theme.border
    };
    egui::Frame::default()
        .fill(theme.bg_card)
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::same(14.0))
        .stroke(egui::Stroke::new(1.0, stroke_color))
}

/// Styled section heading with an icon badge and accent underline.
fn section_heading(ui: &mut egui::Ui, theme: &Theme, icon: &str, label: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(icon)
                .color(theme.accent)
                .size(16.0),
        );
        ui.label(
            egui::RichText::new(label)
                .color(theme.text_primary)
                .size(15.0)
                .strong(),
        );
    });
    // Accent line — properly allocated so it doesn't overlap content below
    let (line_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 2.0),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(line_rect, 0.0, theme.accent.linear_multiply(0.2));
    ui.add_space(4.0);
}

// ─── Rendering ─────────────────────────────────────────────────────────────

impl AbtApp {
    fn render_main_form(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {

        // ── Step 1: Source image ────────────────────────────────────────────
        card_frame(&self.theme, !self.source_path.is_empty()).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            section_heading(ui, &self.theme, "◉", "Source Image");

            if self.source_path.is_empty() {
                // ── Clickable drop zone when no file is selected ──
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 60.0),
                    egui::Sense::click(),
                );
                let hovered = response.hovered();
                let bg = if hovered {
                    self.theme.bg_card_hover
                } else {
                    self.theme.bg_deep.linear_multiply(0.8)
                };
                let border_color = if hovered {
                    self.theme.accent_dim
                } else {
                    self.theme.border
                };
                ui.painter().rect(
                    rect,
                    egui::Rounding::same(6.0),
                    bg,
                    egui::Stroke::new(1.0, border_color),
                );
                ui.painter().text(
                    egui::pos2(rect.center().x, rect.center().y - 10.0),
                    egui::Align2::CENTER_CENTER,
                    "⊞",
                    egui::FontId::proportional(22.0),
                    if hovered {
                        self.theme.accent
                    } else {
                        self.theme.text_muted
                    },
                );
                ui.painter().text(
                    egui::pos2(rect.center().x, rect.center().y + 14.0),
                    egui::Align2::CENTER_CENTER,
                    "Click to browse or drag & drop an image",
                    egui::FontId::proportional(12.0),
                    if hovered {
                        self.theme.text_secondary
                    } else {
                        self.theme.text_muted
                    },
                );
                if hovered {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Select Disk Image")
                        .add_filter(
                            "Disk Images",
                            &[
                                "iso", "img", "raw", "bin", "dmg", "vhd", "vhdx",
                                "vmdk", "qcow2", "wim", "ffu", "gz", "bz2", "xz",
                                "zst", "zip", "dd", "dsk",
                            ],
                        )
                        .add_filter("All Files", &["*"])
                        .pick_file()
                    {
                        self.source_path = path.to_string_lossy().to_string();
                        self.status = format!("✓ Selected: {}", self.source_path);
                    }
                }
            } else {
                // ── File path row: text field + Browse button ──
                ui.horizontal(|ui| {
                    let btn_width = 80.0;
                    let spacing = ui.spacing().item_spacing.x;
                    let edit_w = (ui.available_width() - btn_width - spacing).max(100.0);

                    let te = egui::TextEdit::singleline(&mut self.source_path)
                        .desired_width(edit_w)
                        .hint_text(
                            egui::RichText::new("path/to/image.iso")
                                .color(self.theme.text_muted),
                        )
                        .text_color(self.theme.text_primary);
                    ui.add(te);

                    let browse_btn = egui::Button::new(
                        egui::RichText::new("Browse")
                            .color(self.theme.accent)
                            .strong(),
                    )
                    .rounding(egui::Rounding::same(6.0));

                    if ui.add_sized(egui::vec2(btn_width, 0.0), browse_btn).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Select Disk Image")
                            .add_filter(
                                "Disk Images",
                                &[
                                    "iso", "img", "raw", "bin", "dmg", "vhd", "vhdx",
                                    "vmdk", "qcow2", "wim", "ffu", "gz", "bz2", "xz",
                                    "zst", "zip", "dd", "dsk",
                                ],
                            )
                            .add_filter("All Files", &["*"])
                            .pick_file()
                        {
                            self.source_path = path.to_string_lossy().to_string();
                            self.status = format!("✓ Selected: {}", self.source_path);
                        }
                    }
                });

                // ── Image info badges ──
                let path = std::path::Path::new(&self.source_path);
                if path.exists() {
                    if let Ok(info) = crate::core::image::get_image_info(path) {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(" {} ", info.format))
                                    .color(self.theme.bg_deep)
                                    .background_color(self.theme.accent)
                                    .strong()
                                    .size(11.0),
                            );
                            ui.label(
                                egui::RichText::new(humansize::format_size(
                                    info.size,
                                    humansize::BINARY,
                                ))
                                .color(self.theme.text_secondary)
                                .size(12.0),
                            );
                            if info.format.is_compressed() {
                                ui.label(
                                    egui::RichText::new("⚡ compressed — auto-decompress")
                                        .color(self.theme.warning)
                                        .size(11.0),
                                );
                            }
                        });
                    }
                } else {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("⚠ File not found")
                            .color(self.theme.error)
                            .size(12.0),
                    );
                }
            }
        });

        ui.add_space(10.0);

        // ── Step 2: Target device ──────────────────────────────────────────
        card_frame(&self.theme, self.selected_device.is_some()).show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // ── Heading row with right-aligned Refresh button ──
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("▤")
                        .color(self.theme.accent)
                        .size(16.0),
                );
                ui.label(
                    egui::RichText::new("Target Device")
                        .color(self.theme.text_primary)
                        .size(15.0)
                        .strong(),
                );

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let refresh_btn = egui::Button::new(
                            egui::RichText::new("⟳ Refresh")
                                .color(self.theme.secondary)
                                .size(12.0),
                        )
                        .rounding(egui::Rounding::same(6.0));

                        if ui.add(refresh_btn).clicked() {
                            let enumerator = crate::core::device::create_enumerator();

                            let result = if let Some(ref rt) = self.rt {
                                std::panic::catch_unwind(
                                    std::panic::AssertUnwindSafe(|| {
                                        tokio::task::block_in_place(|| {
                                            rt.block_on(enumerator.list_devices())
                                        })
                                    }),
                                )
                                .unwrap_or_else(|_| {
                                    let temp_rt = tokio::runtime::Runtime::new()
                                        .expect("failed to create temp runtime");
                                    temp_rt.block_on(enumerator.list_devices())
                                })
                            } else {
                                match tokio::runtime::Runtime::new() {
                                    Ok(temp_rt) => {
                                        temp_rt.block_on(enumerator.list_devices())
                                    }
                                    Err(e) => {
                                        Err(anyhow::anyhow!("Runtime error: {}", e))
                                    }
                                }
                            };

                            match result {
                                Ok(devs) => {
                                    self.status =
                                        format!("● {} device(s) detected", devs.len());
                                    self.selected_device = None;
                                    self.devices = devs;
                                }
                                Err(e) => {
                                    self.status =
                                        format!("⚠ Refresh failed: {}", e);
                                }
                            }
                        }
                    },
                );
            });

            // Accent underline — properly allocated
            let (line_rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), 2.0),
                egui::Sense::hover(),
            );
            ui.painter()
                .rect_filled(line_rect, 0.0, self.theme.accent.linear_multiply(0.2));
            ui.add_space(4.0);

            // ── Device list ──
            if self.devices.is_empty() {
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No devices found")
                            .color(self.theme.text_muted)
                            .size(13.0),
                    );
                    ui.label(
                        egui::RichText::new("Click Refresh to scan for removable drives")
                            .color(self.theme.text_muted)
                            .size(11.0),
                    );
                });
                ui.add_space(4.0);
            } else {
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());

                        for (i, dev) in self.devices.iter().enumerate() {
                            if !self.show_all_devices && dev.is_system {
                                continue;
                            }

                            let selected = self.selected_device == Some(i);
                            let size =
                                humansize::format_size(dev.size, humansize::BINARY);

                            ui.push_id(i, |ui| {
                                // ── Device card row ──
                                let card_bg = if selected {
                                    self.theme.accent.linear_multiply(0.1)
                                } else {
                                    self.theme.bg_card_hover.linear_multiply(0.5)
                                };
                                let card_border = if selected {
                                    self.theme.accent_dim
                                } else {
                                    self.theme.border
                                };

                                let frame = egui::Frame::default()
                                    .fill(card_bg)
                                    .rounding(egui::Rounding::same(6.0))
                                    .inner_margin(
                                        egui::Margin::symmetric(10.0, 6.0),
                                    )
                                    .stroke(egui::Stroke::new(
                                        if selected { 1.5 } else { 0.5 },
                                        card_border,
                                    ));

                                let frame_resp = frame.show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        // Selection indicator
                                        let indicator = if selected {
                                            "◉"
                                        } else {
                                            "○"
                                        };
                                        let ind_color = if selected {
                                            self.theme.accent
                                        } else {
                                            self.theme.text_muted
                                        };
                                        ui.label(
                                            egui::RichText::new(indicator)
                                                .color(ind_color)
                                                .size(14.0),
                                        );

                                        ui.add_space(4.0);

                                        // Device info column
                                        ui.vertical(|ui| {
                                            // Name + system badge
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    egui::RichText::new(
                                                        &dev.name,
                                                    )
                                                    .color(
                                                        self.theme.text_primary,
                                                    )
                                                    .strong()
                                                    .size(13.0),
                                                );
                                                if dev.is_system {
                                                    ui.label(
                                                        egui::RichText::new(
                                                            " SYS ",
                                                        )
                                                        .color(
                                                            self.theme.bg_deep,
                                                        )
                                                        .background_color(
                                                            self.theme.error,
                                                        )
                                                        .size(9.0)
                                                        .strong(),
                                                    );
                                                }
                                            });
                                            // Path · Size · Type
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut()
                                                    .item_spacing
                                                    .x = 6.0;
                                                ui.label(
                                                    egui::RichText::new(
                                                        &dev.path,
                                                    )
                                                    .color(
                                                        self.theme.text_muted,
                                                    )
                                                    .size(11.0),
                                                );
                                                ui.label(
                                                    egui::RichText::new("•")
                                                        .color(
                                                            self.theme
                                                                .text_muted,
                                                        )
                                                        .size(11.0),
                                                );
                                                ui.label(
                                                    egui::RichText::new(&size)
                                                        .color(
                                                            self.theme
                                                                .text_secondary,
                                                        )
                                                        .size(11.0),
                                                );
                                                ui.label(
                                                    egui::RichText::new("•")
                                                        .color(
                                                            self.theme
                                                                .text_muted,
                                                        )
                                                        .size(11.0),
                                                );
                                                ui.label(
                                                    egui::RichText::new(
                                                        format!(
                                                            "{}",
                                                            dev.device_type
                                                        ),
                                                    )
                                                    .color(
                                                        self.theme.text_muted,
                                                    )
                                                    .size(11.0),
                                                );
                                            });
                                        });
                                    });
                                });

                                // Make the entire card clickable / tappable
                                let click_resp = frame_resp
                                    .response
                                    .interact(egui::Sense::click());
                                if click_resp.hovered() {
                                    ui.ctx().set_cursor_icon(
                                        egui::CursorIcon::PointingHand,
                                    );
                                }
                                if click_resp.clicked() {
                                    if dev.is_system {
                                        self.status =
                                            "⚠ Cannot select system drive"
                                                .to_string();
                                    } else {
                                        self.selected_device = Some(i);
                                        self.status = format!(
                                            "✓ Selected: {} ({})",
                                            dev.name, size
                                        );
                                    }
                                }
                            });

                            ui.add_space(2.0);
                        }
                    });
            }
        });

        ui.add_space(10.0);

        // ── Step 3: Options ────────────────────────────────────────────────
        card_frame(&self.theme, false).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            section_heading(ui, &self.theme, "⚙", "Options");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.verify_after_write, "");
                ui.label(
                    egui::RichText::new("Verify after writing")
                        .color(self.theme.text_secondary)
                        .size(12.0),
                );
                ui.label(
                    egui::RichText::new("(recommended)")
                        .color(self.theme.text_muted)
                        .size(11.0)
                        .italics(),
                );
            });
        });

        ui.add_space(16.0);

        // ── Write button ───────────────────────────────────────────────────
        let can_write = !self.source_path.is_empty()
            && std::path::Path::new(&self.source_path).exists()
            && self.selected_device.is_some();

        ui.vertical_centered(|ui| {
            let btn_color = if can_write {
                self.theme.accent
            } else {
                self.theme.text_muted
            };

            let write_btn = egui::Button::new(
                egui::RichText::new("⚡ WRITE IMAGE")
                    .color(if can_write {
                        self.theme.bg_deep
                    } else {
                        self.theme.text_muted
                    })
                    .size(16.0)
                    .strong(),
            )
            .fill(if can_write {
                btn_color
            } else {
                self.theme.bg_card
            })
            .rounding(egui::Rounding::same(8.0))
            .stroke(egui::Stroke::new(
                if can_write { 1.5 } else { 0.5 },
                btn_color,
            ))
            .min_size(egui::vec2(240.0, 44.0));

            if ui.add_enabled(can_write, write_btn).clicked() {
                self.state = GuiState::Writing;
                self.progress = Some(Progress::new(100));
                self.status = "⚡ Writing...".to_string();
            }
        });
    }

    fn render_progress(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let snap = self
            .progress
            .as_ref()
            .map(|p| p.snapshot())
            .unwrap_or(ProgressSnapshot {
                phase: OperationPhase::Preparing,
                bytes_written: 0,
                bytes_total: 100,
                percent: 0.0,
                elapsed_secs: 0.0,
                speed_bytes_per_sec: 0.0,
                eta_secs: None,
            });

        ui.add_space(30.0);

        // Phase heading with animated pulse
        ui.vertical_centered(|ui| {
            let phase_text = format!("{}", snap.phase);
            ui.label(
                egui::RichText::new(&phase_text)
                    .color(self.theme.accent)
                    .size(22.0)
                    .strong(),
            );
        });

        ui.add_space(16.0);

        // Custom progress bar with glow effect
        let progress = snap.percent as f32 / 100.0;
        let bar_height = 24.0;
        let available_width = ui.available_width() - 40.0;
        let (rect, _response) = ui.allocate_exact_size(
            egui::vec2(available_width, bar_height),
            egui::Sense::hover(),
        );

        // Center the bar
        let bar_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(available_width, bar_height));

        // Background track
        ui.painter().rect_filled(
            bar_rect,
            bar_height / 2.0,
            self.theme.bg_card,
        );
        ui.painter().rect_stroke(
            bar_rect,
            bar_height / 2.0,
            egui::Stroke::new(1.0, self.theme.border),
        );

        // Filled portion
        if progress > 0.001 {
            let filled_width = bar_rect.width() * progress.clamp(0.0, 1.0);
            let filled_rect = egui::Rect::from_min_size(
                bar_rect.min,
                egui::vec2(filled_width, bar_height),
            );
            ui.painter().rect_filled(
                filled_rect,
                bar_height / 2.0,
                self.theme.accent,
            );

            // Glow at leading edge
            let t = ctx.input(|i| i.time) as f32;
            let glow_alpha = ((t * 4.0).sin() * 0.3 + 0.5).clamp(0.2, 0.8);
            let glow_width = 30.0_f32.min(filled_width);
            let glow_rect = egui::Rect::from_min_size(
                egui::pos2(filled_rect.max.x - glow_width, filled_rect.min.y),
                egui::vec2(glow_width, bar_height),
            );
            ui.painter().rect_filled(
                glow_rect,
                bar_height / 2.0,
                self.theme.accent.linear_multiply(glow_alpha),
            );
        }

        // Percentage text centered on bar
        ui.painter().text(
            bar_rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{:.1}%", snap.percent),
            egui::FontId::proportional(12.0),
            if progress > 0.45 {
                self.theme.bg_deep
            } else {
                self.theme.text_primary
            },
        );

        ui.add_space(16.0);

        // Stats grid
        card_frame(&self.theme, true).show(ui, |ui| {
            let written = humansize::format_size(snap.bytes_written, humansize::BINARY);
            let total = humansize::format_size(snap.bytes_total, humansize::BINARY);
            let speed =
                humansize::format_size(snap.speed_bytes_per_sec as u64, humansize::BINARY);

            ui.columns(3, |cols| {
                cols[0].vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("PROGRESS")
                            .color(self.theme.text_muted)
                            .size(10.0),
                    );
                    ui.label(
                        egui::RichText::new(format!("{} / {}", written, total))
                            .color(self.theme.text_primary)
                            .size(14.0)
                            .strong(),
                    );
                });
                cols[1].vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("SPEED")
                            .color(self.theme.text_muted)
                            .size(10.0),
                    );
                    ui.label(
                        egui::RichText::new(format!("{}/s", speed))
                            .color(self.theme.accent)
                            .size(14.0)
                            .strong(),
                    );
                });
                cols[2].vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("ETA")
                            .color(self.theme.text_muted)
                            .size(10.0),
                    );
                    let eta_text = if let Some(eta) = snap.eta_secs {
                        if eta > 60.0 {
                            format!("{:.0}m {:.0}s", eta / 60.0, eta % 60.0)
                        } else {
                            format!("{:.0}s", eta)
                        }
                    } else {
                        "calculating...".to_string()
                    };
                    ui.label(
                        egui::RichText::new(eta_text)
                            .color(self.theme.text_primary)
                            .size(14.0)
                            .strong(),
                    );
                });
            });
        });

        ui.add_space(20.0);

        // Cancel button
        ui.vertical_centered(|ui| {
            let cancel_btn = egui::Button::new(
                egui::RichText::new("✕ Cancel")
                    .color(self.theme.error)
                    .size(13.0),
            )
            .rounding(egui::Rounding::same(6.0))
            .stroke(egui::Stroke::new(1.0, self.theme.error.linear_multiply(0.5)));

            if ui.add(cancel_btn).clicked() {
                if let Some(ref p) = self.progress {
                    p.cancel();
                }
                self.state = GuiState::Idle;
                self.status = "Cancelled".to_string();
            }
        });
    }

    fn render_complete(&mut self, ui: &mut egui::Ui) {
        ui.add_space(40.0);

        ui.vertical_centered(|ui| {
            // Success icon
            ui.label(
                egui::RichText::new("✓")
                    .color(self.theme.success)
                    .size(56.0),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Write completed")
                    .color(self.theme.success)
                    .size(22.0)
                    .strong(),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Device is safe to remove")
                    .color(self.theme.text_secondary)
                    .size(13.0),
            );
            ui.add_space(24.0);

            let again_btn = egui::Button::new(
                egui::RichText::new("Write Another")
                    .color(self.theme.accent)
                    .size(14.0)
                    .strong(),
            )
            .rounding(egui::Rounding::same(8.0))
            .stroke(egui::Stroke::new(1.0, self.theme.accent_dim))
            .min_size(egui::vec2(180.0, 38.0));

            if ui.add(again_btn).clicked() {
                self.state = GuiState::Idle;
                self.source_path.clear();
                self.selected_device = None;
                self.status = "Ready — select an image and target device".to_string();
            }
        });
    }

    fn render_error(&mut self, ui: &mut egui::Ui, msg: &str) {
        ui.add_space(40.0);

        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("✗")
                    .color(self.theme.error)
                    .size(56.0),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Write failed")
                    .color(self.theme.error)
                    .size(22.0)
                    .strong(),
            );
            ui.add_space(6.0);

            // Error detail card
            card_frame(&self.theme, false).show(ui, |ui| {
                ui.label(
                    egui::RichText::new(msg)
                        .color(self.theme.text_secondary)
                        .size(12.0),
                );
            });

            ui.add_space(20.0);

            let retry_btn = egui::Button::new(
                egui::RichText::new("Try Again")
                    .color(self.theme.accent)
                    .size(14.0)
                    .strong(),
            )
            .rounding(egui::Rounding::same(8.0))
            .stroke(egui::Stroke::new(1.0, self.theme.accent_dim))
            .min_size(egui::vec2(160.0, 38.0));

            if ui.add(retry_btn).clicked() {
                self.state = GuiState::Idle;
                self.status = "Ready — select an image and target device".to_string();
            }
        });
    }
}

pub fn run() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 620.0])
            .with_min_inner_size([560.0, 480.0])
            .with_title("ABT — AgenticBlockTransfer")
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "ABT — AgenticBlockTransfer",
        options,
        Box::new(|cc| {
            configure_fonts(&cc.egui_ctx);
            Ok(Box::new(AbtApp::default()))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {}", e))
}

/// Load platform-specific symbol fonts as fallbacks so that Unicode glyphs
/// (geometric shapes, arrows, dingbats, misc symbols) render correctly.
fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Platform-specific symbol font paths
    let candidates: &[(&str, &str)] = if cfg!(target_os = "windows") {
        &[
            ("segoe_symbols", r"C:\Windows\Fonts\seguisym.ttf"),
        ]
    } else if cfg!(target_os = "macos") {
        &[
            ("apple_symbols", "/System/Library/Fonts/Apple Symbols.ttf"),
            ("sf_symbols", "/System/Library/Fonts/Supplemental/Apple Symbols.ttf"),
        ]
    } else {
        &[
            ("noto_symbols", "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf"),
            ("dejavu", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
        ]
    };

    for &(name, path) in candidates {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(
                name.to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(data)),
            );
            // Append as fallback to proportional family
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.push(name.to_owned());
            }
        }
    }

    ctx.set_fonts(fonts);
}

