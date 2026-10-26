use super::Level;

// Canonical names, default levels and calm eligibility from docs/sounds.md.
pub(super) const SOUNDS: &[(&str, Level, bool)] = &[
    ("ping", Level::Soft, true),
    ("success", Level::Soft, true),
    ("oops", Level::Soft, true),
    ("think", Level::Soft, true),
    ("listen", Level::Soft, true),
    ("done", Level::Soft, true),
    ("whoosh", Level::Soft, true),
    ("pop", Level::Soft, true),
    ("sleep", Level::Soft, true),
    ("wake", Level::Soft, true),
    ("birds", Level::Soft, true),
    ("rooster", Level::Normal, false),
    ("cat", Level::Normal, true),
    ("dog", Level::Normal, false),
    ("owl", Level::Soft, true),
    ("horse", Level::Normal, false),
    ("knock", Level::Normal, true),
    ("clock", Level::Soft, true),
    ("bell", Level::Normal, true),
    ("rimshot", Level::Normal, false),
    ("drumroll", Level::Normal, false),
    ("applause", Level::Normal, true),
    ("cheer", Level::Normal, false),
    ("magic", Level::Soft, true),
    ("fanfare", Level::Loud, false),
    ("thunder", Level::Loud, false),
    ("wind", Level::Normal, true),
    ("splash", Level::Normal, true),
    ("boom", Level::Loud, false),
    ("laser", Level::Normal, false),
    ("boing", Level::Normal, true),
    ("slip", Level::Normal, false),
    ("giggle", Level::Soft, true),
    ("footsteps", Level::Soft, true),
    ("door", Level::Normal, false),
    ("phone", Level::Normal, false),
    ("train", Level::Normal, false),
    ("car", Level::Loud, false),
    ("heart", Level::Soft, false),
    ("tada", Level::Normal, true),
    ("sad", Level::Normal, false),
];

pub(super) const MOODS: &[&str] = &[
    "calm",
    "happy",
    "thinking",
    "surprised",
    "sad",
    "proud",
    "sleepy",
];
pub(super) const AMBIENCE: &[&str] = &["forest", "rain", "sea", "fire", "night", "park", "cafe"];

// Exact matches take precedence. Ties use vocabulary order for reproducibility.
pub(super) fn closest<'a>(input: &str, names: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    names
        .map(|name| (name, distance(input, name)))
        .filter(|(_, distance)| *distance <= 2)
        .min_by_key(|(_, distance)| *distance)
        .map(|(name, _)| name)
}

fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ac) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, bc) in b.iter().enumerate() {
            let old = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ac != *bc))
                .min(row[j] + 1)
                .min(old + 1);
            diagonal = old;
        }
    }
    row[b.len()]
}
