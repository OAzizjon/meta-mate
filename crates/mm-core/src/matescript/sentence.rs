// Segmentation depends only on characters, never on push boundaries or clocks.
#[derive(Default)]
pub(super) struct Sentence {
    chars: Vec<char>,
}

impl Sentence {
    pub(super) fn boundary(&self, next: Option<char>, tag: bool) -> bool {
        let mut end = self.chars.len();
        while end > 0 && self.chars[end - 1].is_whitespace() {
            end -= 1;
        }
        let spaced = end < self.chars.len();
        while end > 0 && closing(self.chars[end - 1]) {
            end -= 1;
        }
        if end == 0 {
            return false;
        }
        let last = self.chars[end - 1];
        if !terminator(last) {
            return false;
        }
        if next.is_some_and(closing) {
            return false;
        }
        if matches!(last, '。' | '！' | '？') {
            return true;
        }
        if !tag && next.is_some() && !spaced {
            return false;
        }
        if !tag && spaced && next.is_some_and(char::is_lowercase) {
            return false;
        }
        if last == '.' {
            let prefix: String = self.chars[..end].iter().collect();
            let word = prefix.split_whitespace().last().unwrap_or("");
            let word = word.trim_start_matches(['"', '“', '«', '(', '[']);
            let lower = word.to_lowercase();
            let abbreviations = [
                "dr.", "mr.", "mrs.", "e.g.", "i.e.", "etc.", "т.е.", "т.д.", "г.", "ул.",
            ];
            if abbreviations.contains(&lower.as_str()) {
                return false;
            }
            let mut letters = word.chars();
            if letters.next().is_some_and(char::is_uppercase)
                && letters.next() == Some('.')
                && letters.next().is_none()
            {
                return false;
            }
        }
        true
    }

    pub(super) fn push(&mut self, ch: char) -> Option<String> {
        if ch.is_whitespace() {
            if self.chars.last().is_some_and(|c| !c.is_whitespace()) {
                self.chars.push(' ');
            }
        } else {
            self.chars.push(ch);
        }
        if self.chars.len() <= 220 {
            return None;
        }
        let cut = self.chars[..220]
            .iter()
            .rposition(|c| matches!(c, ',' | ';' | ' '))
            .map_or(220, |index| index + 1);
        let prefix: String = self.chars.drain(..cut).collect();
        Some(prefix.trim().into())
    }

    pub(super) fn take(&mut self) -> String {
        let text: String = self.chars.drain(..).collect();
        text.trim().into()
    }
}

fn closing(ch: char) -> bool {
    matches!(ch, '"' | '”' | '»' | '’' | ')' | ']')
}
fn terminator(ch: char) -> bool {
    matches!(ch, '.' | '!' | '?' | '…' | '。' | '！' | '？')
}
