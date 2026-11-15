//! The first run's state machine in the window: [`Tour`] feeds
//! `onboarding::Flow` the permissions and the model download, starts the
//! download, asks for permissions, relaunches when macOS needs it, watches
//! the practice dictation, and records completion. The first run the person
//! sees is the onboarding story over it ([`onboarding_kit::live`]), the one
//! onboarding on every OS.
//!
//! [`onboarding_kit::live`]: super::onboarding_kit::live

use std::time::{Duration, Instant};

use super::Request;
use super::Ui;
use super::platform::{PlatformRequest, PlatformState};
use crate::app_settings::AppSettings;
use crate::desktop::onboarding::{
    Effect, Flow, Input, ModelProgress, PermissionState, SetupItem, Step,
};

/// How long the ready check shows before setup moves on.
const ADVANCE_BEAT: Duration = Duration::from_millis(700);

pub(super) struct Tour {
    pub(super) flow: Flow,
    advance_at: Option<Instant>,
    completed: bool,
}

impl Tour {
    fn new(mut flow: Flow, completed: bool) -> Self {
        flow.hold_download(true);
        Self {
            flow,
            advance_at: None,
            completed,
        }
    }

    /// The tour from Welcome, as on a new install: Run setup again.
    pub(super) fn replay(settings: &AppSettings, platform: &PlatformState) -> Tour {
        let installed = super::picker::Picker::default().selected_installed(settings);
        Tour::new(
            Flow::new(
                Step::Welcome,
                platform.setup_status(installed),
                platform.caps.permissions,
                platform.caps.practice_dictation,
            ),
            false,
        )
    }

    /// The tour when this run still needs it.
    pub(super) fn start(settings: &AppSettings, platform: &PlatformState) -> Option<Tour> {
        let installed = super::picker::Picker::default().selected_installed(settings);
        let status = platform.setup_status(installed);
        let completed = crate::desktop::onboarding::completion_recorded();
        let resume = crate::desktop::onboarding::take_resume();
        let saved = (!completed)
            .then(crate::desktop::onboarding::saved_step)
            .flatten();
        (resume.is_some() || (!completed && (!status.ready() || saved.is_some()))).then(|| {
            Tour::new(
                Flow::new(
                    resume.or(saved).unwrap_or(Step::Welcome),
                    status,
                    platform.caps.permissions,
                    platform.caps.practice_dictation,
                ),
                completed,
            )
        })
    }
}

pub(super) fn progress(ui: &Ui) -> (bool, ModelProgress) {
    let selection = ui
        .found
        .choice
        .as_ref()
        .unwrap_or(&ui.settings.transcription);
    let selected = crate::transcription::models::validate(selection).ok();
    let installed = selected.is_some_and(|model| ui.picker.installed(model, &selection.language));
    let transcription = ui.view.as_ref().map(|view| &view.transcription);
    let downloading = selected.and_then(|model| {
        (transcription.and_then(|t| t.preparing) == Some(model.id)).then(|| {
            model.download_bytes().map_or(0.0, |bytes| {
                (transcription.map_or(0, |t| t.downloaded_bytes) as f32 / bytes as f32)
                    .clamp(0.0, 1.0)
            })
        })
    });
    (
        installed,
        ModelProgress {
            installed,
            downloading,
            failed: downloading.is_none() && transcription.is_some_and(|t| t.error.is_some()),
        },
    )
}

/// Feeds the flow fresh permission and model state; true when the tour
/// changed.
pub(super) fn poll(ui: &mut Ui) -> bool {
    // Only a pick or Continue may start a download, even with an empty scan
    // or a tour resumed past setup.
    if ui.tour.is_some() {
        super::found::scan_for_setup(ui);
        if ui.found.recommended.is_none() {
            ui.found.recommended = Some(ui.settings.transcription.clone());
        }
        if let Some(tour) = &mut ui.tour {
            // Explicit Grant(Model) starts the pick. Polling never creates
            // another request while the worker is accepting it.
            tour.flow.hold_download(true);
        }
    }
    let (installed, model) = progress(ui);
    let status = ui.platform.setup_status(installed);
    let accessibility_stale = ui
        .platform
        .stale(crate::desktop::onboarding::PermissionKind::Accessibility);
    let live = ui.live();
    let Some(tour) = &mut ui.tour else {
        return false;
    };
    let before = tour.flow.view();
    let step = before.step;
    let mut effect = tour.flow.on(Input::Status(status, model));
    tour.flow.set_accessibility_stale(accessibility_stale);
    if tour
        .advance_at
        .is_some_and(|at| at.elapsed() >= ADVANCE_BEAT)
    {
        tour.advance_at = None;
        if tour.flow.step() == Step::Setup {
            effect = tour.flow.on(Input::Continue);
        }
    }
    if status.ready() && !tour.completed {
        tour.completed = true;
        if live && let Err(error) = crate::desktop::onboarding::record_completion() {
            tracing::warn!(%error, "could not record onboarding completion");
        }
    }
    let practicing = tour.flow.step() == Step::TryIt;
    apply(ui, effect);
    let heard = ui.host.tour_inputs(practicing);
    let changed = !heard.is_empty();
    for heard in heard {
        input(ui, heard);
    }
    remember_step(ui, step);
    let after = ui.tour.as_ref().map(|tour| tour.flow.view());
    changed || after.as_ref() != Some(&before)
}

