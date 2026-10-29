// Segmentation depends only on characters, never on push boundaries or clocks.
#[derive(Default)]
pub(super) struct Sentence {
    chars: Vec<char>,
    // Rolling boundary state: skip trailing whitespace/closers without rescans,
    // and retain only the current word's start (after leading opening quotes).
    end: usize,
    word_start: usize,
}

impl Sentence {
    pub(super) fn boundary(&self, next: Option<char>, tag: bool) -> bool {
        let end = self.end;
        let spaced = self.chars.last().is_some_and(|ch| ch.is_whitespace());
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
            // All abbreviations have at most four characters. A longer word
            // cannot match, so only collect a small tail, never the full prefix.
            if end - self.word_start > 4 {
                return true;
            }
            let word: String = self.chars[self.word_start..end].iter().collect();
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
                self.update_tail(' ', self.chars.len());
            }
        } else {
            self.chars.push(ch);
            self.update_tail(ch, self.chars.len());
        }
        if self.chars.len() <= 220 {
            return None;
        }
        let cut = self.chars[..220]
            .iter()
            .rposition(|c| matches!(c, ',' | ';' | ' '))
            .map_or(220, |index| index + 1);
        let prefix: String = self.chars.drain(..cut).collect();
        self.rebuild_tail();
        Some(prefix.trim().into())
    }

    pub(super) fn take(&mut self) -> String {
        let text: String = self.chars.drain(..).collect();
        self.end = 0;
        self.word_start = 0;
        text.trim().into()
    }

    fn update_tail(&mut self, ch: char, len: usize) {
        if !ch.is_whitespace() {
            if len > 1 && self.chars[len - 2].is_whitespace() {
                self.word_start = len - 1;
            }
            if !closing(ch) {
                self.end = len;
            } else if len > 1 && self.chars[len - 2].is_whitespace() {
                // Whitespace inside closing marks is not stripped by boundary.
                self.end = len - 1;
            }
            if self.word_start == len - 1 && matches!(ch, '"' | '“' | '«' | '(' | '[') {
                self.word_start = len;
            }
        }
    }

    fn rebuild_tail(&mut self) {
        self.end = 0;
        self.word_start = 0;
        // A forced 220-character split can leave part of a word. Rebuild once
        // per split, so that the retained suffix starts a fresh word as before.
        let len = self.chars.len();
        for index in 0..len {
            let ch = self.chars[index];
            self.update_tail(ch, index + 1);
        }
    }
}

fn closing(ch: char) -> bool {
    matches!(
        ch,
        '"' | '”' | '»' | '’' | ')' | ']' | '」' | '』' | '）' | '】' | '〉' | '》'
    )
}
fn terminator(ch: char) -> bool {
    matches!(ch, '.' | '!' | '?' | '…' | '。' | '！' | '？')
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Previous full-prefix implementation: an independent oracle for cached
    // state, including whitespace/quote edge cases and forced-cut resets.
    fn reference_boundary(chars: &[char], next: Option<char>, tag: bool) -> bool {
        let mut end = chars.len();
        while end > 0 && chars[end - 1].is_whitespace() {
            end -= 1;
        }
        let spaced = end < chars.len();
        while end > 0 && closing(chars[end - 1]) {
            end -= 1;
        }
        if end == 0 || !terminator(chars[end - 1]) || next.is_some_and(closing) {
            return false;
        }
        let last = chars[end - 1];
        if matches!(last, '。' | '！' | '？') {
            return true;
        }
        if (!tag && next.is_some() && !spaced)
            || (!tag && spaced && next.is_some_and(char::is_lowercase))
        {
            return false;
        }
        if last == '.' {
            let prefix: String = chars[..end].iter().collect();
            let word = prefix.split_whitespace().last().unwrap_or("");
            let word = word.trim_start_matches(['"', '“', '«', '(', '[']);
            let lower = word.to_lowercase();
            if [
                "dr.", "mr.", "mrs.", "e.g.", "i.e.", "etc.", "т.е.", "т.д.", "г.", "ул.",
            ]
            .contains(&lower.as_str())
            {
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

    fn assert_boundary(sentence: &Sentence) {
        for next in [None, Some('a'), Some('A'), Some('я'), Some('」'), Some('.')] {
            for tag in [false, true] {
                assert_eq!(
                    sentence.boundary(next, tag),
                    reference_boundary(&sentence.chars, next, tag),
                    "{:?}, next {next:?}, tag {tag}",
                    sentence.chars
                );
            }
        }
    }

    #[test]
    fn cached_tail_handles_abbreviations_and_long_quote_runs() {
        for text in [
            "Dr. Mr. Mrs. e.g. i.e. etc. т.е. т.д. г. ул. A. Я.",
            "\"Dr.\" “Mrs.” «т.е.» ([A.])",
            "Done. \" ) Done.\" ) Next。 」Ready！ ",
            &("(\"".repeat(150) + "Dr." + &")\"".repeat(150)),
        ] {
            let mut sentence = Sentence::default();
            for ch in text.chars() {
                sentence.push(ch);
                assert_boundary(&sentence);
            }
            sentence.take();
            assert_boundary(&sentence);
        }
    }

    proptest! {
        #[test]
        fn cached_boundaries_match_reference(
            chars in prop::collection::vec(
                prop::sample::select("aADrтЯ. !?。…\"“«()[]」 \t".chars().collect::<Vec<_>>()),
                0..700,
            )
        ) {
            let mut sentence = Sentence::default();
            for ch in chars {
                sentence.push(ch);
                assert_boundary(&sentence);
            }
        }
    }
}
