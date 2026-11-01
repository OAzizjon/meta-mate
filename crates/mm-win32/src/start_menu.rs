//! Bounded Start Menu discovery; shortcuts stay opaque and never become model input.

use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Shortcut {
    pub name: String,
    path: PathBuf,
}

fn plain(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_attributes() & 0x400 == 0) // FILE_ATTRIBUTE_REPARSE_POINT
}

fn roots() -> Vec<PathBuf> {
    ["APPDATA", "ProgramData"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|base| PathBuf::from(base).join("Microsoft/Windows/Start Menu/Programs"))
        .collect()
}

/// Never descend through a junction, symlink or another reparse point, even at the root.
fn enumerate(roots: &[PathBuf]) -> Vec<Shortcut> {
    let mut found = Vec::new();
    let mut pending: Vec<_> = roots
        .iter()
        .filter(|root| root.is_absolute() && root.ancestors().all(plain))
        .map(|root| (root.clone(), Vec::<String>::new()))
        .collect();
    let mut visited = 0;
    while let Some((folder, components)) = pending.pop() {
        if !folder.ancestors().all(plain) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.flatten() {
            // Count every entry, including folders, to bound traversal as well as results.
            if visited == 2000 {
                return Vec::new();
            }
            visited += 1;
            let path = entry.path();
            if !plain(&path) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                let mut parts = components.clone();
                parts.push(entry.file_name().to_string_lossy().into_owned());
                let refs: Vec<_> = parts.iter().map(String::as_str).collect();
                if !mm_core::installed_apps::blocked("", &refs) {
                    pending.push((path, parts));
                }
                continue;
            }
            if !kind.is_file()
                || !path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        ["lnk", "appref-ms"]
                            .iter()
                            .any(|allowed| ext.eq_ignore_ascii_case(allowed))
                    })
            {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let refs: Vec<_> = components.iter().map(String::as_str).collect();
            if name.chars().any(|ch| ch.is_control())
                || mm_core::installed_apps::blocked(name, &refs)
                || !target_allowed(&path)
            {
                continue;
            }
            found.push(Shortcut {
                name: name.to_owned(),
                path,
            });
        }
    }
    found
}

/// The shortcut's own bytes (at most 64 KiB) must not name a blocked program: a shortcut called
/// "Family" can point at AnyDesk or cmd.exe. Unreadable or larger files are not offered.
fn target_allowed(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() <= 64 * 1024 => std::fs::read(path)
            .is_ok_and(|bytes| !mm_core::installed_apps::shortcut_targets_blocked(&bytes)),
        _ => false,
    }
}

pub fn installed() -> Vec<Shortcut> {
    enumerate(&roots())
}

/// Recheck discovery at the shell boundary. No arguments and only the `open` verb.
pub fn open(shortcut: &Shortcut, wanted: &dyn Fn() -> bool) -> bool {
    if !installed()
        .iter()
        .any(|current| current.path == shortcut.path && current.name == shortcut.name)
    {
        return false;
    }
    let Some(path) = shortcut.path.to_str() else {
        return false;
    };
    wanted() && crate::shell::open_shortcut_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_menu_is_recursive_filters_extensions_and_excludes_unsafe_folders() {
        let root = std::env::temp_dir().join(format!(
            "mm-start-menu-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("Family")).expect("menu");
        std::fs::create_dir_all(root.join("Startup")).expect("startup");
        for file in [
            "Family/Telegram.lnk",
            "Zoom.url",
            "Word.appref-ms",
            "Ignore.exe",
            "AnyDesk.lnk",
            "Uninstall Zoom.lnk",
            "Startup/Skype.lnk",
        ] {
            std::fs::write(root.join(file), []).expect("fixture");
        }
        // A friendly name that points at a remote-access tool (the target path is inside).
        let target: Vec<u8> = "C:/Program Files/AnyDesk/AnyDesk.exe"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        std::fs::write(root.join("Family/Grandson help.lnk"), target).expect("fixture");
        let mut names: Vec<_> = enumerate(std::slice::from_ref(&root))
            .into_iter()
            .map(|app| app.name)
            .collect();
        names.sort();
        // .url shortcuts are never offered (a .url can carry file: or ms-* targets).
        assert_eq!(names, ["Telegram", "Word"]);
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
