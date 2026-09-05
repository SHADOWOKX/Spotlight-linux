use std::{
    env, fs,
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;
pub const APPLICATION_ID: &str = "io.github.shadowokx.SpotlightLinux";
pub const DEFAULT_LAUNCHER_SHORTCUT: &str = "ALT+space";

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub general: GeneralSettings,
    pub appearance: AppearanceSettings,
    pub search: SearchSettings,
    pub keyboard: KeyboardSettings,
    pub privacy: PrivacySettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            general: GeneralSettings::default(),
            appearance: AppearanceSettings::default(),
            search: SearchSettings::default(),
            keyboard: KeyboardSettings::default(),
            privacy: PrivacySettings::default(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), SettingsValidationError> {
        if !(16..=26).contains(&self.appearance.search_font_size)
            || !(12..=18).contains(&self.appearance.result_font_size)
        {
            return Err(SettingsValidationError::FontSize);
        }
        if self.schema_version == 0 || self.schema_version > CURRENT_SCHEMA_VERSION {
            return Err(SettingsValidationError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if !(0.55..=1.0).contains(&self.appearance.transparency) {
            return Err(SettingsValidationError::Transparency(
                self.appearance.transparency,
            ));
        }
        if !(1..=100).contains(&self.search.maximum_results) {
            return Err(SettingsValidationError::MaximumResults(
                self.search.maximum_results,
            ));
        }
        if !(4..=10).contains(&self.appearance.visible_results) {
            return Err(SettingsValidationError::VisibleResults(
                self.appearance.visible_results,
            ));
        }
        validate_shortcut(&self.keyboard.launcher_shortcut)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    pub launch_at_login: bool,
    pub close_after_action: bool,
    pub remember_last_query: bool,
    pub renderer: RendererPreference,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            close_after_action: true,
            remember_last_query: false,
            renderer: RendererPreference::OpenGl,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RendererPreference {
    #[default]
    OpenGl,
    GtkDefault,
    Vulkan,
    Software,
}

impl RendererPreference {
    pub fn environment_value(self) -> Option<&'static str> {
        match self {
            Self::OpenGl => Some("gl"),
            Self::GtkDefault => None,
            Self::Vulkan => Some("vulkan"),
            Self::Software => Some("cairo"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    pub palette: Palette,
    pub search_font_size: u32,
    pub result_font_size: u32,
    pub show_result_type: bool,
    pub theme: Theme,
    pub window_style: WindowStyle,
    pub transparency: f64,
    pub blur: BlurPreference,
    pub corner_radius: CornerRadius,
    pub window_width: WindowWidth,
    pub density: Density,
    pub result_row_height: ResultRowHeight,
    pub animations: AnimationPreference,
    pub accent: Accent,
    pub icon_size: IconSize,
    pub visible_results: u32,
    pub show_subtitles: bool,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            palette: Palette::Native,
            search_font_size: 20,
            result_font_size: 14,
            show_result_type: true,
            theme: Theme::System,
            window_style: WindowStyle::Normal,
            transparency: 0.90,
            blur: BlurPreference::Auto,
            corner_radius: CornerRadius::Medium,
            window_width: WindowWidth::Standard,
            density: Density::Comfortable,
            result_row_height: ResultRowHeight::Standard,
            animations: AnimationPreference::Full,
            accent: Accent::Graphite,
            icon_size: IconSize::Standard,
            visible_results: 6,
            show_subtitles: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    #[default]
    Graphite,
    System,
    Blue,
    Violet,
    Green,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Palette {
    #[default]
    Native,
    Graphite,
    Midnight,
    Dusk,
    Forest,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconSize {
    Small,
    #[default]
    Standard,
    Large,
}

impl IconSize {
    pub fn pixels(self) -> i32 {
        match self {
            Self::Small => 24,
            Self::Standard => 28,
            Self::Large => 36,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowStyle {
    #[default]
    Normal,
    Glass,
    Minimal,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlurPreference {
    Off,
    #[default]
    Auto,
    WhenSupported,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerRadius {
    Small,
    #[default]
    Medium,
    Large,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowWidth {
    Compact,
    #[default]
    Standard,
    Wide,
}

impl WindowWidth {
    pub fn pixels(self) -> i32 {
        match self {
            Self::Compact => 600,
            Self::Standard => 680,
            Self::Wide => 760,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    #[default]
    Comfortable,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultRowHeight {
    Compact,
    #[default]
    Standard,
    Spacious,
}

impl ResultRowHeight {
    pub fn pixels(self) -> i32 {
        match self {
            Self::Compact => 44,
            Self::Standard => 52,
            Self::Spacious => 60,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationPreference {
    #[default]
    Full,
    Reduced,
    Off,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchSettings {
    pub calculator_enabled: bool,
    pub show_suggestions: bool,
    pub maximum_results: usize,
    pub applications_enabled: bool,
    pub show_latency: bool,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            calculator_enabled: true,
            show_suggestions: true,
            maximum_results: 8,
            applications_enabled: true,
            show_latency: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyboardSettings {
    /// XDG Shortcuts specification syntax, not a GTK accelerator string.
    pub launcher_shortcut: String,
}

impl Default for KeyboardSettings {
    fn default() -> Self {
        Self {
            launcher_shortcut: DEFAULT_LAUNCHER_SHORTCUT.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacySettings {
    pub usage_history: bool,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            usage_history: true,
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum SettingsValidationError {
    #[error("search font must be 16–26px and result font 12–18px")]
    FontSize,
    #[error("unsupported settings schema version {0}")]
    UnsupportedSchema(u32),
    #[error("transparency must be between 0.55 and 1.0, got {0}")]
    Transparency(f64),
    #[error("maximum_results must be between 1 and 100, got {0}")]
    MaximumResults(usize),
    #[error("visible_results must be between 4 and 10, got {0}")]
    VisibleResults(u32),
    #[error("invalid global shortcut: {0}")]
    Shortcut(String),
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("could not read settings: {0}")]
    Read(#[source] io::Error),
    #[error("could not parse settings: {0}")]
    Parse(#[from] toml::de::Error),
    #[error(transparent)]
    Validation(#[from] SettingsValidationError),
    #[error("could not serialize settings: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("could not write settings atomically: {0}")]
    Write(#[source] io::Error),
}

#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Settings, SettingsError> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(error) => return Err(SettingsError::Read(error)),
        };
        let settings: Settings = toml::from_str(&contents)?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        settings.validate()?;
        let contents = toml::to_string_pretty(settings)?;
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(SettingsError::Write)?;

        let mut temporary_path = None;
        let mut temporary_file = None;
        for _ in 0..16 {
            let suffix = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let candidate = parent.join(format!(
                ".{}.{}.{}.tmp",
                self.path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("settings"),
                std::process::id(),
                suffix
            ));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&candidate)
            {
                Ok(file) => {
                    temporary_path = Some(candidate);
                    temporary_file = Some(file);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(SettingsError::Write(error)),
            }
        }

        let temporary_path = temporary_path.ok_or_else(|| {
            SettingsError::Write(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "could not allocate a unique settings temporary file",
            ))
        })?;
        let mut file = temporary_file.expect("temporary path and file are assigned together");
        let result = (|| -> io::Result<()> {
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary_path, &self.path)?;
            if let Ok(directory) = fs::File::open(parent) {
                let _ = directory.sync_all();
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        result.map_err(SettingsError::Write)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XdgPaths {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
    pub autostart_file: PathBuf,
}

#[derive(Debug, Error)]
pub enum XdgPathError {
    #[error("HOME is unavailable and an XDG base directory is not configured")]
    MissingHome,
}

impl XdgPaths {
    pub fn from_process() -> Result<Self, XdgPathError> {
        let home = env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let config_home = xdg_home("XDG_CONFIG_HOME", home.as_ref(), ".config")?;
        let cache_home = xdg_home("XDG_CACHE_HOME", home.as_ref(), ".cache")?;
        let data_home = xdg_home("XDG_DATA_HOME", home.as_ref(), ".local/share")?;
        Ok(Self {
            config_dir: config_home.join("spotlight-linux"),
            cache_dir: cache_home.join("spotlight-linux"),
            data_dir: data_home.join("spotlight-linux"),
            autostart_file: config_home
                .join("autostart")
                .join(format!("{APPLICATION_ID}.desktop")),
        })
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn history_file(&self) -> PathBuf {
        self.data_dir.join("history.sqlite3")
    }
}

fn xdg_home(
    variable: &str,
    home: Option<&PathBuf>,
    fallback: &str,
) -> Result<PathBuf, XdgPathError> {
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|home| home.join(fallback)))
        .ok_or(XdgPathError::MissingHome)
}

/// Validates the deliberately small keyboard subset in the XDG Shortcuts spec.
pub fn validate_shortcut(shortcut: &str) -> Result<(), SettingsValidationError> {
    let parts = shortcut.split('+').collect::<Vec<_>>();
    let Some(key) = parts.last() else {
        return Err(SettingsValidationError::Shortcut(shortcut.into()));
    };
    if key.is_empty()
        || !key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(SettingsValidationError::Shortcut(shortcut.into()));
    }
    let mut modifiers = Vec::new();
    for modifier in &parts[..parts.len() - 1] {
        if !matches!(*modifier, "CTRL" | "ALT" | "SHIFT" | "NUM" | "LOGO")
            || modifiers.contains(modifier)
        {
            return Err(SettingsValidationError::Shortcut(shortcut.into()));
        }
        modifiers.push(*modifier);
    }
    if modifiers.is_empty() {
        return Err(SettingsValidationError::Shortcut(shortcut.into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn missing_file_loads_safe_defaults() {
        let directory = tempdir().unwrap();
        let store = SettingsStore::new(directory.path().join("config.toml"));
        assert_eq!(store.load().unwrap(), Settings::default());
    }

    #[test]
    fn partial_configuration_gets_nested_defaults() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(
            &path,
            "schema_version = 1\n[appearance]\nwindow_style = \"glass\"\n",
        )
        .unwrap();
        let settings = SettingsStore::new(path).load().unwrap();
        assert_eq!(settings.appearance.window_style, WindowStyle::Glass);
        assert!(settings.search.show_suggestions);
        assert_eq!(settings.keyboard.launcher_shortcut, "ALT+space");
    }

    #[test]
    fn save_is_private_and_round_trips() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("nested/config.toml");
        let store = SettingsStore::new(&path);
        let mut settings = Settings::default();
        settings.appearance.theme = Theme::Dark;
        settings.appearance.accent = Accent::Violet;
        settings.appearance.visible_results = 10;
        settings.appearance.icon_size = IconSize::Large;
        settings.appearance.show_subtitles = false;
        settings.appearance.palette = Palette::Dusk;
        settings.appearance.search_font_size = 24;
        settings.appearance.result_font_size = 16;
        settings.appearance.show_result_type = false;
        settings.search.calculator_enabled = false;
        settings.search.show_latency = true;
        settings.search.show_suggestions = false;
        settings.general.renderer = RendererPreference::GtkDefault;
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap(), settings);
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn invalid_values_are_rejected() {
        let mut settings = Settings::default();
        settings.search.maximum_results = 0;
        assert!(matches!(
            settings.validate(),
            Err(SettingsValidationError::MaximumResults(0))
        ));
        settings.search.maximum_results = 8;
        settings.keyboard.launcher_shortcut = "$(touch_bad)".into();
        assert!(matches!(
            settings.validate(),
            Err(SettingsValidationError::Shortcut(_))
        ));
    }

    #[test]
    fn visible_result_bounds_and_legacy_defaults() {
        let mut settings: Settings = toml::from_str("[appearance]\ntheme = 'dark'\n").unwrap();
        assert_eq!(settings.appearance.accent, Accent::Graphite);
        assert_eq!(settings.appearance.visible_results, 6);
        for invalid in [0, 3, 11, u32::MAX] {
            settings.appearance.visible_results = invalid;
            assert_eq!(
                settings.validate(),
                Err(SettingsValidationError::VisibleResults(invalid))
            );
        }
    }

    #[test]
    fn shortcut_uses_xdg_trigger_grammar() {
        assert!(validate_shortcut(DEFAULT_LAUNCHER_SHORTCUT).is_ok());
        assert!(validate_shortcut("LOGO+space").is_ok());
        assert!(validate_shortcut("CTRL+SHIFT+Return").is_ok());
        assert!(validate_shortcut("CTRL+CTRL+x").is_err());
        assert!(validate_shortcut("space").is_err());
        assert!(validate_shortcut("<Super>space").is_err());
    }
}
