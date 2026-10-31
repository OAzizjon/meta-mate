//! The user's settings (WP30): the settings window (settings.html, src/settings/model.ts)
//! reads them with `load_settings` and saves them with `save_settings`. They live in
//! memory (`SettingsStore`) and in `settings.json` in the app's config folder, written
//! atomically. A missing or broken file gives the defaults, never an error the user sees.
//! Every save reaches every window as a `settings-changed` event. `startWithWindows` is
//! only stored: starting with Windows is the installer's concern.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

pub use mm_core::api::profile::Profile;
use mm_core::api::request::ModelChoice;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

const FILE_NAME: &str = "settings.json";
/// Written first, then renamed over `FILE_NAME`: a crash mid-write leaves the old file.
const TEMP_NAME: &str = "settings.json.tmp";
/// What the window gets when saving fails: no path, so no user name.
const SAVE_FAILED: &str = "could not save settings";
/// Sent to every window after a save, with the saved settings as its payload.
const CHANGED_EVENT: &str = "settings-changed";

pub const MIN_VOICE_SPEED: f64 = 0.8;
pub const MAX_VOICE_SPEED: f64 = 1.3;
pub const DEFAULT_VOICE_SPEED: f64 = 1.0;
/// The steps of the window's range input (`VOICE_SPEED_STEP` in src/settings/model.ts).
pub const VOICE_SPEED_STEP: f64 = 0.05;

/// The store's voice speed as `f64` bits, for the TTS call (proxy.rs), which holds no app
/// handle. Kept in step with the store.
static VOICE_SPEED: AtomicU64 = AtomicU64::new(DEFAULT_VOICE_SPEED.to_bits());
static LANGUAGE: AtomicU64 = AtomicU64::new(0);

/// The UI language preference; automatic follows Windows.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Auto,
    En,
    Ru,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

fn deserialize_theme<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Theme, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

impl Language {
    pub fn resolve(self, system: &str) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ru => "ru",
            Self::Auto if system == "ru" => "ru",
            Self::Auto => "en",
        }
    }
}

fn deserialize_language<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Language, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

