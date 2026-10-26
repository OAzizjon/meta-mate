//! Replays a recorded UIA fixture (`fixtures/uia/NAME.json`, written by `mm uia --save`) so
//! pointing logic runs without a desktop. Reading follows `tools/mm_fixture.py`.

use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::Path;

use mm_core::geom::{PhysPoint, PhysRect};
use serde::Deserialize;

use crate::error::PlatformError;
use crate::target::TargetWindow;
use crate::ui_snapshot::{UiElement, UiSnapshot, is_interactive_type};
use crate::ui_tree::UiTree;

/// The made-up id of the one recorded window a replay serves.
pub const REPLAY_TARGET: TargetWindow = TargetWindow {
    handle: 1,
    process_id: 1,
};

/// Serves `UiTree` from one fixture. The PNG is never decoded; its size comes from the JSON.
pub struct ReplayBackend {
    snapshot: UiSnapshot,
    screenshot_size: (u32, u32),
}

#[derive(Deserialize)]
struct RawFixture {
    window: RawWindow,
    nodes: Vec<RawNode>,
}

#[derive(Deserialize)]
struct RawWindow {
    frame: Option<[i64; 4]>,
    shot_frame: Option<[i64; 4]>,
    truncated: Option<bool>,
}

#[derive(Deserialize)]
struct RawNode {
    id: String,
    parent: Option<String>,
    #[serde(rename = "type")]
    control_type: String,
    name: Option<String>,
    rect: Option<[i64; 4]>,
    enabled: Option<bool>,
    offscreen: Option<bool>,
    password: Option<bool>,
    password_like: Option<bool>,
}

impl ReplayBackend {
    /// Loads `fixtures/uia/<name>.json` under the project `root`. The name is 1-60 ASCII
    /// letters, digits, `-` or `_`, so it cannot point outside that folder.
    pub fn load(root: &Path, name: &str) -> Result<Self, PlatformError> {
        if !is_valid_name(name) {
            return Err(invalid("name must be 1-60 letters, digits, - or _"));
        }
        let dir = root.join("fixtures").join("uia");
        match std::fs::read_to_string(dir.join(format!("{name}.json"))) {
            Ok(text) => Self::from_json(&text),
            Err(err) if err.kind() == ErrorKind::NotFound => Err(PlatformError::FixtureNotFound),
            Err(err) => Err(invalid(format!("cannot read: {err}"))),
        }
    }

    /// Parses fixture JSON. Unknown flags fail closed: a node with no `password` value counts
    /// as a password field, one with no `enabled`/`offscreen` value is never a hit.
    pub fn from_json(text: &str) -> Result<Self, PlatformError> {
        // PowerShell may write a byte order mark (the Python loader reads utf-8-sig).
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let raw: RawFixture = match serde_json::from_str(text) {
            Ok(raw) => raw,
            Err(err) => return Err(invalid(err.to_string())),
        };
        let Some(shot) = raw.window.shot_frame.or(raw.window.frame) else {
            return Err(invalid("window needs shot_frame or frame"));
        };
        let Some(frame) = to_rect(shot)? else {
            return Err(invalid("shot_frame must have a positive size"));
        };
        let screenshot_size = (to_u32(shot[2])?, to_u32(shot[3])?);

        let mut ids = HashSet::new();
        let mut elements = Vec::with_capacity(raw.nodes.len());
        for node in raw.nodes {
            if !ids.insert(node.id.clone()) {
                return Err(invalid(format!("duplicate node id {}", node.id)));
            }
            let rect = match node.rect {
                Some(rect) => to_rect(rect)?,
                None => None,
            };
            let interactive = is_interactive_type(&node.control_type);
            elements.push(UiElement {
                id: node.id,
                parent: node.parent,
                control_type: node.control_type,
                name: node.name.unwrap_or_default(),
                rect,
                interactive,
                enabled: node.enabled == Some(true),
                offscreen: node.offscreen != Some(false),
                is_password: node.password != Some(false),
                password_like: node.password_like != Some(false),
            });
        }

        let snapshot = UiSnapshot {
            target: REPLAY_TARGET,
            frame,
            elements,
            truncated: raw.window.truncated != Some(false),
        };
        Ok(Self {
            snapshot,
            screenshot_size,
        })
    }

    /// The recorded window; any other target is `WindowNotFound`.
    pub fn target(&self) -> TargetWindow {
        self.snapshot.target
    }

    /// Width and height of the fixture's PNG: the shot frame in physical pixels.
    pub fn screenshot_size(&self) -> (u32, u32) {
        self.screenshot_size
    }
}

impl UiTree for ReplayBackend {
    fn snapshot(&self, target: TargetWindow) -> Result<UiSnapshot, PlatformError> {
        if target != self.snapshot.target {
            return Err(PlatformError::WindowNotFound);
        }
        Ok(self.snapshot.clone())
    }

    fn element_from_point(&self, point: PhysPoint) -> Result<Option<UiElement>, PlatformError> {
        Ok(self.snapshot.element_at(point).cloned())
    }
}

fn invalid(reason: impl Into<String>) -> PlatformError {
    PlatformError::FixtureInvalid(reason.into())
}

/// 1-60 ASCII letters, digits, `-` or `_`, as in `tools/mm_fixture.py`.
fn is_valid_name(name: &str) -> bool {
    let allowed = |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'_';
    (1..=60).contains(&name.len()) && name.bytes().all(allowed)
}

