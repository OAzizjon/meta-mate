//! Versioned keepsake sidecars; paths come only from validated local stems.

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const PNG_LIMIT: usize = 8 * 1024 * 1024;
const THUMB_LIMIT: usize = 2 * 1024 * 1024;
const JSON_LIMIT: usize = 64 * 1024;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
pub(super) struct SavedCard {
    pub version: u32,
    pub id: String,
    pub title: String,
    pub question: String,
    pub steps: Vec<String>,
    pub created: u64,
    pub image: Option<String>,
}

#[derive(Serialize)]
pub struct CardNote {
    pub id: String,
    pub title: String,
    pub steps: Vec<String>,
    pub created: u64,
    pub thumb: Option<String>,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid card")
}

pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= 180
        && !id.contains("..")
        && !id.ends_with(['.', ' '])
        && !id
            .chars()
            .any(|c| c.is_control() || r#"/\:<>"|?*"#.contains(c))
}

fn regular(path: &Path, directory: bool) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(invalid());
        }
    }
    if meta.file_type().is_symlink()
        || (directory && !meta.is_dir())
        || (!directory && !meta.is_file())
    {
        return Err(invalid());
    }
    Ok(())
}

fn bounded(folder: &Path, name: &str, limit: usize) -> io::Result<Vec<u8>> {
    regular(folder, true)?;
    let path = folder.join(name);
    regular(&path, false)?;
    if path.canonicalize()?.parent() != Some(folder.canonicalize()?.as_path()) {
        return Err(invalid());
    }
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid());
    }
    Ok(bytes)
}

pub(super) fn load(folder: &Path, id: &str) -> io::Result<SavedCard> {
    if !valid_id(id) {
        return Err(invalid());
    }
    let bytes = bounded(folder, &format!("{id}.json"), JSON_LIMIT)?;
    let card: SavedCard = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if card.version != 1
        || card.id != id
        || card.title.trim().is_empty()
        || card.steps.is_empty()
        || card.steps.len() > super::MAX_STEPS
        || card
            .image
            .as_ref()
            .is_some_and(|image| image != &format!("{id}.png"))
    {
        return Err(invalid());
    }
    Ok(card)
}

pub(super) fn image(folder: &Path, card: &SavedCard, limit: usize) -> Option<String> {
    let bytes = bounded(folder, card.image.as_ref()?, limit).ok()?;
    bytes
        .starts_with(PNG_SIGNATURE)
        .then(|| super::data_url(&bytes))
}

pub(super) fn list(folder: &Path) -> Vec<CardNote> {
    if regular(folder, true).is_err() {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut cards: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()?.to_str()? != "json" {
                return None;
            }
            load(folder, path.file_stem()?.to_str()?).ok()
        })
        .collect();
    cards.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
    cards
        .into_iter()
        .take(60)
        .map(|card| {
            let thumb = image(folder, &card, THUMB_LIMIT);
            CardNote {
                id: card.id,
                title: card.title,
                steps: card.steps,
                created: card.created,
                thumb,
            }
        })
        .collect()
}

fn json(card: &SavedCard) -> io::Result<Vec<u8>> {
    serde_json::to_vec_pretty(card).map_err(|_| invalid())
}

fn atomic(folder: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    regular(folder, true)?;
    let destination = folder.join(name);
    match fs::symlink_metadata(&destination) {
        Ok(_) => regular(&destination, false)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temp = folder.join(format!(".card-{}-{nonce}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, &destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

pub(super) fn save(
    folder: &Path,
    stem: &str,
    mut card: SavedCard,
    png: Option<&[u8]>,
) -> io::Result<SavedCard> {
    fs::create_dir_all(folder)?;
    regular(folder, true)?;
    for copy in 1..=super::SAME_NAME_LIMIT {
        let id = if copy == 1 {
            stem.to_owned()
        } else {
            format!("{stem} ({copy})")
        };
        if !valid_id(&id) {
            return Err(invalid());
        }
        if fs::symlink_metadata(folder.join(format!("{id}.png"))).is_ok() {
            continue;
        }
        let path = folder.join(format!("{id}.json"));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        card.id = id;
        card.image = None;
        file.write_all(&json(&card)?)?;
        file.sync_all()?;
        drop(file);
        if let Some(png) =
            png.filter(|png| png.len() <= PNG_LIMIT && png.starts_with(PNG_SIGNATURE))
            && atomic(folder, &format!("{}.png", card.id), png).is_ok()
        {
            card.image = Some(format!("{}.png", card.id));
            atomic(folder, &format!("{}.json", card.id), &json(&card)?)?;
        }
        return Ok(card);
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "card names taken",
    ))
}

pub(super) fn decode_png(encoded: &str) -> io::Result<Vec<u8>> {
    if encoded.len() > PNG_LIMIT.div_ceil(3) * 4 {
        return Err(invalid());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| invalid())?;
    if bytes.len() > PNG_LIMIT || !bytes.starts_with(PNG_SIGNATURE) {
        return Err(invalid());
    }
    Ok(bytes)
}

pub(super) fn save_captioned(folder: &Path, id: &str, encoded: &str) -> io::Result<()> {
    let mut card = load(folder, id)?;
    let bytes = decode_png(encoded)?;
    atomic(folder, &format!("{id}.png"), &bytes)?;
    card.image = Some(format!("{id}.png"));
    atomic(folder, &format!("{id}.json"), &json(&card)?)
}

pub(super) fn ensure_folder(folder: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    regular(folder, true)?;
    Ok(folder.to_path_buf())
}
