//! Ephemeral questions and cancellable answer worker for the selected meeting.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};

#[derive(Default)]
pub(in crate::rune_ui) struct Questions {
    question: TextEdit,
    answer: Option<String>,
    error: Option<String>,
    pending: Option<Receiver<Result<crate::meeting::ask::Answer, String>>>,
    cancel: Arc<AtomicBool>,
    meeting_id: Option<String>,
}

impl Drop for Questions {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

impl Questions {
    pub(in crate::rune_ui) fn fixture() -> Self {
        Self {
            question: TextEdit::new("What did we agree to do next?"),
            answer: Some("Write a proposal for the build screen; it will be reviewed tomorrow [01:35–01:41].".into()),
            error: None,
            pending: None,
            cancel: Arc::default(),
            meeting_id: None,
        }
    }
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.pending = None;
    }

    pub(super) fn poll(&mut self, wanted: bool, selected: Option<&str>) -> bool {
        if !wanted || (self.meeting_id.is_some() && self.meeting_id.as_deref() != selected) {
            let changed = self.pending.is_some() || self.answer.is_some() || self.error.is_some();
            self.clear();
            return changed;
        }
        let Some(rx) = &self.pending else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err(tr("Antigravity request failed").into()),
        };
        self.pending = None;
        match result {
            Ok(answer) => self.answer = Some(answer.answer),
            Err(error) => self.error = Some(error),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meeting_ask_cancels_on_selection_change_and_discards_late_answers() {
        let mut state = Questions::default();
        state.meeting_id = Some("one".into());
        let (tx, rx) = mpsc::channel();
        state.pending = Some(rx);
        let cancel = state.cancel.clone();
        assert!(state.poll(true, Some("two")));
        assert!(cancel.load(Ordering::Acquire));
        assert!(tx.send(Err("late answer".into())).is_err());
        assert!(state.answer.is_none());
    }

    #[test]
    fn meeting_ask_delivers_answers_and_cancels_when_the_feature_is_disabled() {
        let mut state = Questions::default();
        state.meeting_id = Some("one".into());
        let (tx, rx) = mpsc::channel();
        state.pending = Some(rx);
        tx.send(Ok(crate::meeting::ask::Answer {
            version: 1,
            meeting_id: "one".into(),
            answer: "Grounded [00:12]".into(),
            model: "test".into(),
            excerpted: false,
        }))
        .unwrap();
        assert!(state.poll(true, Some("one")));
        assert_eq!(state.answer.as_deref(), Some("Grounded [00:12]"));
        assert!(state.poll(false, Some("one")));
        assert!(state.answer.is_none());
    }
}

pub(in crate::rune_ui) fn poll(ui: &mut Ui) -> bool {
    if ui.is_fixture() {
        return false;
    }
    let wanted = enabled(ui)
        && ui.pane == super::super::Pane::Meetings
        && !ui.labs_open
        && ui.meetings.view == View::Recordings
        && ui.window.as_ref().is_some_and(rune::AppHandle::is_visible);
    ui.meetings
        .questions
        .poll(wanted, ui.meetings.selected.as_deref())
}

fn enabled(ui: &Ui) -> bool {
    crate::meeting::ask::enabled(&ui.settings)
}

fn start(ui: &mut Ui) {
    if ui.is_fixture() || !enabled(ui) || ui.meetings.questions.pending.is_some() {
        return;
    }
    let Some(id) = ui.meetings.selected.clone() else {
        return;
    };
    let settings = ui.settings.clone();
    let state = &mut ui.meetings.questions;
    let question = state.question.text.clone();
    state.cancel = Arc::new(AtomicBool::new(false));
    let cancel = state.cancel.clone();
    let (tx, rx) = mpsc::channel();
    state.pending = Some(rx);
    state.answer = None;
    state.error = None;
    state.meeting_id = Some(id.clone());
    std::thread::spawn(move || {
        crate::transcription::background_priority::deprioritize_background_thread();
        let result = crate::meeting::ask::answer(
            &settings,
            || {
                let root = crate::app_paths::support_dir()?;
                crate::meeting::query::Library::new(root.join("meetings")).meeting(&id)
            },
            &question,
            &cancel,
        )
        .map_err(|error| error.to_string());
        let _ = tx.send(result);
    });
}

pub(super) fn panel(ui: &Ui, p: Paint) -> Option<El> {
    if !enabled(ui) {
        return None;
    }
    let state = &ui.meetings.questions;
    let busy = state.pending.is_some();
    let mut panel = col().key("meeting-ask").gap(8.0)
        .child(section_label(p, "meeting-ask-label", tr("Ask meetings")))
        .child(text(tr("Ask questions about a meeting with Antigravity; sends transcript excerpts to Google"))
            .font(DESCRIPTION).color(p.faint()).wrap());
    if !ui.antigravity.signed_in() {
        return Some(
            panel.child(
                text(tr("Sign in to Antigravity"))
                    .font(DESCRIPTION)
                    .color(p.faint()),
            ),
        );
    }
    panel = panel.child(
        row()
            .gap(8.0)
            .center_y()
            .child(
                TextInput::new(&state.question)
                    .label(tr("Question"))
                    .disabled(busy)
                    .on_edit(|ui: &mut Ui| &mut ui.meetings.questions.question)
                    .on_submit(|ui: &mut Ui, _| start(ui))
                    .build(p.k)
                    .key("meeting-question")
                    .grow(1.0),
            )
            .child(button(
                p,
                "meeting-ask-button",
                if busy { tr("Cancel") } else { tr("Ask") },
                ButtonKind::Secondary,
                move |ui: &mut Ui, _| {
                    if busy {
                        ui.meetings.questions.cancel();
                    } else {
                        start(ui);
                    }
                },
            )),
    );
    if busy {
        panel = panel.child(
            text(tr("Asking Antigravity…"))
                .font(DESCRIPTION)
                .color(p.faint()),
        );
    }
    if let Some(error) = &state.error {
        panel = panel.child(error_panel(
            p,
            "meeting-ask-error",
            tr("Antigravity request failed"),
            error,
            Tone::Danger,
        ));
    }
    if let Some(answer) = &state.answer {
        panel = panel.child(
            text(answer)
                .key("meeting-answer")
                .font(p.k.ty.body)
                .color(p.text())
                .wrap(),
        );
    }
    Some(panel)
}
