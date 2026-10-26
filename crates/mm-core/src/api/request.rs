//! Building a Responses API request body (docs/prompts.md, D41).
//!
//! Pure builders: each returns the JSON body for `POST /v1/responses`; the proxy client
//! sends it. The prompts live here as `const`s (model input, not UI text, so invariant 7
//! does not apply) and get `ROBOT_NAME` at build time (invariant 6).
//!
//! Every `image_data_url` must be a redacted screenshot (invariant 3, convention 4):
//! password fields painted over before the data URL was made. A `RedactedImage` type
//! replaces the `&str` later.

use std::fmt;

use serde::Serialize;
use serde_json::{Value, json};

use crate::constants::ROBOT_NAME;
use crate::geom::{FrameTransform, PhysRect, ShotRect};
use crate::matescript::Mode;

/// The model chosen in Settings (D40); never switched by voice or by a tag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum ModelChoice {
    /// Muse Spark 1.3, the default.
    #[default]
    #[serde(rename = "muse-spark-1.3")]
    Quick,
    /// Muse Wonder 1.0; the proxy falls back to Spark when the allowance runs out.
    #[serde(rename = "muse-wonder-1.0")]
    Thorough,
}

/// `reasoning.effort`. Every Spark reasons before it answers (D41): the first token came
/// after 1.9 s at `minimal`, 3.2 s at `low` and 7-10 s at the default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Minimal,
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    /// Spoken answers: chat and "where is X".
    pub const SPOKEN: Self = Self::Minimal;
    /// Computer use: `minimal` and `low` both hit the fixture's target (D41); `minimal`
    /// was faster (5.3 s vs 6.9 s).
    pub const COMPUTER_USE: Self = Self::Minimal;
    /// Retrying a step the person could not complete (docs/prompts.md §2).
    pub const RETRY: Self = Self::Low;
}

/// `max_output_tokens` of a chat answer; the proxy caps it again (D40).
pub const SPOKEN_MAX_OUTPUT_TOKENS: u32 = 400;

/// Screen turns reason over the screenshot first and reasoning counts as output: the
/// fixture's computer-use turns took 356 (`minimal`) and 570 (`low`) output tokens.
pub const SCREEN_MAX_OUTPUT_TOKENS: u32 = 1200;

/// What stays the same for one teacher task. Follow-up turns repeat the first request's
/// model, effort, instructions and tool: the screenshot size is fixed for the whole task
/// (docs/architecture.md §3 B.1) and `previous_response_id` does not carry instructions.
/// The size is ours (the frame transform): Meta's computer tool takes none (D41).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComputerSession {
    pub model: ModelChoice,
    pub effort: ReasoningEffort,
    pub sent_width: u32,
    pub sent_height: u32,
}

const NAME_MAX_CHARS: usize = 60;
const HIDDEN_NAME: &str = "[hidden]";
const ELEMENT_LIST_HEADER: &str =
    "Interactive elements (id, type, name, [x, y, w, h] in screenshot pixels):";

/// One UI Automation element as the model sees it (docs/architecture.md §3 A.2).
/// `Debug` shows metadata only (convention 3): names can be private content.
#[derive(Clone, PartialEq)]
pub struct ElementLine {
    id: String,
    control_type: String,
    name: String,
    rect: ShotRect,
    interactive: bool,
    hidden: bool,
}

