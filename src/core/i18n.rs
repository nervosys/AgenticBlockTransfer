#![allow(dead_code)]
//! Localization / i18n — multi-language support for CLI, TUI, and GUI messages.
//!
//! Provides a lightweight message catalog system with:
//! - Compile-time English fallback (always available)
//! - Runtime locale detection (LANG / LC_MESSAGES / system API)
//! - JSON-based translation files
//! - Parameterized message formatting with `{0}`, `{1}` placeholders
//! - 12+ translatable message categories
//!
//! Usage:
//! ```ignore
//! use crate::core::i18n::{t, set_locale};
//! set_locale("de");
//! println!("{}", t("write.started")); // "Schreibvorgang gestartet"
//! println!("{}", t_fmt("write.progress", &["50%"])); // "Fortschritt: 50%"
//! ```

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// Global message catalog instance.
static CATALOG: OnceLock<RwLock<MessageCatalog>> = OnceLock::new();

/// Supported locales.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    En,
    De,
    Fr,
    Es,
    Pt,
    Ja,
    Zh,
    Ko,
    Ru,
    Ar,
    It,
    Nl,
}

impl Locale {
    /// Parse a locale string (e.g., "en_US.UTF-8" → En).
    pub fn from_str(s: &str) -> Self {
        let lower = s.to_lowercase();
        let code = lower.split(|c| c == '_' || c == '-' || c == '.').next().unwrap_or("en");
        match code {
            "de" => Self::De,
            "fr" => Self::Fr,
            "es" => Self::Es,
            "pt" => Self::Pt,
            "ja" => Self::Ja,
            "zh" => Self::Zh,
            "ko" => Self::Ko,
            "ru" => Self::Ru,
            "ar" => Self::Ar,
            "it" => Self::It,
            "nl" => Self::Nl,
            _ => Self::En,
        }
    }

    /// BCP 47 language tag.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::En => "en",
            Self::De => "de",
            Self::Fr => "fr",
            Self::Es => "es",
            Self::Pt => "pt",
            Self::Ja => "ja",
            Self::Zh => "zh",
            Self::Ko => "ko",
            Self::Ru => "ru",
            Self::Ar => "ar",
            Self::It => "it",
            Self::Nl => "nl",
        }
    }

    /// Human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::En => "English",
            Self::De => "Deutsch",
            Self::Fr => "Français",
            Self::Es => "Español",
            Self::Pt => "Português",
            Self::Ja => "日本語",
            Self::Zh => "中文",
            Self::Ko => "한국어",
            Self::Ru => "Русский",
            Self::Ar => "العربية",
            Self::It => "Italiano",
            Self::Nl => "Nederlands",
        }
    }

    /// All supported locales.
    pub fn all() -> &'static [Locale] {
        &[
            Self::En, Self::De, Self::Fr, Self::Es, Self::Pt,
            Self::Ja, Self::Zh, Self::Ko, Self::Ru, Self::Ar,
            Self::It, Self::Nl,
        ]
    }
}

/// The message catalog: a map of locale → (message_key → translated_string).
pub struct MessageCatalog {
    locale: Locale,
    messages: HashMap<Locale, HashMap<String, String>>,
}

impl MessageCatalog {
    /// Create a new catalog with built-in English messages and detect system locale.
    pub fn new() -> Self {
        let mut catalog = Self {
            locale: Locale::En,
            messages: HashMap::new(),
        };

        // Load built-in English messages
        catalog.messages.insert(Locale::En, Self::english_messages());

        // Load built-in German messages (example)
        catalog.messages.insert(Locale::De, Self::german_messages());

        // Load built-in French messages (example)
        catalog.messages.insert(Locale::Fr, Self::french_messages());

        // Load built-in Spanish messages
        catalog.messages.insert(Locale::Es, Self::spanish_messages());

        // Detect system locale
        catalog.locale = detect_system_locale();

        catalog
    }

