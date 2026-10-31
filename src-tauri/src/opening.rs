//! Playback-triggered website/app opening. Only the Rust-owned answer target is used.

use mm_core::matescript::OpenTarget;
use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};

use crate::answer_stream::AnswerStream;

#[derive(Serialize)]
pub struct OpenResult {
    name: String,
    opened: bool,
}

#[tauri::command]
pub async fn open_answer_target(
    app: AppHandle,
    window: WebviewWindow,
    generation: u64,
) -> Option<OpenResult> {
    if window.label() != "mate" {
        return None;
    }
    let stream = app.state::<AnswerStream>();
    let target = stream.take_open(generation)?;
    // Cancelled between the tag and now (the person spoke again): nothing opens.
    if stream.is_cancelled(generation) {
        return None;
    }
    let locale = stream.locale();
    let (name, opened) = match target {
        OpenTarget::Site(ref url) => {
            // Revalidate at the shell boundary as well as at parsing.
            let checked = OpenTarget::parse(url)?;
            (checked.host()?.to_owned(), mm_win32::shell::open_url(url))
        }
        OpenTarget::App(id) => (
            crate::locale_text(&locale, id.spec().1),
            mm_win32::shell::open_app(id),
        ),
    };
    if !opened {
        let line = crate::locale_text(&locale, "open.failed").replace("{name}", &name);
        let proxy = app
            .try_state::<crate::proxy::Proxy>()
            .map(|proxy| proxy.inner().clone());
        if let Some(proxy) = proxy
            && let Ok(speech) = proxy.speak(&locale, &line).await
            && !stream.is_cancelled(generation)
        {
            stream.send_wav(generation, 0, &speech.wav);
        }
    }
    Some(OpenResult { name, opened })
}