impl ElementLine {
    /// `rect` is the element's physical rectangle; it is clipped to the capture and mapped
    /// to screenshot pixels with this frame's transform. `hidden` marks password fields,
    /// password-like fields (D37: `IsPassword` alone is not enough) and anything the
    /// redaction painted over: their names are dropped here and never reach a prompt.
    /// `None` when `id` is not `e` + digits or the element lies outside the capture.
    pub fn new(
        id: &str,
        control_type: &str,
        name: &str,
        rect: PhysRect,
        frame: FrameTransform,
        interactive: bool,
        hidden: bool,
    ) -> Option<Self> {
        if !is_element_id(id) {
            return None;
        }
        // `shot_rect_to_phys` clamps to the capture, so the round trip clips the element
        // to the screenshot like the probe's list (`tools/mm_fixture.py`).
        let phys = frame.shot_rect_to_phys(frame.phys_rect_to_shot(rect));
        let c = frame.phys_rect_to_shot(phys);
        let rect = ShotRect::new(c.left, c.top, c.right, c.bottom).ok()?;
        Some(Self {
            id: id.to_owned(),
            control_type: clean_control_type(control_type),
            name: if hidden {
                String::new()
            } else {
                clean_name(name)
            },
            rect,
            interactive,
            hidden,
        })
    }

    /// `e11 RadioButton "Large" [960,255,200,32]`, rounded like `geom`.
    fn render(&self) -> String {
        let (id, kind) = (&self.id, &self.control_type);
        let left = self.rect.left.round();
        let top = self.rect.top.round();
        let width = (self.rect.right.round() - left) as i64;
        let height = (self.rect.bottom.round() - top) as i64;
        let (x, y) = (left as i64, top as i64);
        let name = if self.hidden {
            HIDDEN_NAME.to_owned()
        } else {
            quoted(&self.name)
        };
        format!("{id} {kind} {name} [{x},{y},{width},{height}]")
    }
}

impl fmt::Debug for ElementLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElementLine")
            .field("id", &self.id)
            .field("control_type", &self.control_type)
            .field("name_chars", &self.name.chars().count())
            .field("rect", &self.rect)
            .field("interactive", &self.interactive)
            .field("hidden", &self.hidden)
            .finish()
    }
}

/// The element list for the model, one line per element in the probe's format:
/// interactive elements first, slice order kept within both groups, at most `max`
/// element lines, then `… N more` when some were left out. Hidden elements show
/// `[hidden]` instead of a name.
pub fn render_element_list(elements: &[ElementLine], max: usize) -> String {
    let ordered = elements
        .iter()
        .filter(|e| e.interactive)
        .chain(elements.iter().filter(|e| !e.interactive));
    let mut lines: Vec<String> = ordered.take(max).map(ElementLine::render).collect();
    let rest = elements.len().saturating_sub(lines.len());
    if rest > 0 {
        lines.push(format!("… {rest} more"));
    }
    lines.join("\n")
}

/// A spoken answer without the screen (docs/prompts.md §1), streamed. `language_hint` is
/// the speech recognizer's language code; a value that does not look like one is left out.
pub fn chat_request(
    model: ModelChoice,
    effort: ReasoningEffort,
    language_hint: Option<&str>,
    question: &str,
) -> Value {
    json!({
        "model": model,
        "instructions": spoken_instructions(language_hint, false),
        "input": [user_message(question, None)],
        "stream": true,
        "max_output_tokens": SPOKEN_MAX_OUTPUT_TOKENS,
        "reasoning": {"effort": effort}
    })
}

/// "Where is X?" (mode A of docs/architecture.md §3): one streamed answer that points with
/// `<point eNN>` or `<point X,Y>`, no computer tool. `element_list` comes from
/// [`render_element_list`] for the frame of the image. Only a redacted screenshot may be
/// passed as `image_data_url` (convention 4).
pub fn point_request(
    model: ModelChoice,
    effort: ReasoningEffort,
    question: &str,
    element_list: &str,
    image_data_url: &str,
) -> Value {
    json!({
        "model": model,
        "instructions": spoken_instructions(None, true),
        "input": [user_message(&screen_text(question, element_list), Some(image_data_url))],
        "stream": true,
        "max_output_tokens": SCREEN_MAX_OUTPUT_TOKENS,
        "reasoning": {"effort": effort}
    })
}