    /// Set the active locale.
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    /// Get the active locale.
    pub fn locale(&self) -> Locale {
        self.locale
    }

    /// Look up a message by key. Falls back to English if not found.
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        // Try current locale first
        if let Some(msgs) = self.messages.get(&self.locale) {
            if let Some(msg) = msgs.get(key) {
                return msg;
            }
        }

        // Fall back to English
        if let Some(msgs) = self.messages.get(&Locale::En) {
            if let Some(msg) = msgs.get(key) {
                return msg;
            }
        }

        // Return the key itself as last resort
        key
    }

    /// Format a message with positional parameters.
    /// Replaces `{0}`, `{1}`, etc. with the provided arguments.
    pub fn format(&self, key: &str, args: &[&str]) -> String {
        let template = self.get(key);
        let mut result = template.to_string();
        for (i, arg) in args.iter().enumerate() {
            result = result.replace(&format!("{{{}}}", i), arg);
        }
        result
    }

    /// Load translation messages from a JSON string.
    /// Expected format: `{"key": "translated value", ...}`
    pub fn load_json(&mut self, locale: Locale, json: &str) -> Result<usize, String> {
        let map: HashMap<String, String> =
            serde_json::from_str(json).map_err(|e| format!("Invalid translation JSON: {}", e))?;
        let count = map.len();
        self.messages.insert(locale, map);
        Ok(count)
    }

    /// Get the number of translated messages for a locale.
    pub fn message_count(&self, locale: Locale) -> usize {
        self.messages.get(&locale).map(|m| m.len()).unwrap_or(0)
    }

    // ─── Built-in message tables ────────────────────────────────────────

    fn english_messages() -> HashMap<String, String> {
        let mut m = HashMap::new();

        // General
        m.insert("app.name".into(), "AgenticBlockTransfer".into());
        m.insert("app.version".into(), "Version {0}".into());

        // Write operation
        m.insert("write.started".into(), "Write operation started".into());
        m.insert("write.progress".into(), "Progress: {0}".into());
        m.insert("write.complete".into(), "Write completed successfully".into());
        m.insert("write.failed".into(), "Write failed: {0}".into());
        m.insert("write.cancelled".into(), "Write cancelled by user".into());
        m.insert("write.source".into(), "Source: {0}".into());
        m.insert("write.target".into(), "Target: {0}".into());
        m.insert("write.verify_start".into(), "Verifying written data...".into());
        m.insert("write.verify_pass".into(), "Verification passed".into());
        m.insert("write.verify_fail".into(), "Verification failed!".into());

        // Device
        m.insert("device.list_header".into(), "Available devices:".into());
        m.insert("device.none_found".into(), "No suitable devices found".into());
        m.insert("device.removable".into(), "Removable".into());
        m.insert("device.system".into(), "System drive".into());
        m.insert("device.confirm".into(), "Are you sure you want to write to {0}? This will ERASE ALL DATA. (yes/no)".into());

        // Errors
        m.insert("error.permission".into(), "Permission denied. Run with elevated privileges.".into());
        m.insert("error.not_found".into(), "File not found: {0}".into());
        m.insert("error.io".into(), "I/O error: {0}".into());
        m.insert("error.invalid_format".into(), "Unsupported image format: {0}".into());
        m.insert("error.device_busy".into(), "Device is busy or mounted: {0}".into());

        // Safety
        m.insert("safety.dry_run".into(), "DRY RUN — no data will be written".into());
        m.insert("safety.system_drive".into(), "WARNING: {0} appears to be a system drive!".into());
        m.insert("safety.backup_created".into(), "Partition table backup saved to {0}".into());

        // GUI / TUI
        m.insert("ui.select_source".into(), "Select source image".into());
        m.insert("ui.select_device".into(), "Select target device".into());
        m.insert("ui.start_write".into(), "Start Write".into());
        m.insert("ui.cancel".into(), "Cancel".into());
        m.insert("ui.browse".into(), "Browse...".into());
        m.insert("ui.refresh".into(), "Refresh".into());
        m.insert("ui.settings".into(), "Settings".into());
        m.insert("ui.about".into(), "About".into());
        m.insert("ui.theme".into(), "Theme".into());
        m.insert("ui.language".into(), "Language".into());

        m
    }

    fn german_messages() -> HashMap<String, String> {
        let mut m = HashMap::new();

        m.insert("app.name".into(), "AgenticBlockTransfer".into());
        m.insert("app.version".into(), "Version {0}".into());

        m.insert("write.started".into(), "Schreibvorgang gestartet".into());
        m.insert("write.progress".into(), "Fortschritt: {0}".into());
        m.insert("write.complete".into(), "Schreibvorgang erfolgreich abgeschlossen".into());
        m.insert("write.failed".into(), "Schreibvorgang fehlgeschlagen: {0}".into());
        m.insert("write.cancelled".into(), "Schreibvorgang abgebrochen".into());
        m.insert("write.source".into(), "Quelle: {0}".into());
        m.insert("write.target".into(), "Ziel: {0}".into());
        m.insert("write.verify_start".into(), "Geschriebene Daten werden überprüft...".into());
        m.insert("write.verify_pass".into(), "Überprüfung bestanden".into());
        m.insert("write.verify_fail".into(), "Überprüfung fehlgeschlagen!".into());

        m.insert("device.list_header".into(), "Verfügbare Geräte:".into());
        m.insert("device.none_found".into(), "Keine geeigneten Geräte gefunden".into());
        m.insert("device.removable".into(), "Wechseldatenträger".into());
        m.insert("device.system".into(), "Systemlaufwerk".into());
        m.insert("device.confirm".into(), "Möchten Sie wirklich auf {0} schreiben? ALLE DATEN WERDEN GELÖSCHT. (ja/nein)".into());

        m.insert("error.permission".into(), "Zugriff verweigert. Bitte mit erhöhten Rechten ausführen.".into());
        m.insert("error.not_found".into(), "Datei nicht gefunden: {0}".into());
        m.insert("error.io".into(), "E/A-Fehler: {0}".into());
        m.insert("error.invalid_format".into(), "Nicht unterstütztes Bildformat: {0}".into());
        m.insert("error.device_busy".into(), "Gerät ist belegt oder eingehängt: {0}".into());

        m.insert("safety.dry_run".into(), "TESTLAUF — es werden keine Daten geschrieben".into());
        m.insert("safety.system_drive".into(), "WARNUNG: {0} scheint ein Systemlaufwerk zu sein!".into());
        m.insert("safety.backup_created".into(), "Partitionstabelle gesichert in {0}".into());

        m.insert("ui.select_source".into(), "Quelldatei auswählen".into());
        m.insert("ui.select_device".into(), "Zielgerät auswählen".into());
        m.insert("ui.start_write".into(), "Schreiben starten".into());
        m.insert("ui.cancel".into(), "Abbrechen".into());
        m.insert("ui.browse".into(), "Durchsuchen...".into());
        m.insert("ui.refresh".into(), "Aktualisieren".into());
        m.insert("ui.settings".into(), "Einstellungen".into());
        m.insert("ui.about".into(), "Über".into());
        m.insert("ui.theme".into(), "Design".into());
        m.insert("ui.language".into(), "Sprache".into());

        m
    }

    fn french_messages() -> HashMap<String, String> {
        let mut m = HashMap::new();

        m.insert("app.name".into(), "AgenticBlockTransfer".into());
        m.insert("app.version".into(), "Version {0}".into());

        m.insert("write.started".into(), "Écriture démarrée".into());
        m.insert("write.progress".into(), "Progression : {0}".into());
        m.insert("write.complete".into(), "Écriture terminée avec succès".into());
        m.insert("write.failed".into(), "Échec de l'écriture : {0}".into());
        m.insert("write.cancelled".into(), "Écriture annulée par l'utilisateur".into());
        m.insert("write.source".into(), "Source : {0}".into());
        m.insert("write.target".into(), "Cible : {0}".into());
        m.insert("write.verify_start".into(), "Vérification des données écrites...".into());
        m.insert("write.verify_pass".into(), "Vérification réussie".into());
        m.insert("write.verify_fail".into(), "Échec de la vérification !".into());

        m.insert("device.list_header".into(), "Périphériques disponibles :".into());
        m.insert("device.none_found".into(), "Aucun périphérique compatible trouvé".into());
        m.insert("device.removable".into(), "Amovible".into());
        m.insert("device.system".into(), "Lecteur système".into());
        m.insert("device.confirm".into(), "Voulez-vous vraiment écrire sur {0} ? TOUTES LES DONNÉES SERONT EFFACÉES. (oui/non)".into());

        m.insert("error.permission".into(), "Permission refusée. Exécutez avec des privilèges élevés.".into());
        m.insert("error.not_found".into(), "Fichier introuvable : {0}".into());
        m.insert("error.io".into(), "Erreur E/S : {0}".into());
        m.insert("error.invalid_format".into(), "Format d'image non pris en charge : {0}".into());

        m.insert("ui.select_source".into(), "Sélectionner l'image source".into());
        m.insert("ui.select_device".into(), "Sélectionner le périphérique cible".into());
        m.insert("ui.start_write".into(), "Démarrer l'écriture".into());
        m.insert("ui.cancel".into(), "Annuler".into());
        m.insert("ui.browse".into(), "Parcourir...".into());
        m.insert("ui.refresh".into(), "Actualiser".into());
        m.insert("ui.settings".into(), "Paramètres".into());
        m.insert("ui.about".into(), "À propos".into());
        m.insert("ui.theme".into(), "Thème".into());
        m.insert("ui.language".into(), "Langue".into());

        m
    }

    fn spanish_messages() -> HashMap<String, String> {
        let mut m = HashMap::new();

        m.insert("app.name".into(), "AgenticBlockTransfer".into());
        m.insert("app.version".into(), "Versión {0}".into());

        m.insert("write.started".into(), "Escritura iniciada".into());
        m.insert("write.progress".into(), "Progreso: {0}".into());
        m.insert("write.complete".into(), "Escritura completada con éxito".into());
        m.insert("write.failed".into(), "Error en la escritura: {0}".into());
        m.insert("write.cancelled".into(), "Escritura cancelada por el usuario".into());
        m.insert("write.source".into(), "Origen: {0}".into());
        m.insert("write.target".into(), "Destino: {0}".into());
        m.insert("write.verify_start".into(), "Verificando los datos escritos...".into());
        m.insert("write.verify_pass".into(), "Verificación exitosa".into());
        m.insert("write.verify_fail".into(), "¡Error en la verificación!".into());

        m.insert("device.list_header".into(), "Dispositivos disponibles:".into());
        m.insert("device.none_found".into(), "No se encontraron dispositivos compatibles".into());
        m.insert("device.removable".into(), "Extraíble".into());
        m.insert("device.system".into(), "Unidad del sistema".into());
        m.insert("device.confirm".into(), "¿Está seguro de que desea escribir en {0}? SE BORRARÁN TODOS LOS DATOS. (sí/no)".into());

        m.insert("error.permission".into(), "Permiso denegado. Ejecute con privilegios elevados.".into());
        m.insert("error.not_found".into(), "Archivo no encontrado: {0}".into());
        m.insert("error.io".into(), "Error de E/S: {0}".into());

        m.insert("ui.select_source".into(), "Seleccionar imagen de origen".into());
        m.insert("ui.select_device".into(), "Seleccionar dispositivo de destino".into());
        m.insert("ui.start_write".into(), "Iniciar escritura".into());
        m.insert("ui.cancel".into(), "Cancelar".into());
        m.insert("ui.browse".into(), "Examinar...".into());
        m.insert("ui.refresh".into(), "Actualizar".into());
        m.insert("ui.settings".into(), "Configuración".into());
        m.insert("ui.about".into(), "Acerca de".into());
        m.insert("ui.theme".into(), "Tema".into());
        m.insert("ui.language".into(), "Idioma".into());

        m
    }
}