pub(super) fn input(ui: &mut Ui, input: Input) {
    let Some(tour) = &mut ui.tour else {
        return;
    };
    let step = tour.flow.step();
    let effect = tour.flow.on(input);
    apply(ui, effect);
    remember_step(ui, step);
}

/// Saves a new step so a run quit part-way resumes there. The story sounds
/// the success chime itself, on Hex is ready.
fn remember_step(ui: &Ui, before: Step) {
    if let Some(tour) = &ui.tour
        && tour.flow.step() != before
        && ui.live()
    {
        crate::desktop::onboarding::record_step(tour.flow.step());
    }
}

fn apply(ui: &mut Ui, effect: Effect) {
    match effect {
        Effect::DownloadModel => {
            // The model onboarding's choice picked, else the recommended one.
            let selection = ui
                .found
                .choice
                .clone()
                .unwrap_or_else(|| ui.settings.transcription.clone());
            if crate::transcription::models::validate(&selection).is_ok() {
                ui.send(Request::ChooseModel {
                    model: selection.model,
                    language: selection.language,
                });
            }
        }
        Effect::AdvanceSoon => {
            if let Some(tour) = &mut ui.tour {
                tour.advance_at = Some(Instant::now());
            }
        }
        Effect::RequestMicrophone => ui.ask(PlatformRequest::Permission(
            crate::desktop::onboarding::PermissionAction::RequestMicrophone,
        )),
        Effect::ShowGuide(kind) => ui.ask(PlatformRequest::Permission(
            crate::desktop::onboarding::PermissionAction::ShowGuide(kind),
        )),
        Effect::ResetPermission(kind) => ui.ask(PlatformRequest::Permission(
            crate::desktop::onboarding::PermissionAction::ResetAndPrompt(kind),
        )),
        Effect::Relaunch => ui.ask(PlatformRequest::Relaunch),
        Effect::Finish => {
            ui.tour = None;
            ui.first_run = None;
        }
        Effect::None => {}
    }
}

/// A first run at `step`, with the selected model not yet installed.
pub(super) fn fixture(ui: &mut Ui, step: Step, downloading: bool) {
    ui.picker.installed = Some(Vec::new());
    let status = ui.platform.setup_status(false);
    let mut flow = Flow::new(
        Step::Welcome,
        status,
        ui.platform.caps.permissions,
        ui.platform.caps.practice_dictation,
    );
    flow.hold_download(true);
    if step != Step::Welcome {
        flow.on(Input::Continue);
    }
    ui.tour = Some(Tour::new(flow, false));
    if downloading {
        super::found_choice::start_selected(ui);
        set_downloaded(ui, 62);
        poll(ui);
    }
}

pub(super) fn set_downloaded(ui: &mut Ui, percent: u64) {
    if let Some(view) = &mut ui.view {
        let model = ui.settings.transcription.model;
        let bytes = crate::transcription::models::definition(model)
            .download_bytes()
            .unwrap_or(0);
        view.transcription.preparing = Some(model);
        view.transcription.downloaded_bytes = bytes * percent / 100;
    }
}

/// The macOS tour's states, walked as a person would.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MacStep {
    Setup,
    SetupStale,
    SetupDenied,
    /// Every permission on, Accessibility waiting for a restart.
    SetupRestart,
    /// Every permission on, the model still downloading.
    SetupWaiting,
    TryIt,
    /// The shortcut listener failed after setup.
    TryItReopen,
    Done,
}

