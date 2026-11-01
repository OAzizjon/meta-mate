use super::sentence::Sentence;
use super::tags::{self, Tag};
use super::{Bound, Item, Say, Settings};

#[derive(Default)]
enum State {
    #[default]
    Text,
    Lt,
    Tag(String, usize),
    Discard(usize),
}

/// Incremental, Unicode-safe MateScript parser for a single answer.
/// Concatenate the items returned by `push` and `finish` to obtain the timeline.
pub struct Parser {
    settings: Settings,
    state: State,
    sentence: Sentence,
    bound: Vec<Bound>,
    output: Vec<Item>,
    sfx_count: usize,
    next_id: u64,
    language_emitted: bool,
    finished: bool,
    open_seen: bool,
    pending_open: Option<super::OpenTarget>,
}

impl Parser {
    /// Start an answer with the supplied validation context.
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            state: State::Text,
            sentence: Sentence::default(),
            bound: Vec::new(),
            output: Vec::new(),
            sfx_count: 0,
            next_id: 1,
            language_emitted: false,
            finished: false,
            open_seen: false,
            pending_open: None,
        }
    }

    /// Consume a chunk and return newly available items. Chunks may be empty.
    /// Input after `finish` or an explicit end tag is ignored.
    pub fn push(&mut self, chunk: &str) -> Vec<Item> {
        for ch in chunk.chars() {
            if self.finished {
                break;
            }
            self.character(ch);
        }
        std::mem::take(&mut self.output)
    }

    /// Flush remaining speech and unbound actions, rejecting partial pointing tags.
    /// Repeated calls return no further items; EOF does not invent an end tag.
    pub fn finish(&mut self) -> Vec<Item> {
        if !self.finished {
            if let State::Tag(raw, _) = std::mem::take(&mut self.state)
                && (raw.starts_with("point") || raw.starts_with("step"))
            {
                self.rejected(&raw);
            }
            self.flush();
            self.orphans();
            self.language();
            self.finished = true;
            self.state = State::Text;
        }
        std::mem::take(&mut self.output)
    }

    fn character(&mut self, ch: char) {
        match std::mem::take(&mut self.state) {
            State::Text => {
                if ch == '<' {
                    self.state = State::Lt;
                } else {
                    self.text(ch);
                }
            }
            State::Lt => {
                // A closing "</mood>" is a (bad) tag too; "A<B" stays text.
                if ch.is_ascii_lowercase() || ch == '/' {
                    if self.sentence.boundary(None, true) {
                        self.flush();
                    }
                    self.state = State::Tag(ch.into(), 1);
                } else {
                    self.text('<');
                    if ch == '<' {
                        self.state = State::Lt;
                    } else {
                        self.text(ch);
                    }
                }
            }
            State::Tag(mut raw, count) => {
                if ch == '>' {
                    self.tag(&raw);
                } else if ch == '\n' || ch == '\r' || count + 1 >= tag_limit(&raw) {
                    self.rejected(&raw);
                    if ch == '\n' || ch == '\r' {
                        self.flush();
                    }
                    // Keep discarding the broken tag's tail (golden case 5).
                    self.state = State::Discard(0);
                } else {
                    raw.push(ch);
                    self.state = State::Tag(raw, count + 1);
                }
            }
            State::Discard(count) => {
                if ch == '\n' || ch == '\r' {
                    self.flush();
                } else if ch != '>' && count + 1 < 256 {
                    self.state = State::Discard(count + 1);
                }
            }
        }
    }

    fn text(&mut self, ch: char) {
        if ch == '\n' || ch == '\r' {
            self.flush();
            return;
        }
        if !ch.is_whitespace() && self.sentence.boundary(Some(ch), false) {
            self.flush();
        }
        if let Some(text) = self.sentence.push(ch) {
            self.say(text);
        }
    }

    fn tag(&mut self, raw: &str) {
        match tags::parse(raw, &self.settings, &mut self.sfx_count) {
            Some(Tag::Bound(bound)) => {
                // Sfx accumulate; singleton actions use the last source occurrence.
                if !matches!(bound, Bound::Sfx(_)) {
                    self.bound.retain(|old| {
                        std::mem::discriminant(old) != std::mem::discriminant(&bound)
                    });
                }
                self.bound.push(bound);
            }
            Some(Tag::Item(item)) => {
                // A positional action follows all speech already encountered.
                self.flush();
                if let Item::Open(target) = item {
                    // The person did not ask to open anything: the tag is dropped, so text
                    // on screen can never make Mate open a site or an app.
                    let asked = self
                        .settings
                        .open_request
                        .as_deref()
                        .is_some_and(|request| target.named_in(request));
                    if asked && !self.open_seen {
                        self.open_seen = true;
                        if self.next_id > 1 {
                            self.output.push(Item::Open(target));
                        } else {
                            self.pending_open = Some(target);
                        }
                    }
                    return;
                }
                // Waiting for a click needs the pointer up first: pending actions play
                // before the wait instead of riding on the next sentence.
                if matches!(item, Item::End | Item::Wait) {
                    self.orphans();
                }
                if matches!(item, Item::End) {
                    self.finished = true;
                }
                if matches!(item, Item::Lang(_)) {
                    self.language_emitted = true;
                } else {
                    self.language();
                }
                self.output.push(item);
            }
            None => self.rejected(raw),
        }
    }

    fn language(&mut self) {
        if !self.language_emitted {
            self.output.push(Item::Lang(self.settings.language.clone()));
            self.language_emitted = true;
        }
    }

    fn rejected(&mut self, raw: &str) {
        if raw.starts_with("point") || raw.starts_with("step") {
            self.flush();
            self.orphans();
            self.language();
            self.output.push(Item::PointRejected);
            return;
        }
        self.language();
        self.output.push(Item::BadTag);
    }

    fn flush(&mut self) {
        let text = self.sentence.take();
        if !text.is_empty() {
            self.say(text);
        }
    }

    fn say(&mut self, text: String) {
        self.language();
        self.output.push(Item::Say(Say {
            id: self.next_id,
            text,
            bound: std::mem::take(&mut self.bound),
        }));
        self.next_id = self.next_id.saturating_add(1);
        if let Some(target) = self.pending_open.take() {
            self.output.push(Item::Open(target));
        }
    }

    fn orphans(&mut self) {
        if !self.bound.is_empty() {
            self.language();
            self.output.extend(self.bound.drain(..).map(Item::Bound));
        }
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new(Settings::default())
    }
}

/// The longest tag body kept while reading: an `<open>` URL up to 200 characters, a `<show>`
/// text up to `SHOW_MAX` (three example questions for "What can you do?"), anything else 64.
fn tag_limit(raw: &str) -> usize {
    if raw.starts_with("open ") {
        206
    } else if raw.starts_with("show ") {
        super::tags::SHOW_MAX + 6
    } else {
        64
    }
}
