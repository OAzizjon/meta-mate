use super::vocabulary::{AMBIENCE, MOODS, SOUNDS, closest};
use super::{Bound, Item, Level, Mode, Point, Settings, Sound, Step};

pub(super) enum Tag {
    Bound(Bound),
    Item(Item),
}

/// The longest `<show>` text: room for three short example questions (the capability answer).
pub(super) const SHOW_MAX: usize = 120;

pub(super) fn parse(raw: &str, settings: &Settings, sfx_count: &mut usize) -> Option<Tag> {
    let (name, payload) = raw.split_once(' ').unwrap_or((raw, ""));
    if !(2..=6).contains(&name.len()) || !name.bytes().all(|b| b.is_ascii_lowercase()) {
        return None;
    }
    if name == "show" {
        return (!payload.is_empty()
            && payload.chars().count() <= SHOW_MAX
            && !payload.contains(['<', '>', '\n', '\r']))
        .then(|| Tag::Bound(Bound::Show(payload.into())));
    }
    if name == "open" {
        return super::OpenTarget::parse(payload).map(|target| Tag::Item(Item::Open(target)));
    }
    let args: Vec<&str> = if payload.is_empty() {
        Vec::new()
    } else {
        payload.split(' ').collect()
    };
    if args.iter().any(|arg| {
        arg.is_empty()
            || arg.len() > 32
            || !arg
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.,/:+-".contains(&b))
    }) {
        return None;
    }
    match (name, args.as_slice()) {
        ("lang", [code])
            if matches!(code.len(), 2 | 3) && code.bytes().all(|b| b.is_ascii_lowercase()) =>
        {
            Some(Tag::Item(Item::Lang((*code).into())))
        }
        ("mood", [mood]) => {
            closest(mood, MOODS.iter().copied()).map(|mood| Tag::Bound(Bound::Mood(mood.into())))
        }
        ("point", [target]) => point(target, settings).map(|p| Tag::Bound(Bound::Point(p))),
        ("step", [position, target]) => {
            let (number, total) = position.split_once('/')?;
            let number = number.parse::<u32>().ok()?;
            let total = total.parse::<u32>().ok()?;
            if number == 0 || number > total {
                return None;
            }
            Some(Tag::Bound(Bound::Step(Step {
                number,
                total,
                target: point(target, settings)?,
            })))
        }
        ("wait", ["click"]) => Some(Tag::Item(Item::Wait)),
        ("pause", [ms]) => {
            // Saturate long numeric arguments before clamping, including u64 overflow.
            let digits = ms.strip_prefix(['-', '+']).unwrap_or(ms);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let value = if ms.starts_with('-') {
                0
            } else {
                digits.bytes().fold(0u32, |n, b| {
                    n.saturating_mul(10).saturating_add(u32::from(b - b'0'))
                })
            };
            Some(Tag::Item(Item::Pause(value.clamp(100, 3000))))
        }
        ("end", []) => Some(Tag::Item(Item::End)),
        ("amb", ["off"]) => Some(Tag::Item(Item::Amb {
            name: None,
            level: Level::Soft,
        })),
        ("amb", [amb]) | ("amb", [amb, _]) if AMBIENCE.contains(amb) => {
            let level = match args.get(1) {
                None | Some(&"soft") => Level::Soft,
                Some(&"normal") => Level::Normal,
                _ => return None,
            };
            Some(Tag::Item(Item::Amb {
                name: Some((*amb).into()),
                level,
            }))
        }
        ("sfx", [sound, ..]) => {
            let canonical = closest(sound, SOUNDS.iter().map(|s| s.0))?;
            let &(_, default_level, calm) = SOUNDS.iter().find(|s| s.0 == canonical)?;
            let mut level = default_level;
            let mut solo = false;
            let mut options = &args[1..];
            if let Some(option) = options.first()
                && let Some(explicit) = level_name(option)
            {
                level = explicit;
                options = &options[1..];
            }
            if options == ["solo"] {
                solo = true;
            } else if !options.is_empty() {
                return None;
            }
            let limit = if settings.mode == Mode::Calm { 2 } else { 4 };
            if *sfx_count >= limit || (settings.mode == Mode::Calm && !calm) {
                return None;
            }
            if settings.mode == Mode::Calm {
                level = Level::Soft;
            }
            *sfx_count += 1;
            let sound = Sound {
                name: canonical.into(),
                level,
                solo,
            };
            Some(if solo {
                Tag::Item(Item::Sfx(sound))
            } else {
                Tag::Bound(Bound::Sfx(sound))
            })
        }
        _ => None,
    }
}

fn level_name(name: &str) -> Option<Level> {
    match name {
        "soft" => Some(Level::Soft),
        "normal" => Some(Level::Normal),
        "loud" => Some(Level::Loud),
        _ => None,
    }
}

fn point(raw: &str, settings: &Settings) -> Option<Point> {
    if let Some((x, y)) = raw.split_once(',') {
        let x = x.parse::<u32>().ok()?;
        let y = y.parse::<u32>().ok()?;
        let (width, height) = settings.screenshot?;
        (x < width && y < height).then_some(Point::Pixel(x, y))
    } else {
        settings
            .point_ids
            .iter()
            .any(|id| id == raw)
            .then(|| Point::Id(raw.into()))
    }
}
