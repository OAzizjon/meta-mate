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

/// The shape of `Settings` in src/settings/model.ts. Missing fields take their defaults,
/// unknown fields are ignored.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub model: Model,
    pub ptt: Ptt,
    pub sound: Sound,
    /// `MIN_VOICE_SPEED..=MAX_VOICE_SPEED`.
    pub voice_speed: f64,
    pub robot_size: RobotSize,
    pub reduce_motion: bool,
    pub start_with_windows: bool,
    pub show_home_at_start: bool,
}

impl Default for Settings {
    /// `DEFAULT_SETTINGS` of src/settings/model.ts.
    fn default() -> Self {
        Self {
            model: Model::Quick,
            ptt: Ptt::Hold,
            sound: Sound::Calm,
            voice_speed: DEFAULT_VOICE_SPEED,
            robot_size: RobotSize::M,
            reduce_motion: false,
            start_with_windows: false,
            show_home_at_start: true,
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
        #[cfg(windows)]
        crate::ptt::set_ptt_mode(settings.ptt);
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
    let settings = settings.sanitized();
    let saved = {
        let mut current = store.lock();
        *current = settings.clone();
        VOICE_SPEED.store(settings.voice_speed.to_bits(), Ordering::Relaxed);
        #[cfg(windows)]
        crate::ptt::set_ptt_mode(settings.ptt);
        // Written under the lock: two saves cannot interleave and the file ends as the last.
        persist(&app, &current)
    };
    // Sent after the lock is released: a listener may read the store.
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
            model: Model::Thorough,
            ptt: Ptt::Toggle,
            sound: Sound::Fun,
            voice_speed: 1.15,
            robot_size: RobotSize::XL,
            reduce_motion: true,
            start_with_windows: true,
            show_home_at_start: false,
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
    fn defaults_match_the_window_model() {
        let defaults = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(
            defaults,
            json!({
                "model": "quick",
                "ptt": "hold",
                "sound": "calm",
                "voiceSpeed": 1.0,
                "robotSize": "M",
                "reduceMotion": false,
                "startWithWindows": false,
                "showHomeAtStart": true
            })
        );
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
