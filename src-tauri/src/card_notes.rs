//! Versioned keepsake sidecars; paths come only from validated local stems.

use base64::Engine;
use mm_core::shot::png_data_url;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const PNG_LIMIT: usize = 8 * 1024 * 1024;
const THUMB_LIMIT: usize = 256 * 1024;
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
        .then(|| png_data_url(&bytes))
}

fn thumbnail(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut decoder = png::Decoder::new(io::Cursor::new(bytes));
    decoder.set_limits(png::Limits {
        bytes: 32 * 1024 * 1024,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| invalid())?;
    let size = reader.output_buffer_size().ok_or_else(invalid)?;
    if size > 32 * 1024 * 1024 {
        return Err(invalid());
    }
    let mut pixels = vec![0; size];
    let info = reader.next_frame(&mut pixels).map_err(|_| invalid())?;
    let (width, height) = (info.width as usize, info.height as usize);
    let out_width = width.min(360);
    let out_height = (height * out_width / width).max(1);
    let channels = info.color_type.samples();
    let mut scaled = vec![0; out_width * out_height * channels];
    for y in 0..out_height {
        for x in 0..out_width {
            let (left, right) = (x * width / out_width, (x + 1) * width / out_width);
            let (top, bottom) = (y * height / out_height, (y + 1) * height / out_height);
            for channel in 0..channels {
                let mut sum = 0u64;
                for sy in top..bottom {
                    for sx in left..right {
                        sum += pixels[(sy * width + sx) * channels + channel] as u64;
                    }
                }
                scaled[(y * out_width + x) * channels + channel] =
                    (sum / ((right - left) * (bottom - top)) as u64) as u8;
            }
        }
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, out_width as u32, out_height as u32);
        encoder.set_color(info.color_type);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .map_err(|_| invalid())?
            .write_image_data(&scaled)
            .map_err(|_| invalid())?;
    }
    Ok(bytes)
}

fn thumb(folder: &Path, card: &SavedCard, remaining: &mut usize) -> Option<String> {
    card.image.as_ref()?;
    let name = format!("{}.thumb.png", card.id);
    if fs::symlink_metadata(folder.join(&name))
        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
        && *remaining > 0
    {
        *remaining -= 1;
        let bytes = bounded(folder, card.image.as_ref()?, PNG_LIMIT).ok()?;
        let bytes = thumbnail(&bytes).ok()?;
        atomic(folder, &name, &bytes).ok()?;
    }
    let bytes = bounded(folder, &name, THUMB_LIMIT).ok()?;
    bytes
        .starts_with(PNG_SIGNATURE)
        .then(|| png_data_url(&bytes))
}

pub(super) fn list(folder: &Path) -> Vec<CardNote> {
    if regular(folder, true).is_err() {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()?.to_str()? != "json" {
                return None;
            }
            let modified = fs::symlink_metadata(&path).ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    paths.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let mut cards: Vec<_> = paths
        .into_iter()
        .take(200)
        .filter_map(|(_, path)| load(folder, path.file_stem()?.to_str()?).ok())
        .collect();
    cards.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
    let mut remaining = 6;
    cards
        .into_iter()
        .take(60)
        .map(|card| {
            let thumb = thumb(folder, &card, &mut remaining);
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
            && let Ok(thumb) = thumbnail(png)
            && atomic(folder, &format!("{}.png", card.id), png).is_ok()
        {
            atomic(folder, &format!("{}.thumb.png", card.id), &thumb)?;
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
    let thumb = thumbnail(&bytes)?;
    atomic(folder, &format!("{id}.png"), &bytes)?;
    atomic(folder, &format!("{id}.thumb.png"), &thumb)?;
    card.image = Some(format!("{id}.png"));
    atomic(folder, &format!("{id}.json"), &json(&card)?)
}

pub(super) fn ensure_folder(folder: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    regular(folder, true)?;
    Ok(folder.to_path_buf())
}
