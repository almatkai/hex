//! Antigravity account and model controls. No I/O occurs during rendering.
use super::controls::{self, CardRow, Choice, Paint};
use super::{Menu, Ui};
use crate::desktop::i18n::tr;
use rune::El;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Snapshot {
    email: Option<String>,
    models: Vec<String>,
    failed: bool,
}

#[derive(Default)]
pub(super) struct Account {
    snapshot: Snapshot,
    receiver: Option<Receiver<Snapshot>>,
    cancel: Arc<AtomicBool>,
    signing: bool,
    read_at: Option<Instant>,
}

impl Drop for Account {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

impl Account {
    pub(super) fn fixture(signed_in: bool) -> Self {
        Self {
            snapshot: Snapshot {
                email: signed_in.then(|| "alex@example.com".into()),
                models: if signed_in {
                    vec!["gemini-3.8-flash-low".into(), "claude-sonnet-4-6".into()]
                } else {
                    Vec::new()
                },
                failed: false,
            },
            receiver: None,
            cancel: Arc::new(AtomicBool::new(false)),
            signing: false,
            read_at: None,
        }
    }
    pub(super) fn signed_in(&self) -> bool {
        self.snapshot.email.is_some()
    }

    fn start(&mut self, account_action: bool) {
        if self.receiver.is_some() {
            if !account_action || self.signing {
                return;
            }
            // A background refresh only reads the account and catalog; drop
            // it so a Sign in or Sign out click is never swallowed.
            self.cancel.store(true, Ordering::Release);
            self.receiver = None;
        }
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let signed_in = self.signed_in();
        let old_email = self.snapshot.email.clone();
        let old_models = self.snapshot.models.clone();
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.signing = account_action;
        self.read_at = Some(Instant::now());
        std::thread::spawn(move || {
            let failed = account_action
                && if signed_in {
                    crate::antigravity::logout().is_err()
                } else {
                    crate::antigravity::login(&cancel, None).is_err()
                };
            let email = crate::antigravity::email();
            let models = if email.is_some() && (email != old_email || old_models.is_empty()) {
                crate::antigravity::models(&cancel).unwrap_or_default()
            } else if email.is_some() {
                old_models
            } else {
                Vec::new()
            };
            let _ = tx.send(Snapshot {
                email,
                models,
                failed,
            });
        });
    }

    pub(super) fn poll(&mut self, wanted: bool) -> bool {
        if !wanted && self.signing {
            self.cancel.store(true, Ordering::Release);
        }
        if let Some(rx) = &self.receiver {
            match rx.try_recv() {
                Ok(snapshot) => {
                    self.snapshot = snapshot;
                    self.receiver = None;
                    self.signing = false;
                    return true;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.receiver = None;
                    self.signing = false;
                    self.snapshot.failed = true;
                    return true;
                }
                Err(mpsc::TryRecvError::Empty) => return false,
            }
        }
        if wanted
            && self
                .read_at
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(15))
        {
            self.start(false);
        }
        false
    }
}

pub(super) fn rows(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<CardRow> {
    let account = &ui.antigravity;
    let busy = account.signing;
    let label = if busy {
        tr("Cancel")
    } else if account.signed_in() {
        tr("Sign out")
    } else {
        tr("Sign in with Google")
    };
    let control = controls::button(
        p,
        "antigravity-auth",
        label,
        controls::ButtonKind::Secondary,
        move |ui: &mut Ui, _| {
            if ui.is_fixture() {
                return;
            }
            if busy {
                ui.antigravity.cancel.store(true, Ordering::Release);
            } else {
                ui.antigravity.start(true);
            }
        },
    );
    let description = if account.snapshot.failed {
        tr("Antigravity sign-in failed. Try again")
    } else {
        account
            .snapshot
            .email
            .as_deref()
            .unwrap_or(tr("Sign in to Antigravity"))
    };
    let mut rows = vec![
        controls::setting_row(
            p,
            "antigravity-account",
            tr("Antigravity"),
            Some(description),
            control,
        )
        .into(),
    ];
    let models = if account.snapshot.models.is_empty() {
        crate::antigravity::MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        account.snapshot.models.clone()
    };
    let choices = models
        .into_iter()
        .map(|m| Choice {
            selected: m == ui.settings.meetings.antigravity_model,
            label: m.clone(),
            detail: None,
            choose: Box::new(move |ui| {
                ui.update(|s| s.meetings.antigravity_model = m.clone());
            }),
        })
        .collect();
    let menu = Menu::Pick("antigravity-model");
    let picker = controls::select(
        p,
        "antigravity-model",
        tr("Model"),
        &ui.settings.meetings.antigravity_model,
        ui.open == Some(menu),
        choices,
        move |ui| {
            ui.open = if ui.open == Some(menu) {
                None
            } else {
                Some(menu)
            };
        },
        layer,
    );
    rows.push(controls::setting_row(p, "antigravity-model-row", tr("Model"), None, picker).into());
    rows
}