/// Detect the system locale from environment variables or platform API.
pub fn detect_system_locale() -> Locale {
    // Try LANG, LC_MESSAGES, LC_ALL environment variables
    for var in &["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            if !val.is_empty() && val != "C" && val != "POSIX" {
                return Locale::from_str(&val);
            }
        }
    }

    // On Windows, try to detect from system locale
    #[cfg(target_os = "windows")]
    {
        // Check LANGUAGE environment variable (set by some tools)
        if let Ok(val) = std::env::var("LANGUAGE") {
            if !val.is_empty() {
                return Locale::from_str(&val);
            }
        }
    }

    Locale::En
}

/// Initialize the global message catalog.
/// Call once at startup, before any `t()` or `t_fmt()` calls.
pub fn init() {
    CATALOG.get_or_init(|| RwLock::new(MessageCatalog::new()));
}

/// Set the active locale globally.
pub fn set_locale(locale_str: &str) {
    init();
    if let Some(lock) = CATALOG.get() {
        if let Ok(mut catalog) = lock.write() {
            catalog.set_locale(Locale::from_str(locale_str));
        }
    }
}

/// Get the current locale.
pub fn current_locale() -> Locale {
    init();
    if let Some(lock) = CATALOG.get() {
        if let Ok(catalog) = lock.read() {
            return catalog.locale();
        }
    }
    Locale::En
}