/// The first teacher turn (docs/prompts.md §2): the computer tool, with the person as the
/// hand. The image keeps its size for the whole task: the model answers in its pixels.
/// Only a redacted screenshot may be passed as `image_data_url` (convention 4).
pub fn computer_request(
    model: ModelChoice,
    effort: ReasoningEffort,
    question: &str,
    element_list: &str,
    image_data_url: &str,
) -> Value {
    json!({
        "model": model,
        "instructions": teacher_instructions(),
        "input": [user_message(&screen_text(question, element_list), Some(image_data_url))],
        "tools": [computer_tool()],
        "max_output_tokens": SCREEN_MAX_OUTPUT_TOKENS,
        "reasoning": {"effort": effort}
    })
}

/// The next teacher turn: the screenshot taken after the person did the step, as the
/// `computer_call_output` for `call_id` of the response `previous_response_id`.
///
/// `acknowledged_safety_check_ids` holds only ids of `pending_safety_checks` that a HUMAN
/// confirmed (invariant 4): in the teacher, the person's own click on the pointed element
/// (D41); for a payment, purchase, delete, money transfer or password step, an explicit
/// yes. Code never copies the pending checks into this list: pass an empty slice when
/// nobody confirmed. Only a redacted screenshot may be passed as `image_data_url`
/// (convention 4).
pub fn computer_followup(
    session: ComputerSession,
    previous_response_id: &str,
    call_id: &str,
    acknowledged_safety_check_ids: &[String],
    image_data_url: &str,
) -> Value {
    let mut output = json!({
        "type": "computer_call_output",
        "call_id": call_id,
        "output": {"type": "computer_screenshot", "image_url": image_data_url}
    });
    if !acknowledged_safety_check_ids.is_empty() {
        let checks = acknowledged_safety_check_ids
            .iter()
            .map(|id| json!({"id": id}))
            .collect();
        if let Some(fields) = output.as_object_mut() {
            fields.insert("acknowledged_safety_checks".into(), Value::Array(checks));
        }
    }
    json!({
        "model": session.model,
        "instructions": teacher_instructions(),
        "previous_response_id": previous_response_id,
        "input": [output],
        "tools": [computer_tool()],
        "max_output_tokens": SCREEN_MAX_OUTPUT_TOKENS,
        "reasoning": {"effort": session.effort}
    })
}

/// Meta's computer tool takes no display size or environment: the S4 request that hit its
/// target sent exactly this (D41, `fixtures/replay/synthetic-text-size`).
fn computer_tool() -> Value {
    json!({"type": "computer"})
}

fn user_message(text: &str, image_data_url: Option<&str>) -> Value {
    let mut content = vec![json!({"type": "input_text", "text": text})];
    if let Some(url) = image_data_url {
        content.push(json!({"type": "input_image", "image_url": url}));
    }
    json!({"type": "message", "role": "user", "content": content})
}

/// The probe's user text: the question, then the element list (`tools/mm_probe.py`).
fn screen_text(question: &str, element_list: &str) -> String {
    let list = if element_list.trim().is_empty() {
        "(none)"
    } else {
        element_list
    };
    format!("{question}\n\n{ELEMENT_LIST_HEADER}\n{list}")
}

fn spoken_instructions(language_hint: Option<&str>, screen: bool) -> String {
    let hint = match language_hint.and_then(language_code) {
        Some(code) => format!("- Speech recognition heard \"{code}\": only a hint.\n"),
        None => String::new(),
    };
    let mut text = PERSONA_PROMPT
        .replace("{ROBOT_NAME}", ROBOT_NAME)
        .replace("{MODE}", mode_name(Mode::default()))
        .replace("{LANG_HINT}", &hint);
    if screen {
        text.push_str(SCREEN_PROMPT);
    } else {
        text.push_str(NO_SCREEN_PROMPT);
    }
    text.push_str(SAFETY_PROMPT);
    text.push_str(EXAMPLES_PROMPT);
    if screen {
        text.push_str(SCREEN_EXAMPLE_PROMPT);
    }
    text
}

fn teacher_instructions() -> String {
    TEACHER_PROMPT.replace("{ROBOT_NAME}", ROBOT_NAME)
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Calm => "calm",
        Mode::Fun => "fun",
    }
}