/// `[x, y, width, height]` to a rect. Empty UIA rects are `None` (never a target); negative
/// sizes and coordinates beyond `i32` are errors.
fn to_rect([x, y, width, height]: [i64; 4]) -> Result<Option<PhysRect>, PlatformError> {
    if width < 0 || height < 0 {
        return Err(invalid("negative rectangle size"));
    }
    if width == 0 || height == 0 {
        return Ok(None);
    }
    Ok(Some(PhysRect {
        left: to_i32(x)?,
        top: to_i32(y)?,
        right: to_i32(x.saturating_add(width))?,
        bottom: to_i32(y.saturating_add(height))?,
    }))
}

fn to_i32(value: i64) -> Result<i32, PlatformError> {
    i32::try_from(value).map_err(|_| invalid("coordinate out of range"))
}

fn to_u32(value: i64) -> Result<u32, PlatformError> {
    u32::try_from(value).map_err(|_| invalid("size out of range"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// The project root, where `fixtures/` lives.
    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn text_size() -> ReplayBackend {
        ReplayBackend::load(&root(), "synthetic-text-size").expect("fixture loads")
    }

    fn hit(replay: &ReplayBackend, x: i32, y: i32) -> Option<UiElement> {
        replay
            .element_from_point(PhysPoint { x, y })
            .expect("a replay hit test never fails")
    }

    #[test]
    fn finds_large_by_point() {
        let large = hit(&text_size(), 1000, 270).expect("a hit");
        assert_eq!(large.id, "e11");
        assert_eq!(large.name, "Large");
        assert!(large.interactive);
    }

    #[test]
    fn finds_large_by_id() {
        let replay = text_size();
        let snapshot = replay.snapshot(replay.target()).expect("snapshot");
        assert_eq!(snapshot.elements.len(), 15);
        assert!(!snapshot.truncated);
        assert_eq!(replay.screenshot_size(), (1280, 800));
        let large = snapshot.element("e11").expect("e11 exists");
        assert_eq!(large.name, "Large");
        assert_eq!(large.control_type, "RadioButton");
        assert_eq!(large.parent.as_deref(), Some("e0"));
        assert!(!large.is_password && !large.password_like);
        let rect = large.rect.as_ref().expect("e11 has bounds");
        assert_eq!((rect.left, rect.top), (960, 255));
        assert_eq!((rect.right, rect.bottom), (1160, 287));
    }

    #[test]
    fn empty_space_hits_the_window_and_outside_hits_nothing() {
        let replay = text_size();
        let window = hit(&replay, 900, 600).expect("a hit");
        assert_eq!(window.id, "e0");
        assert!(!window.interactive);
        assert!(hit(&replay, -5, 10).is_none());
    }

    #[test]
    fn other_window_is_not_found() {
        let other = TargetWindow {
            handle: 99,
            process_id: 1,
        };
        let snapshot = text_size().snapshot(other);
        assert!(matches!(snapshot, Err(PlatformError::WindowNotFound)));
    }

    #[test]
    fn unknown_or_unsafe_fixture_names_are_errors() {
        let project = root();
        let missing = ReplayBackend::load(&project, "no-such-fixture");
        assert!(matches!(missing, Err(PlatformError::FixtureNotFound)));
        for name in ["", "../Cargo", "a/b", "synthetic text size"] {
            let loaded = ReplayBackend::load(&project, name);
            assert!(
                matches!(loaded, Err(PlatformError::FixtureInvalid(_))),
                "{name}"
            );
        }
    }

    #[test]
    fn malformed_json_is_an_error() {
        let cases = [
            "",
            "{ not json",
            "[]",
            r#"{"nodes": []}"#,
            r#"{"window": {"shot_frame": [0, 0, 10, 10]}}"#,
            r#"{"window": {}, "nodes": []}"#,
            r#"{"window": {"shot_frame": [0, 0, 0, 10]}, "nodes": []}"#,
            r#"{"window": {"shot_frame": [0, 0, 10.5, 10]}, "nodes": []}"#,
            r#"{"window": {"frame": [0, 0, 10, 10]}, "nodes": [{"id": 3, "type": "Button"}]}"#,
            r#"{"window": {"frame": [0, 0, 10, 10]}, "nodes": [{"id": "e0"}]}"#,
            r#"{"window": {"frame": [0, 0, 10, 10]}, "nodes": [{"id": "e0", "type": "Edit",
                "rect": [0, 0, -1, 5]}]}"#,
            r#"{"window": {"frame": [0, 0, 10, 10]}, "nodes": [{"id": "e0", "type": "Edit",
                "rect": [0, 0, 5]}]}"#,
            r#"{"window": {"frame": [0, 0, 10, 10]}, "nodes": [{"id": "e0", "type": "Edit",
                "rect": [0, 0, 5, 5]}, {"id": "e0", "type": "Edit", "rect": null}]}"#,
        ];
        for text in cases {
            let parsed = ReplayBackend::from_json(text);
            assert!(
                matches!(parsed, Err(PlatformError::FixtureInvalid(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn unknown_flags_fail_closed() {
        let json = r#"{"window": {"frame": [-100, 0, 50, 50]},
            "nodes": [{"id": "e0", "type": "Edit", "rect": [-90, 10, 20, 10]}]}"#;
        // Also checks that a leading byte order mark is accepted.
        let text = format!("\u{feff}{json}");
        let replay = ReplayBackend::from_json(&text).expect("minimal fixture loads");
        let snapshot = replay.snapshot(replay.target()).expect("snapshot");
        assert!(snapshot.truncated);
        let edit = snapshot.element("e0").expect("e0 exists");
        assert!(edit.is_password && edit.password_like && !edit.enabled && edit.offscreen);
        assert!(hit(&replay, -85, 15).is_none());
    }
}