pub fn language() -> Language {
    match LANGUAGE.load(Ordering::Relaxed) {
        1 => Language::En,
        2 => Language::Ru,
        _ => Language::Auto,
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    Quick,
    Thorough,
}

impl From<Model> for ModelChoice {
    /// Quick is Muse Spark, Thorough is Muse Wonder (mm-core maps them to model ids).
    fn from(model: Model) -> Self {
        match model {
            Model::Quick => Self::Quick,
            Model::Thorough => Self::Thorough,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Ptt {
    Hold,
    Toggle,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PttKey {
    #[default]
    RightCtrl,
    RightAlt,
    CapsLock,
    Insert,
    F9,
    F10,
}

impl PttKey {
    pub fn virtual_key(self) -> u32 {
        match self {
            Self::RightCtrl => 0xA3,
            Self::RightAlt => 0xA5,
            Self::CapsLock => 0x14,
            Self::Insert => 0x2D,
            Self::F9 => 0x78,
            Self::F10 => 0x79,
        }
    }

    pub fn locale_key(self) -> &'static str {
        match self {
            Self::RightCtrl => "settings.key_rightCtrl",
            Self::RightAlt => "settings.key_rightAlt",
            Self::CapsLock => "settings.key_capsLock",
            Self::Insert => "settings.key_insert",
            Self::F9 => "settings.key_f9",
            Self::F10 => "settings.key_f10",
        }
    }
}

fn deserialize_ptt_key<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<PttKey, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Sound {
    Calm,
    Fun,
}

/// Mate's size, named as in the window: "S", "M", "L", "XL".
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RobotSize {
    S,
    M,
    L,
    XL,
}

/// Mate's resting top-left corner, in physical virtual-screen pixels. Rust owns it:
/// saving preferences from an already open settings window must not reset the home.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MateHome {
    pub x: i32,
    pub y: i32,
}

/// The shape of `Settings` in src/settings/model.ts. Missing fields take their defaults,
/// unknown fields are ignored.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub profile: Profile,
    #[serde(default)]
    pub profile_asked: bool,
    #[serde(default, deserialize_with = "deserialize_language")]
    pub language: Language,
    #[serde(default, deserialize_with = "deserialize_theme")]
    pub theme: Theme,
    pub model: Model,
    pub ptt: Ptt,
    #[serde(default, deserialize_with = "deserialize_ptt_key")]
    pub ptt_key: PttKey,
    pub sound: Sound,
    /// `MIN_VOICE_SPEED..=MAX_VOICE_SPEED`.
    pub voice_speed: f64,
    pub robot_size: RobotSize,
    pub reduce_motion: bool,
    pub spotlight: bool,
    pub start_with_windows: bool,
    pub show_home_at_start: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mate_home: Option<MateHome>,
}

impl Default for Settings {
    /// `DEFAULT_SETTINGS` of src/settings/model.ts.
    fn default() -> Self {
        Self {
            profile: Profile::default(),
            profile_asked: false,
            language: Language::Auto,
            theme: Theme::Light,
            model: Model::Quick,
            ptt: Ptt::Hold,
            ptt_key: PttKey::RightCtrl,
            sound: Sound::Calm,
            voice_speed: DEFAULT_VOICE_SPEED,
            robot_size: RobotSize::M,
            reduce_motion: false,
            spotlight: true,
            start_with_windows: false,
            show_home_at_start: true,
            mate_home: None,
        }
    }
}

impl Settings {
    /// The rules of `parseSettings` (src/settings/model.ts): a voice speed outside
    /// `MIN_VOICE_SPEED..=MAX_VOICE_SPEED` (NaN and infinities too) becomes the default; one
    /// inside snaps to the nearest `VOICE_SPEED_STEP`, rounded to 2 decimals. The other
    /// fields cannot be out of range once deserialized.
    pub fn sanitized(self) -> Settings {
        let voice_speed = if (MIN_VOICE_SPEED..=MAX_VOICE_SPEED).contains(&self.voice_speed) {
            let stepped = (self.voice_speed / VOICE_SPEED_STEP).round() * VOICE_SPEED_STEP;
            (stepped * 100.0).round() / 100.0
        } else {
            DEFAULT_VOICE_SPEED
        };
        Settings {
            profile: self.profile.sanitized(),
            voice_speed,
            ..self
        }
    }
}

/// The current settings, managed by the app.
#[derive(Default)]
pub struct SettingsStore(Mutex<Settings>);

impl SettingsStore {
    pub fn new(settings: Settings) -> Self {
        let settings = settings.sanitized();
        VOICE_SPEED.store(settings.voice_speed.to_bits(), Ordering::Relaxed);
        LANGUAGE.store(settings.language as u64, Ordering::Relaxed);
        #[cfg(windows)]
        crate::ptt::set_ptt_mode(settings.ptt);
        #[cfg(windows)]
        crate::ptt::set_ptt_key(settings.ptt_key);
        Self(Mutex::new(settings))
    }

    /// A copy of the current settings.
    pub fn get(&self) -> Settings {
        self.lock().clone()
    }

    /// The settings are plain data, whole even if a holder panicked: a poisoned lock is
    /// still used.
    fn lock(&self) -> MutexGuard<'_, Settings> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[tauri::command]
pub fn load_settings(store: State<'_, SettingsStore>) -> Settings {
    store.get()
}

/// Sanitizes, keeps and saves `settings`, then sends them to every window. They stay in
/// memory, and in effect, even when the file cannot be written.
#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    store: State<'_, SettingsStore>,
    settings: Settings,
) -> Result<(), String> {
    let mut settings = settings.sanitized();
    let saved = {
        let mut current = store.lock();
        settings.mate_home = current.mate_home;
        *current = settings.clone();
        LANGUAGE.store(settings.language as u64, Ordering::Relaxed);
        VOICE_SPEED.store(settings.voice_speed.to_bits(), Ordering::Relaxed);
        #[cfg(windows)]
        crate::ptt::set_ptt_mode(settings.ptt);
        #[cfg(windows)]
        crate::ptt::set_ptt_key(settings.ptt_key);
        // Written under the lock: two saves cannot interleave and the file ends as the last.
        persist(&app, &current)
    };
    // Sent after the lock is released: a listener may read the store.
    crate::refresh_tray_labels(&app);
    if let Err(error) = app.emit(CHANGED_EVENT, settings) {
        eprintln!("settings: could not tell the windows: {error}");
    }
    saved.map_err(|error| {
        eprintln!("settings: could not save: {error}");
        SAVE_FAILED.to_owned()
    })
}

/// The model the person chose, read from the store at this moment; Quick before the
/// store exists.
pub fn model(app: &AppHandle) -> Model {
    app.try_state::<SettingsStore>()
        .map_or(Model::Quick, |store| store.get().model)
}

pub fn profile(app: &AppHandle) -> Profile {
    app.try_state::<SettingsStore>()
        .map_or_else(Profile::default, |store| store.get().profile)
}

/// Persists a drag release under the same lock as preference saves.
#[cfg(windows)]
pub fn save_mate_home(app: &AppHandle, rect: mm_core::geom::PhysRect) {
    let store = app.state::<SettingsStore>();
    let settings = {
        let mut current = store.lock();
        current.mate_home = Some(MateHome {
            x: rect.left,
            y: rect.top,
        });
        if let Err(error) = persist(app, &current) {
            eprintln!("settings: could not save Mate's home: {error}");
        }
        current.clone()
    };
    let _ = app.emit(CHANGED_EVENT, settings);
}

/// The voice speed of the current settings (`DEFAULT_VOICE_SPEED` before the store exists).
pub fn voice_speed() -> f64 {
    f64::from_bits(VOICE_SPEED.load(Ordering::Relaxed))
}

/// The saved settings, at startup.
pub fn load(app: &AppHandle) -> Settings {
    match app.path().app_config_dir() {
        Ok(dir) => read(&dir),
        Err(_) => Settings::default(),
    }
}

/// The error text names no path (io and Tauri path errors carry none).
fn persist(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    write(&dir, settings).map_err(|error| error.to_string())
}

/// `settings.json` in `dir`; the defaults when it is missing, unreadable or broken.
fn read(dir: &Path) -> Settings {
    match fs::read_to_string(dir.join(FILE_NAME)) {
        Ok(text) => parse(&text),
        Err(_) => Settings::default(),
    }
}

/// Settings from the text of `settings.json`; the defaults when it is not valid. A BOM
/// (left by an editor) is skipped.
fn parse(text: &str) -> Settings {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    serde_json::from_str::<Settings>(text).map_or_else(|_| Settings::default(), Settings::sanitized)
}

/// Writes `settings.json` in `dir` atomically: pretty UTF-8 JSON goes to
/// `settings.json.tmp`, reaches the disk, then replaces the old file in one rename.
fn write(dir: &Path, settings: &Settings) -> io::Result<()> {
    let mut text = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
    text.push('\n');
    fs::create_dir_all(dir)?;
    let temp = dir.join(TEMP_NAME);
    {
        let mut file = File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    // On Windows `fs::rename` replaces an existing file (MOVEFILE_REPLACE_EXISTING).
    fs::rename(&temp, dir.join(FILE_NAME))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    fn changed() -> Settings {
        Settings {
            profile: Profile::default(),
            profile_asked: false,
            language: Language::Auto,
            theme: Theme::Light,
            model: Model::Thorough,
            ptt: Ptt::Toggle,
            ptt_key: PttKey::RightCtrl,
            sound: Sound::Fun,
            voice_speed: 1.15,
            robot_size: RobotSize::XL,
            reduce_motion: true,
            spotlight: true,
            start_with_windows: true,
            show_home_at_start: false,
            mate_home: None,
        }
    }

    /// An empty folder of its own for each test (tests run in parallel).
    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("metamate-settings-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn language_migration_validation_and_resolution() {
        for (value, expected) in [
            (json!("auto"), Language::Auto),
            (json!("en"), Language::En),
            (json!("ru"), Language::Ru),
            (json!("de"), Language::Auto),
            (json!(null), Language::Auto),
            (json!(42), Language::Auto),
        ] {
            let settings = parse(&json!({"language": value, "sound": "fun"}).to_string());
            assert_eq!(settings.language, expected);
            assert_eq!(settings.sound, Sound::Fun);
            let restored = parse(&serde_json::to_string(&settings).unwrap());
            assert_eq!(restored, settings);
        }
        let old = parse(r#"{"model":"thorough","sound":"fun"}"#);
        assert_eq!(old.language, Language::Auto);
        assert_eq!(old.model, Model::Thorough);
        assert_eq!(old.sound, Sound::Fun);
        assert_eq!(Language::Auto.resolve("ru"), "ru");
        assert_eq!(Language::Auto.resolve("en"), "en");
        assert_eq!(Language::Auto.resolve("de"), "en");
        assert_eq!(Language::En.resolve("ru"), "en");
        assert_eq!(Language::Ru.resolve("en"), "ru");
    }

    #[test]
    fn theme_defaults_validates_and_round_trips_without_resetting_other_preferences() {
        assert_eq!(parse(r#"{"sound":"fun"}"#).theme, Theme::Light);
        for (value, expected) in [
            (json!("light"), Theme::Light),
            (json!("dark"), Theme::Dark),
            (json!("future"), Theme::Light),
            (json!("Dark"), Theme::Light),
            (json!(null), Theme::Light),
            (json!(42), Theme::Light),
            (json!({}), Theme::Light),
        ] {
            let settings = parse(&json!({"theme": value, "sound": "fun"}).to_string());
            assert_eq!(settings.theme, expected);
            assert_eq!(settings.sound, Sound::Fun);
            assert_eq!(parse(&serde_json::to_string(&settings).unwrap()), settings);
        }
    }

    #[test]
    fn defaults_match_the_window_model() {
        let defaults = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(
            defaults,
            json!({
                "profile": {"name": "", "about": ""},
                "profileAsked": false,
                "language": "auto",
                "theme": "light",
                "model": "quick",
                "ptt": "hold",
                "pttKey": "rightCtrl",
                "sound": "calm",
                "voiceSpeed": 1.0,
                "robotSize": "M",
                "reduceMotion": false,
                "spotlight": true,
                "startWithWindows": false,
                "showHomeAtStart": true
            })
        );
    }

    #[test]
    fn profiles_load_from_old_settings_sanitize_and_round_trip() {
        let old = parse(r#"{"sound":"fun"}"#);
        assert_eq!(old.profile, Profile::default());
        assert!(!old.profile_asked);
        let settings = parse(
            &json!({
                "profile": {"name": "  <Анна>  ", "about": "я ".repeat(400)},
                "profileAsked": true
            })
            .to_string(),
        );
        assert_eq!(settings.profile.name, "Анна");
        assert!(settings.profile.about.chars().count() <= 300);
        assert!(settings.profile_asked);
        assert_eq!(parse(&serde_json::to_string(&settings).unwrap()), settings);
    }

    #[test]
    fn talk_keys_parse_and_old_or_unknown_keys_keep_other_preferences() {
        for (key, vk) in [
            (PttKey::RightCtrl, 0xA3),
            (PttKey::RightAlt, 0xA5),
            (PttKey::CapsLock, 0x14),
            (PttKey::Insert, 0x2D),
            (PttKey::F9, 0x78),
            (PttKey::F10, 0x79),
        ] {
            let value = serde_json::to_value(key).unwrap();
            let text = json!({"pttKey": value, "sound": "fun"}).to_string();
            let settings = parse(&text);
            assert_eq!(settings.ptt_key, key);
            assert_eq!(key.virtual_key(), vk);
            assert_eq!(settings.sound, Sound::Fun);
        }
        for text in [
            r#"{"sound":"fun"}"#,
            r#"{"sound":"fun","pttKey":"future"}"#,
            r#"{"sound":"fun","pttKey":null}"#,
            r#"{"sound":"fun","pttKey":4}"#,
        ] {
            let settings = parse(text);
            assert_eq!(settings.ptt_key, PttKey::RightCtrl);
            assert_eq!(settings.sound, Sound::Fun);
        }
    }

    #[test]
    fn every_choice_has_the_window_name() {
        fn name(value: impl Serialize) -> serde_json::Value {
            serde_json::to_value(value).unwrap()
        }
        assert_eq!(name(Model::Thorough), json!("thorough"));
        assert_eq!(name(Ptt::Toggle), json!("toggle"));
        assert_eq!(name(Sound::Fun), json!("fun"));
        assert_eq!(name(RobotSize::S), json!("S"));
        assert_eq!(name(RobotSize::L), json!("L"));
        assert_eq!(name(RobotSize::XL), json!("XL"));
    }

    #[test]
    fn saved_home_round_trips_with_home_window_preferences() {
        let settings = parse(r#"{"mateHome":{"x":-1200,"y":640},"showHomeAtStart":false}"#);
        assert_eq!(settings.mate_home, Some(MateHome { x: -1200, y: 640 }));
        assert!(!settings.show_home_at_start);
        assert_eq!(parse(&serde_json::to_string(&settings).unwrap()), settings);
        assert_eq!(parse("{}").mate_home, None);
    }

    #[test]
    fn a_voice_speed_in_range_is_kept() {
        for speed in [MIN_VOICE_SPEED, 0.85, 1.0, 1.15, MAX_VOICE_SPEED] {
            let settings = Settings {
                voice_speed: speed,
                ..changed()
            };
            assert_eq!(settings.clone().sanitized(), settings);
        }
    }

    #[test]
    fn a_voice_speed_snaps_to_the_step_like_the_window() {
        // The cases of src/settings/model.test.ts.
        for (speed, snapped) in [(1.03, 1.05), (0.82, 0.8), (1.299, 1.3), (0.875, 0.9)] {
            let settings = Settings {
                voice_speed: speed,
                ..changed()
            };
            assert_eq!(
                settings.sanitized(),
                Settings {
                    voice_speed: snapped,
                    ..changed()
                },
                "{speed}"
            );
        }
        assert_eq!(
            parse(r#"{"voiceSpeed":1.12}"#),
            Settings {
                voice_speed: 1.1,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn a_voice_speed_out_of_range_becomes_the_default_and_nothing_else_changes() {
        for speed in [
            0.0,
            -1.0,
            0.79,
            1.31,
            5.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let settings = Settings {
                voice_speed: speed,
                ..changed()
            };
            assert_eq!(
                settings.sanitized(),
                Settings {
                    voice_speed: DEFAULT_VOICE_SPEED,
                    ..changed()
                },
                "{speed}"
            );
        }
    }

    #[test]
    fn settings_survive_a_round_trip_through_json() {
        let text = serde_json::to_string_pretty(&changed()).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&text).unwrap(), changed());
        assert_eq!(parse(&text), changed());
        let defaults = serde_json::to_string(&Settings::default()).unwrap();
        assert_eq!(parse(&defaults), Settings::default());
    }

    #[test]
    fn the_window_payload_is_read_as_sent() {
        let sent = r#"{"model":"thorough","ptt":"toggle","sound":"fun","voiceSpeed":1.15,
            "robotSize":"XL","reduceMotion":true,"startWithWindows":true,
            "showHomeAtStart":false}"#;
        assert_eq!(serde_json::from_str::<Settings>(sent).unwrap(), changed());
    }

    #[test]
    fn missing_fields_take_their_defaults_and_unknown_fields_are_ignored() {
        let settings = parse(r#"{"model":"thorough","robotSize":"L","theme":"dark","volume":3}"#);
        assert_eq!(
            settings,
            Settings {
                model: Model::Thorough,
                robot_size: RobotSize::L,
                theme: Theme::Dark,
                ..Settings::default()
            }
        );
        assert_eq!(parse("{}"), Settings::default());
    }

    #[test]
    fn old_settings_show_home_and_the_new_preference_round_trips() {
        // A saved file from before Home keeps every existing preference.
        let old = r#"{"model":"thorough","ptt":"toggle","sound":"fun","voiceSpeed":1.15,
            "robotSize":"XL","reduceMotion":true,"startWithWindows":true}"#;
        assert_eq!(
            parse(old),
            Settings {
                show_home_at_start: true,
                ..changed()
            }
        );
        for show in [true, false] {
            let text = format!(r#"{{"showHomeAtStart":{show}}}"#);
            let settings = parse(&text);
            assert_eq!(settings.show_home_at_start, show);
            assert_eq!(parse(&serde_json::to_string(&settings).unwrap()), settings);
        }
    }

    #[test]
    fn a_saved_voice_speed_out_of_range_becomes_the_default() {
        let settings = parse(r#"{"sound":"fun","voiceSpeed":9}"#);
        assert_eq!(
            settings,
            Settings {
                sound: Sound::Fun,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn corrupt_input_gives_the_defaults() {
        for text in [
            "",
            "   ",
            "{",
            "not json",
            "null",
            "42",
            "\"quick\"",
            r#"{"model":"slow"}"#,
            r#"{"robotSize":"XXL"}"#,
            r#"{"voiceSpeed":"fast"}"#,
            r#"{"reduceMotion":"yes"}"#,
            r#"{"showHomeAtStart":"yes"}"#,
            r#"{"showHomeAtStart":null}"#,
            r#"{"model":null}"#,
            r#"{"model":"thorough"}garbage"#,
        ] {
            assert_eq!(parse(text), Settings::default(), "{text}");
        }
    }

    #[test]
    fn a_bom_is_skipped() {
        assert_eq!(
            parse("\u{feff}{\"ptt\":\"toggle\"}"),
            Settings {
                ptt: Ptt::Toggle,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn the_store_keeps_only_sanitized_settings() {
        let store = SettingsStore::new(Settings {
            voice_speed: 2.0,
            ..changed()
        });
        assert_eq!(
            store.get(),
            Settings {
                voice_speed: DEFAULT_VOICE_SPEED,
                ..changed()
            }
        );
        assert_eq!(SettingsStore::default().get(), Settings::default());
    }

    #[test]
    fn a_written_file_is_read_back_and_replaced_by_the_next_save() {
        let dir = folder("write");
        write(&dir, &changed()).unwrap();
        assert_eq!(read(&dir), changed());
        let text = fs::read_to_string(dir.join(FILE_NAME)).unwrap();
        assert!(text.contains("\n  \"voiceSpeed\": 1.15,\n"), "{text}");

        write(&dir, &Settings::default()).unwrap();
        assert_eq!(read(&dir), Settings::default());
        assert!(!dir.join(TEMP_NAME).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_corrupt_file_gives_the_defaults() {
        let dir = folder("corrupt");
        assert_eq!(read(&dir), Settings::default());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(FILE_NAME), "{\"model\":").unwrap();
        assert_eq!(read(&dir), Settings::default());
        fs::write(dir.join(FILE_NAME), b"\xff\xfe\x00").unwrap();
        assert_eq!(read(&dir), Settings::default());
        let _ = fs::remove_dir_all(&dir);
    }
}