fn language_code(hint: &str) -> Option<&str> {
    let code = hint.trim();
    let allowed = |c: char| c.is_ascii_alphanumeric() || c == '-';
    let valid = (2..=16).contains(&code.len()) && code.chars().all(allowed);
    valid.then_some(code)
}

fn is_element_id(id: &str) -> bool {
    let Some(digits) = id.strip_prefix('e') else {
        return false;
    };
    (1..=9).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
}

fn clean_control_type(raw: &str) -> String {
    let mut kind: String = raw.chars().filter(char::is_ascii_alphanumeric).collect();
    kind.truncate(32);
    if kind.is_empty() {
        "Unknown".to_owned()
    } else {
        kind
    }
}

/// Control characters and whitespace runs become one space, invisible format characters
/// (bidi overrides, zero-width marks) go; a longer name ends in `…` at 60 characters.
fn clean_name(raw: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for ch in raw.chars() {
        if ch.is_control() || ch.is_whitespace() {
            space = !out.is_empty();
        } else if !is_invisible(ch) {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(ch);
        }
    }
    if out.chars().count() <= NAME_MAX_CHARS {
        return out;
    }
    let mut short: String = out.chars().take(NAME_MAX_CHARS - 1).collect();
    short.truncate(short.trim_end().len());
    short.push('…');
    short
}

