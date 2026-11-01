//! Playback-triggered website/app opening. Only the Rust-owned answer target is used.

use mm_core::matescript::OpenTarget;
use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};

use crate::answer_stream::AnswerStream;

/// Resolve the person's installed-app request before calling any answer model.
pub(crate) async fn offer_installed(
    app: &AppHandle,
    proxy: &crate::proxy::Proxy,
    generation: u64,
    question: &str,
    locale: &str,
) -> bool {
    use crate::answer_stream::AnswerMsg;
    use mm_core::installed_apps::{self, Match};
    use mm_core::matescript::Item;
    let Some(target) = installed_apps::requested_app(question) else {
        return false;
    };
    let stream = app.state::<AnswerStream>();
    if let Some(name) = installed_apps::remote_access(&target) {
        let line = crate::locale_text(locale, "open.remote_warning").replace("{name}", name);
        fixed_line(app, proxy, generation, locale, 1, line, true).await;
        stream.send(AnswerMsg::Item {
            generation,
            item: Item::End,
        });
        return true;
    }
    let shortcuts = match tokio::task::spawn_blocking(mm_win32::start_menu::installed).await {
        Ok(shortcuts) => shortcuts,
        Err(_) => return false,
    };
    let shortcut =
        match installed_apps::find(&target, shortcuts.iter().map(|app| app.name.as_str())) {
            Match::One(index) => shortcuts[index].clone(),
            Match::Ambiguous => {
                let line = crate::locale_text(locale, "open.exact_name");
                fixed_line(app, proxy, generation, locale, 1, line, false).await;
                stream.send(AnswerMsg::Item {
                    generation,
                    item: Item::End,
                });
                return true;
            }
            Match::None => return false,
        };
    let line =
        crate::locale_text(locale, "open.confirm_installed").replace("{name}", &shortcut.name);
    // Prepare speech first; the consent timer starts only when the question is shown.
    let speech = proxy.speak(locale, &line).await.ok();
    let consent_app = app.clone();
    let yes = tokio::task::spawn_blocking(move || {
        let stream = consent_app.state::<AnswerStream>();
        let wanted = || !stream.is_cancelled(generation);
        crate::screen::confirm_question(None, &wanted, || {
            send_line(&stream, generation, 1, line, false);
            if let Some(speech) = speech {
                stream.send_wav(generation, 1, &speech.wav);
            } else {
                stream.end_sentence(generation, 1);
            }
        })
    })
    .await
    .unwrap_or(false);
    if stream.is_cancelled(generation) {
        return true;
    }
    let result = if yes {
        if mm_win32::start_menu::open(&shortcut, &|| !stream.is_cancelled(generation)) {
            None
        } else {
            Some(crate::locale_text(locale, "open.failed").replace("{name}", &shortcut.name))
        }
    } else {
        Some(crate::locale_text(locale, "open.cancelled"))
    };
    if let Some(line) = result {
        fixed_line(app, proxy, generation, locale, 2, line, false).await;
    }
    stream.send(AnswerMsg::Item {
        generation,
        item: Item::End,
    });
    true
}

fn send_line(stream: &AnswerStream, generation: u64, id: u64, text: String, warning: bool) {
    use crate::answer_stream::AnswerMsg;
    use mm_core::matescript::{Bound, Item, Say};
    stream.send(AnswerMsg::Item {
        generation,
        item: Item::Say(Say {
            id,
            bound: if warning {
                vec![Bound::Mood("sad".into()), Bound::Show(text.clone())]
            } else {
                vec![Bound::Show(text.clone())]
            },
            text,
        }),
    });
}

async fn fixed_line(
    app: &AppHandle,
    proxy: &crate::proxy::Proxy,
    generation: u64,
    locale: &str,
    id: u64,
    line: String,
    warning: bool,
) {
    let stream = app.state::<AnswerStream>();
    if stream.is_cancelled(generation) {
        return;
    }
    send_line(&stream, generation, id, line.clone(), warning);
    if let Ok(speech) = proxy.speak(locale, &line).await {
        stream.send_wav(generation, id, &speech.wav);
    } else {
        stream.end_sentence(generation, id);
    }
}

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