/// Walks a fresh macOS first run to `state`. `ui.platform` must carry
/// macOS capabilities.
pub(super) fn mac_fixture(ui: &mut Ui, state: MacStep) {
    use PermissionState::{NeedsRequest, NeedsRestart, NeedsSettings, Ready};
    let fresh = crate::desktop::onboarding::SetupStatus {
        microphone: NeedsRequest,
        accessibility: NeedsSettings,
        transcription_model: false,
    };
    let installed = matches!(state, MacStep::TryIt | MacStep::TryItReopen | MacStep::Done);
    let permissions = match state {
        MacStep::Setup => fresh,
        MacStep::SetupStale => crate::desktop::onboarding::SetupStatus {
            microphone: Ready,
            ..fresh
        },
        MacStep::SetupDenied => crate::desktop::onboarding::SetupStatus {
            microphone: NeedsSettings,
            ..fresh
        },
        MacStep::SetupRestart => crate::desktop::onboarding::SetupStatus {
            microphone: Ready,
            accessibility: NeedsRestart,
            ..fresh
        },
        MacStep::SetupWaiting => crate::desktop::onboarding::SetupStatus {
            microphone: Ready,
            accessibility: Ready,
            ..fresh
        },
        _ => crate::desktop::onboarding::SetupStatus {
            microphone: Ready,
            accessibility: Ready,
            transcription_model: true,
        },
    };
    ui.platform.permissions = fresh;
    ui.picker.installed = Some(Vec::new());
    ui.tour = Some(Tour::new(
        Flow::new(Step::Welcome, fresh, true, true),
        false,
    ));
    // Permission and practice scenes have already confirmed a model.
    poll(ui);
    super::found_choice::start_selected(ui);
    input(ui, Input::Continue);
    if state == MacStep::SetupStale {
        for item in [SetupItem::Microphone, SetupItem::Accessibility] {
            input(ui, Input::Grant(item));
        }
        ui.platform
            .note_stale(crate::desktop::onboarding::PermissionKind::Accessibility);
    }
    ui.platform.permissions = permissions;
    if installed {
        ui.picker.installed = Some(vec![ui.settings.transcription.model]);
        if let Some(view) = &mut ui.view {
            view.transcription.preparing = None;
        }
    } else {
        set_downloaded(ui, 31);
    }
    poll(ui);
    if installed {
        input(ui, Input::Continue);
    }
    if let Some(tour) = &mut ui.tour {
        tour.advance_at = None;
    }
    if state == MacStep::TryItReopen {
        input(
            ui,
            Input::ListenerFailed {
                relaunch_allowed: false,
            },
        );
    }
    if state == MacStep::Done {
        input(ui, Input::Skip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::onboarding::ItemStatus;
    use crate::rune_ui::onboarding_kit::live::{self, ModelStep, Page};
    use crate::transcription::models::{self as models, TranscriptionModelId as M};

    fn fresh_onboarding(language: &str) -> Ui {
        let mut ui = crate::rune_ui::portal_fixture();
        ui.settings.transcription = models::recommended_selection(language);
        fixture(&mut ui, Step::Setup, false);
        live::fixture(&mut ui, Page::Download);
        ui
    }

    fn requests(ui: &Ui) -> &[Request] {
        match &ui.service {
            crate::rune_ui::Service::Fixture(sent) => sent,
            _ => panic!("expected fixture service"),
        }
    }

    #[test]
    fn onboarding_always_shows_the_model_choice_without_downloading() {
        for language in ["en", "ru", "kk", "fr", "ja"] {
            let mut ui = fresh_onboarding(language);
            for _ in 0..4 {
                poll(&mut ui);
                // Reading the page, even for minutes, never starts a model.
                ui.first_run.as_mut().unwrap().now += 60.0;
                live::poll(&mut ui, &mut rune::Tx::default());
            }
            let live = ui.first_run.as_ref().unwrap().live.as_ref().unwrap();
            assert_eq!(live.model_step, ModelStep::Choosing);
            assert!(live.holds());
            assert!(live.models[0].recommended && live.models[0].selected);
            assert_eq!(live.models.iter().filter(|row| row.selected).count(), 1);
            assert_eq!(
                live.models[0].name,
                models::definition(models::recommended_selection(language).model).name
            );
            for model in [
                M::GigaAmMultilingual,
                M::GigaAmV3,
                M::WhisperLargeV3Turbo,
                M::ParakeetUnifiedEnglish,
                M::ParakeetV3,
            ] {
                assert_eq!(
                    live.models
                        .iter()
                        .filter(|row| row.name == models::definition(model).name)
                        .count(),
                    1
                );
            }
            assert!(requests(&ui).is_empty(), "{language}: no pick, no download");
            assert!(ui.view.as_ref().unwrap().transcription.preparing.is_none());
        }
    }

    #[test]
    fn onboarding_continue_downloads_only_the_preselected_recommendation() {
        for language in ["en", "ru", "kk", "ja"] {
            let mut ui = fresh_onboarding(language);
            live::continue_model(&mut ui);
            let choice = models::recommended_selection(language);
            assert!(
                matches!(requests(&ui), [Request::ChooseModel { model, language }] if *model == choice.model && *language == choice.language)
            );
            poll(&mut ui);
            assert_eq!(requests(&ui).len(), 1, "no automatic second request");
        }
    }

    #[test]
    fn onboarding_downloads_the_catalog_row_picked_and_tracks_its_progress() {
        for model in [
            M::GigaAmMultilingual,
            M::GigaAmV3,
            M::WhisperLargeV3Turbo,
            M::ParakeetV3,
        ] {
            let mut ui = fresh_onboarding("en");
            let index = ui
                .first_run
                .as_ref()
                .unwrap()
                .live
                .as_ref()
                .unwrap()
                .models
                .iter()
                .position(|row| row.name == models::definition(model).name)
                .unwrap();
            super::super::found_choice::pick(&mut ui, index);
            let expected_language = models::definition(model).language_for("en");
            assert!(
                matches!(requests(&ui), [Request::ChooseModel { model: picked, language }] if *picked == model && language == expected_language)
            );
            // The worker reports progress before saving the new selection.
            assert_ne!(ui.settings.transcription.model, model);
            let view = ui.view.as_mut().unwrap();
            view.transcription.preparing = Some(model);
            view.transcription.downloaded_bytes =
                models::definition(model).download_bytes().unwrap() / 2;
            poll(&mut ui);
            live::poll(&mut ui, &mut rune::Tx::default());
            let live = ui.first_run.as_ref().unwrap().live.as_ref().unwrap();
            assert!(live.models[index].line.contains("50%"));
            assert!(live.models[index].selected);
            assert!(!live.stalled && !live.failed);
            live::continue_model(&mut ui);
            poll(&mut ui);
            assert_eq!(requests(&ui).len(), 1, "Continue keeps the pick");
        }
    }

    #[test]
    fn onboarding_found_catalog_files_share_the_catalog_row() {
        let mut ui = fresh_onboarding("en");
        ui.found.models = super::super::found::fixture();
        live::poll(&mut ui, &mut rune::Tx::default());
        let rows = &ui.first_run.as_ref().unwrap().live.as_ref().unwrap().models;
        assert_eq!(
            rows.iter()
                .filter(|row| row.name == models::definition(M::ParakeetV3).name)
                .count(),
            1
        );
        assert!(
            rows.iter()
                .find(|row| row.name == models::definition(M::ParakeetV3).name)
                .unwrap()
                .line
                .contains("On this computer")
        );
        assert_eq!(rows.last().unwrap().name, "Whisper Small");
        assert!(requests(&ui).is_empty());
    }

    #[test]
    fn onboarding_never_releases_an_unpicked_download_after_setup() {
        let mut ui = fresh_onboarding("en");
        ui.tour = Some(Tour::new(
            Flow::new(Step::TryIt, ui.platform.setup_status(false), true, true),
            false,
        ));
        poll(&mut ui);
        assert!(requests(&ui).is_empty());
    }

    #[test]
    fn mac_stale_fixture_marks_accessibility_stale_and_dispatches_reset() {
        let mut ui = crate::rune_ui::mac_fixture();
        mac_fixture(&mut ui, MacStep::SetupStale);
        let rows = ui.tour.as_ref().unwrap().flow.view().rows;
        assert!(
            rows.iter()
                .any(|row| row.item == SetupItem::Accessibility && row.status == ItemStatus::Stale)
        );
        input(&mut ui, Input::Grant(SetupItem::Accessibility));
        assert!(
            !ui.platform
                .stale(crate::desktop::onboarding::PermissionKind::Accessibility)
        );
        match &ui.host {
            crate::rune_ui::platform::PlatformHost::Fixture(sent) => {
                assert_eq!(
                    sent.last(),
                    Some(&PlatformRequest::Permission(
                        crate::desktop::onboarding::PermissionAction::ResetAndPrompt(
                            crate::desktop::onboarding::PermissionKind::Accessibility
                        )
                    ))
                );
            }
            _ => panic!("expected fixture host"),
        }
    }
}