fn is_invisible(ch: char) -> bool {
    matches!(
        ch,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// JSON-style quoting keeps a name on one line and its quotes unambiguous.
fn quoted(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    out.push('"');
    for ch in name.chars() {
        if matches!(ch, '"' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// docs/prompts.md §1: persona, language, style and the answer format. The sound and
/// ambience names mirror matescript/vocabulary.rs (the parser drops any other name); a
/// test checks them against the public parser.
const PERSONA_PROMPT: &str = "\
You are {ROBOT_NAME}, a small, kind light bulb who lives on the user's Windows desktop and points by shining his flashlight beam on things on the screen.
Many of your users are older people. Be warm, calm and patient. Never patronize, never blame.

LANGUAGE
- Answer in the language the user spoke.
{LANG_HINT}- Start every answer with <lang CODE>.
- The app's buttons may be in another language than the user's. Match by meaning, and say the button's label as it appears on screen.

STYLE
- Short sentences, plain words, one instruction per sentence. No jargon (\"cursor\", \"UI\", \"click target\").
- Spoken answers: max 3 sentences unless telling a story or the user asked for more.
- Never claim you clicked or changed anything. You only show and explain.

FORMAT
- Plain text is spoken. Tags in angle brackets: <lang ru> <mood happy> <sfx NAME [soft|normal|loud] [solo]> <amb NAME [soft|normal]> <amb off> <pause 500> <point ID> <point X,Y> <show TEXT> <end>.
- Moods: calm, happy, thinking, surprised, sad, proud, sleepy.
- Gentle sounds (any mode): ping, success, oops, think, listen, done, whoosh, pop, sleep, wake, birds, cat, owl, knock, clock, bell, applause, magic, wind, splash, boing, giggle, footsteps, tada.
- Fun mode only: rooster, dog, horse, rimshot, drumroll, cheer, fanfare, thunder, boom, laser, slip, door, phone, train, car, heart, sad.
- Ambience: forest, rain, sea, fire, night, park, cafe. No other sound or ambience names.
- Sound mode: {MODE}. In calm mode use at most 2 gentle sounds per answer, never loud ones. In fun mode up to 4.
- <sfx> (without solo), <mood>, <point> and <show> start with the NEXT sentence: write them right before the sentence they go with, never after it.
- Never put a tag inside a word. Never use \"<\" for anything else.

";

/// docs/prompts.md §1, "THE SCREEN"; the element list itself travels in the user message.
const SCREEN_PROMPT: &str = "\
THE SCREEN
- You see a screenshot of the user's active window and, in the message, a list of its elements (format: e17 Button \"Text size\" [x,y,w,h], in pixels of the screenshot).
- To show something, use <point ID> with an id from the list. Use <point X,Y> (pixels of the screenshot) only if the thing is visible but not in the list.
- If you cannot find it, say so honestly and suggest what to open. Never guess.
- Black boxes and [hidden] elements are password fields. Never ask the user to read a password aloud.

";

const NO_SCREEN_PROMPT: &str = "\
THE SCREEN
- You do not see the screen in this conversation. Never point at it or describe it.

";

const SAFETY_PROMPT: &str = "\
SAFETY
- Do not help with payments, transfers or deleting things without telling the user to check carefully first.
- If a page looks like a scam (urgent money requests, \"your computer is infected\" pop-ups), say so gently and suggest calling a family member.

";

/// Examples 1 and 4 of docs/matescript.md.
const EXAMPLES_PROMPT: &str = "\
EXAMPLES
A story (fun mode):
<lang en><mood calm><amb forest soft>Once upon a time, a little fox lived at the edge of a quiet forest.
<sfx birds soft>Every morning the birds woke him up.
One day the sky turned dark. <sfx thunder solo><mood surprised>\"Oh no,\" said the fox, \"a storm!\"
He ran home just in time, <amb rain soft>and listened to the rain from his warm bed. <mood sleepy>

A joke (fun mode):
<lang en><mood happy>Why did the computer go to the doctor?
Because it had a virus! <sfx rimshot solo><mood proud>
";

/// Example 2 of docs/matescript.md, only when there is a screen to point at.
const SCREEN_EXAMPLE_PROMPT: &str = "
\"Where is the Save button?\" (Word, element list given):
<lang en><mood happy><point e12><sfx ping soft>It's right here, the little disk at the top left.
You can also press Control and S together.
";

/// docs/prompts.md §2. The task travels in the first user message, so this text stays the
/// same and is sent again with every follow-up turn.
const TEACHER_PROMPT: &str = "\
You are {ROBOT_NAME}, a small, kind light bulb on the screen, guiding an older person through a task on their own Windows computer.
You never act yourself: the PERSON performs every action. You decide the next single action and it will be shown to them.
The task, a list of the window's elements and a screenshot are in the person's first message.

Rules:
- Return exactly one action per computer_call: the smallest useful step.
- Prefer visible buttons and menus over keyboard shortcuts.
- Never perform or suggest purchases, payments, money transfers, deleting files or changing passwords unless the person explicitly asked, and then stop before the final confirmation and say what will happen.
- If the task is impossible from this screen, or what you need is not visible, stop and explain in one sentence. Never guess.
- After each action you will get a new screenshot. Check that the screen changed as expected before the next step.
- When the task is done, stop and say so.
- With each step, write one short spoken sentence for the person in the language of the task. Plain words; you may start it with <lang CODE> and <mood NAME> (calm, happy, thinking, surprised, sad, proud, sleepy). No other tags.
- Never claim you clicked or changed anything. You only show and explain.
- Black boxes and [hidden] elements are password fields. Never ask the person to read a password aloud.";

#[cfg(test)]
mod tests {
    use super::*;

    use crate::matescript::{Item, Parser, Settings};

    const DATA_URL: &str = "data:image/png;base64,AAAA";
    const LARGE_LINE: &str = "e11 Button \"Large\" [960,255,200,32]";
    const QUICK: ModelChoice = ModelChoice::Quick;
    const SPOKEN: ReasoningEffort = ReasoningEffort::SPOKEN;
    const ACT: ReasoningEffort = ReasoningEffort::COMPUTER_USE;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PhysRect {
        PhysRect::new(left, top, right, bottom).unwrap()
    }

    fn frame() -> FrameTransform {
        // 2560x1600 physical pixels at (100, 50), sent at half size.
        FrameTransform::new(rect(100, 50, 2660, 1650), 1280, 800).unwrap()
    }

    fn large() -> PhysRect {
        rect(2020, 560, 2420, 624)
    }

    fn element(id: &str, name: &str, interactive: bool, hidden: bool) -> ElementLine {
        let line = ElementLine::new(id, "Button", name, large(), frame(), interactive, hidden);
        line.unwrap()
    }

    fn large_button() -> ElementLine {
        element("e11", "Large", true, false)
    }

    fn instructions(body: &Value) -> &str {
        body["instructions"].as_str().unwrap()
    }

    fn session() -> ComputerSession {
        ComputerSession {
            model: QUICK,
            effort: ACT,
            sent_width: 1280,
            sent_height: 800,
        }
    }

    #[test]
    fn model_ids_and_efforts_serialize_as_the_api_expects() {
        let id = |model: ModelChoice| serde_json::to_value(model).unwrap();
        assert_eq!(id(ModelChoice::Quick), "muse-spark-1.3");
        assert_eq!(id(ModelChoice::Thorough), "muse-wonder-1.0");
        assert_eq!(ModelChoice::default(), ModelChoice::Quick);
        let lower = |effort: ReasoningEffort| serde_json::to_value(effort).unwrap();
        assert_eq!(lower(ReasoningEffort::Minimal), "minimal");
        assert_eq!(lower(ReasoningEffort::Low), "low");
        assert_eq!(lower(ReasoningEffort::Medium), "medium");
        assert_eq!(lower(ReasoningEffort::High), "high");
        assert_eq!(ReasoningEffort::SPOKEN, ReasoningEffort::Minimal);
        assert_eq!(ReasoningEffort::COMPUTER_USE, ReasoningEffort::Minimal);
        assert_eq!(ReasoningEffort::RETRY, ReasoningEffort::Low);
    }

    #[test]
    fn chat_request_streams_a_spoken_answer() {
        let body = chat_request(QUICK, SPOKEN, Some("ru"), "Tell me a joke");
        assert_eq!(body["model"], "muse-spark-1.3");
        assert_eq!(body["reasoning"]["effort"], "minimal");
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["max_output_tokens"], json!(400));
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"][0]["text"], "Tell me a joke");
        assert!(body.get("tools").is_none());
        let text = instructions(&body);
        assert!(text.contains("heard \"ru\""));
        assert!(text.contains("Start every answer with <lang CODE>"));
        assert!(text.contains("Sound mode: calm"));
        assert!(!text.contains("<point e12>"));
    }

    #[test]
    fn a_language_hint_that_is_not_a_code_stays_out_of_the_prompt() {
        for hint in [Some("ru zzz"), Some("ru}\nzzz"), Some(""), None] {
            let body = chat_request(QUICK, SPOKEN, hint, "Hi");
            assert!(!instructions(&body).contains("zzz"));
            assert!(!instructions(&body).contains("heard"));
        }
    }

    #[test]
    fn point_request_sends_the_list_and_the_image() {
        let list = render_element_list(&[large_button()], 150);
        let body = point_request(ModelChoice::Thorough, SPOKEN, "Where?", &list, DATA_URL);
        assert_eq!(body["model"], "muse-wonder-1.0");
        assert_eq!(body["reasoning"]["effort"], "minimal");
        assert_eq!(body["stream"], json!(true));
        assert!(body.get("tools").is_none());
        let content = &body["input"][0]["content"];
        let expected = format!("Where?\n\n{ELEMENT_LIST_HEADER}\n{LARGE_LINE}");
        assert_eq!(content[0]["text"], expected);
        assert_eq!(content[1]["type"], "input_image");
        assert_eq!(content[1]["image_url"], DATA_URL);
        assert!(instructions(&body).contains("with an id from the list"));
        assert!(instructions(&body).contains("<point e12>"));
    }

    #[test]
    fn computer_request_carries_the_tool_and_the_teacher_rules() {
        let list = render_element_list(&[large_button()], 150);
        let body = computer_request(QUICK, ACT, "Bigger text", &list, DATA_URL);
        assert_eq!(body["tools"], json!([{"type": "computer"}]));
        assert_eq!(body["model"], "muse-spark-1.3");
        assert_eq!(body["reasoning"]["effort"], "minimal");
        assert!(body.get("stream").is_none());
        assert_eq!(body["input"][0]["content"][1]["image_url"], DATA_URL);
        let text = instructions(&body);
        assert!(text.contains("You never act yourself"));
        assert!(text.contains("exactly one action per computer_call"));
    }

    #[test]
    fn followup_acknowledges_only_the_checks_it_is_given() {
        let confirmed = ["cu_sc_1".to_owned()];
        let body = computer_followup(session(), "resp_1", "call_1", &confirmed, DATA_URL);
        assert_eq!(body["model"], "muse-spark-1.3");
        assert_eq!(body["previous_response_id"], "resp_1");
        assert_eq!(body["tools"], json!([{"type": "computer"}]));
        assert_eq!(body["reasoning"]["effort"], "minimal");
        assert_eq!(body["instructions"], teacher_instructions());
        let output = &body["input"][0];
        assert_eq!(output["type"], "computer_call_output");
        assert_eq!(output["call_id"], "call_1");
        let checks = json!([{"id": "cu_sc_1"}]);
        assert_eq!(output["acknowledged_safety_checks"], checks);
        let screenshot = json!({"type": "computer_screenshot", "image_url": DATA_URL});
        assert_eq!(output["output"], screenshot);

        let body = computer_followup(session(), "resp_1", "call_1", &[], DATA_URL);
        let output = &body["input"][0];
        assert!(output.get("acknowledged_safety_checks").is_none());
    }

    #[test]
    fn element_rect_is_mapped_and_clipped_to_the_screenshot() {
        assert_eq!(large_button().render(), LARGE_LINE);
        let corner = rect(0, 0, 300, 100);
        let line = ElementLine::new("e0", "Window", "Edge", corner, frame(), false, false);
        let line = line.unwrap().render();
        assert_eq!(line, "e0 Window \"Edge\" [0,0,100,25]");
        let outside = rect(3000, 0, 3100, 100);
        let line = ElementLine::new("e1", "Button", "Off", outside, frame(), true, false);
        assert!(line.is_none());
        for id in ["", "e", "E1", "x1", "e1 2", "e-1", "e1234567890"] {
            let line = ElementLine::new(id, "Button", "Ok", large(), frame(), true, false);
            assert!(line.is_none(), "{id:?}");
        }
    }

    #[test]
    fn hidden_elements_never_show_their_name() {
        let secret = ElementLine::new("e5", "Edit", "hunter2", large(), frame(), true, true);
        let secret = secret.unwrap();
        assert_eq!(secret.render(), "e5 Edit [hidden] [960,255,200,32]");
        assert!(!format!("{secret:?}").contains("hunter2"));
        let list = render_element_list(&[secret, large_button()], 150);
        assert!(list.contains("e5 Edit [hidden] "));
        assert!(!list.contains("hunter2"));
    }

    #[test]
    fn names_are_cleaned_and_trimmed_to_60_chars() {
        let long = element("e1", &"A".repeat(100), true, false).render();
        let expected = format!("e1 Button \"{}…\" [960,255,200,32]", "A".repeat(59));
        assert_eq!(long, expected);
        let exact = "B".repeat(60);
        let line = element("e1", &exact, true, false).render();
        assert!(line.contains(exact.as_str()));

        let raw = " Save\nas\u{7}\tit\u{202E}s \"q\" \\\u{85}";
        let line = element("e1", raw, true, false).render();
        let expected = r#"e1 Button "Save as its \"q\" \\" [960,255,200,32]"#;
        assert_eq!(line, expected);
        assert!(!line.chars().any(char::is_control));
        let kind = ElementLine::new("e1", "But\nton", "Ok", large(), frame(), true, false);
        assert!(kind.unwrap().render().starts_with("e1 Button "));
    }

    #[test]
    fn list_puts_interactive_first_and_caps_the_lines() {
        let mut elements = vec![
            element("e1", "Title", false, false),
            element("e2", "Body", false, false),
        ];
        for n in 3..8 {
            elements.push(element(&format!("e{n}"), "Ok", true, false));
        }

        let list = render_element_list(&elements, 3);
        let lines: Vec<&str> = list.lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("e3 "));
        assert!(lines[1].starts_with("e4 "));
        assert!(lines[2].starts_with("e5 "));
        assert_eq!(lines[3], "… 4 more");

        let all = render_element_list(&elements, 150);
        let ids: Vec<&str> = all.lines().map(|l| &l[..2]).collect();
        assert_eq!(ids, ["e3", "e4", "e5", "e6", "e7", "e1", "e2"]);
        assert!(!all.contains("more"));
        assert!(!all.chars().any(|c| c.is_control() && c != '\n'));

        assert_eq!(render_element_list(&elements, 0), "… 7 more");
        assert_eq!(render_element_list(&[], 10), "");
    }

    #[test]
    fn prompts_take_the_robot_name_from_the_constant() {
        let templates = [
            PERSONA_PROMPT,
            SCREEN_PROMPT,
            NO_SCREEN_PROMPT,
            SAFETY_PROMPT,
            EXAMPLES_PROMPT,
            SCREEN_EXAMPLE_PROMPT,
            TEACHER_PROMPT,
        ];
        for template in templates {
            assert!(!template.contains("Mate"));
        }
        let list = render_element_list(&[large_button()], 150);
        let bodies = [
            chat_request(QUICK, SPOKEN, Some("en"), "Hi"),
            point_request(QUICK, SPOKEN, "Hi", &list, DATA_URL),
            computer_request(QUICK, ACT, "Hi", &list, DATA_URL),
            computer_followup(session(), "resp_1", "call_1", &[], DATA_URL),
        ];
        let named = format!("You are {ROBOT_NAME}, ");
        for body in &bodies {
            let text = instructions(body);
            assert!(text.contains(named.as_str()));
            assert!(!text.contains(['{', '}']));
        }
    }

    fn names_after(prefix: &str) -> Vec<&'static str> {
        let line = PERSONA_PROMPT
            .lines()
            .find_map(|l| l.strip_prefix(prefix))
            .unwrap();
        let list = line.split('.').next().unwrap();
        list.split(", ").collect()
    }

    fn timeline(mode: Mode, answer: &str) -> Vec<Item> {
        let mut parser = Parser::new(Settings {
            mode,
            ..Settings::default()
        });
        let mut items = parser.push(answer);
        items.extend(parser.finish());
        items
    }

    fn plays(items: &[Item], name: &str) -> bool {
        let named = |item: &Item| matches!(item, Item::Sfx(sound) if sound.name == name);
        items.iter().any(named)
    }

    #[test]
    fn prompt_sound_names_match_the_parser_vocabulary() {
        let gentle = names_after("- Gentle sounds (any mode): ");
        let fun = names_after("- Fun mode only: ");
        // 41 = SOUNDS in matescript/vocabulary.rs: a new sound needs a prompt line too.
        assert_eq!(gentle.len() + fun.len(), 41);
        for name in gentle {
            let tag = format!("<sfx {name} solo>");
            assert!(plays(&timeline(Mode::Calm, &tag), name), "{name}");
        }
        for name in fun {
            let tag = format!("<sfx {name} solo>");
            assert!(plays(&timeline(Mode::Fun, &tag), name), "{name}");
            assert!(!plays(&timeline(Mode::Calm, &tag), name), "{name}");
        }
        for name in names_after("- Ambience: ") {
            let items = timeline(Mode::Calm, &format!("<amb {name}>"));
            let starts = |i: &Item| matches!(i, Item::Amb { name: Some(n), .. } if n == name);
            assert!(items.iter().any(starts), "{name}");
        }
    }
}