/// Translate a message key to the current locale.
/// Falls back to English, then to the key itself.
pub fn t(key: &str) -> String {
    init();
    if let Some(lock) = CATALOG.get() {
        if let Ok(catalog) = lock.read() {
            return catalog.get(key).to_string();
        }
    }
    key.to_string()
}

/// Translate a message key with positional format arguments.
pub fn t_fmt(key: &str, args: &[&str]) -> String {
    init();
    if let Some(lock) = CATALOG.get() {
        if let Ok(catalog) = lock.read() {
            return catalog.format(key, args);
        }
    }
    key.to_string()
}

/// Load custom translations from a JSON string.
pub fn load_translations(locale_str: &str, json: &str) -> Result<usize, String> {
    init();
    if let Some(lock) = CATALOG.get() {
        if let Ok(mut catalog) = lock.write() {
            let locale = Locale::from_str(locale_str);
            return catalog.load_json(locale, json);
        }
    }
    Err("Failed to acquire catalog lock".into())
}

/// Get all supported locale tags.
pub fn supported_locales() -> Vec<(&'static str, &'static str)> {
    Locale::all()
        .iter()
        .map(|l| (l.tag(), l.name()))
        .collect()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_parsing() {
        assert_eq!(Locale::from_str("en_US.UTF-8"), Locale::En);
        assert_eq!(Locale::from_str("de_DE.UTF-8"), Locale::De);
        assert_eq!(Locale::from_str("fr"), Locale::Fr);
        assert_eq!(Locale::from_str("es-ES"), Locale::Es);
        assert_eq!(Locale::from_str("ja_JP"), Locale::Ja);
        assert_eq!(Locale::from_str("zh-Hans"), Locale::Zh);
        assert_eq!(Locale::from_str("unknown"), Locale::En);
        assert_eq!(Locale::from_str(""), Locale::En);
    }

    #[test]
    fn test_locale_tags() {
        assert_eq!(Locale::En.tag(), "en");
        assert_eq!(Locale::De.tag(), "de");
        assert_eq!(Locale::Ja.tag(), "ja");
    }

    #[test]
    fn test_locale_names() {
        assert_eq!(Locale::En.name(), "English");
        assert_eq!(Locale::De.name(), "Deutsch");
        assert_eq!(Locale::Fr.name(), "Français");
        assert_eq!(Locale::Ja.name(), "日本語");
    }

    #[test]
    fn test_english_messages() {
        let catalog = MessageCatalog::new();
        // Force English
        assert_eq!(
            catalog.messages.get(&Locale::En).unwrap().get("write.started").unwrap(),
            "Write operation started"
        );
    }

    #[test]
    fn test_german_messages() {
        let mut catalog = MessageCatalog::new();
        catalog.set_locale(Locale::De);
        assert_eq!(catalog.get("write.started"), "Schreibvorgang gestartet");
    }

    #[test]
    fn test_french_messages() {
        let mut catalog = MessageCatalog::new();
        catalog.set_locale(Locale::Fr);
        assert_eq!(catalog.get("write.started"), "Écriture démarrée");
    }

    #[test]
    fn test_spanish_messages() {
        let mut catalog = MessageCatalog::new();
        catalog.set_locale(Locale::Es);
        assert_eq!(catalog.get("write.started"), "Escritura iniciada");
    }

    #[test]
    fn test_fallback_to_english() {
        let mut catalog = MessageCatalog::new();
        catalog.set_locale(Locale::Ko); // Korean has no translations loaded
        // Should fall back to English
        assert_eq!(catalog.get("write.started"), "Write operation started");
    }

    #[test]
    fn test_fallback_to_key() {
        let catalog = MessageCatalog::new();
        assert_eq!(catalog.get("nonexistent.key"), "nonexistent.key");
    }

    #[test]
    fn test_format_with_args() {
        let catalog = MessageCatalog::new();
        let result = catalog.format("write.progress", &["50%"]);
        assert_eq!(result, "Progress: 50%");
    }

    #[test]
    fn test_format_device_confirm() {
        let mut catalog = MessageCatalog::new();
        catalog.set_locale(Locale::De);
        let result = catalog.format("device.confirm", &["/dev/sdb"]);
        assert!(result.contains("/dev/sdb"));
        assert!(result.contains("ALLE DATEN WERDEN GELÖSCHT"));
    }

    #[test]
    fn test_load_custom_json() {
        let mut catalog = MessageCatalog::new();
        let json = r#"{"write.started": "Escrita iniciada", "write.complete": "Escrita concluída"}"#;
        let count = catalog.load_json(Locale::Pt, json).unwrap();
        assert_eq!(count, 2);

        catalog.set_locale(Locale::Pt);
        assert_eq!(catalog.get("write.started"), "Escrita iniciada");
        assert_eq!(catalog.get("write.complete"), "Escrita concluída");
    }

    #[test]
    fn test_message_count() {
        let catalog = MessageCatalog::new();
        assert!(catalog.message_count(Locale::En) > 30);
        assert!(catalog.message_count(Locale::De) > 25);
        assert!(catalog.message_count(Locale::Fr) > 20);
        assert_eq!(catalog.message_count(Locale::Ko), 0); // Not loaded
    }

    #[test]
    fn test_supported_locales() {
        let locales = supported_locales();
        assert!(locales.len() >= 12);
        assert!(locales.iter().any(|(tag, _)| *tag == "en"));
        assert!(locales.iter().any(|(tag, _)| *tag == "de"));
        assert!(locales.iter().any(|(tag, _)| *tag == "ja"));
    }

    #[test]
    fn test_global_t_function() {
        // Uses the global catalog (initialized lazily)
        let msg = t("write.started");
        // Should return English or whatever the system locale is
        assert!(!msg.is_empty());
        assert_ne!(msg, "write.started"); // Should not return the key itself
    }

    #[test]
    fn test_global_t_fmt_function() {
        let msg = t_fmt("write.source", &["/dev/sdb"]);
        assert!(msg.contains("/dev/sdb"));
    }

    #[test]
    fn test_all_locales_constant() {
        let all = Locale::all();
        assert_eq!(all.len(), 12);
        assert_eq!(all[0], Locale::En);
    }
}
