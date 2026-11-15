//! Onboarding always offers a speech model choice: the language's
//! recommendation first, then a small catalog list and files found here.
//! A pick or Continue starts preparation; merely showing the list never
//! downloads anything. Found files are checked and used in place.

use super::Ui;
use crate::desktop::i18n::{tr, tr_fill};
use crate::desktop::onboarding::{Input, ItemStatus, SetupItem};
use crate::transcription::found_models::Found;
use crate::transcription::models::{
    self as transcription_models, TranscriptionModelId, TranscriptionSelection,
};

/// Whether the list waits for its first pick, so Continue starts the
/// selected row.
pub(super) fn pending(ui: &Ui) -> bool {
    ui.found.picked.is_none() && !ui.found.adopting()
}

/// Hex's recommended model as the list first showed it: picking a found
/// model changes the saved selection, and the rows stay as they were.
fn recommended(ui: &Ui) -> TranscriptionSelection {
    ui.found
        .recommended
        .clone()
        .unwrap_or_else(|| ui.settings.transcription.clone())
}

/// One row of the list: a catalog model, or another found file.
enum Entry<'a> {
    Catalog {
        model: TranscriptionModelId,
        on_disk: bool,
    },
    Found(&'a Found),
}

fn entries(ui: &Ui) -> Vec<Entry<'_>> {
    let recommendation = recommended(ui);
    use TranscriptionModelId::{
        GigaAmMultilingual, GigaAmV3, ParakeetUnifiedEnglish, ParakeetV3, WhisperLargeV3Turbo,
    };
    let mut catalog = vec![recommendation.model];
    for model in [
        GigaAmMultilingual,
        GigaAmV3,
        WhisperLargeV3Turbo,
        ParakeetUnifiedEnglish,
        ParakeetV3,
    ] {
        if !catalog.contains(&model) {
            catalog.push(model);
        }
    }
    let language = recommendation.language;
    catalog
        .iter()
        .map(|&model| {
            let definition = transcription_models::definition(model);
            Entry::Catalog {
                model,
                on_disk: ui
                    .found
                    .models
                    .iter()
                    .any(|found| found.catalog == Some(model))
                    || ui
                        .picker
                        .installed(definition, definition.language_for(&language)),
            }
        })
        .chain(
            ui.found
                .models
                .iter()
                .filter(|found| found.catalog.is_none_or(|model| !catalog.contains(&model)))
                .map(Entry::Found),
        )
        .collect()
}

/// Compact language descriptions, translated independently of model names.
fn languages(model: TranscriptionModelId) -> String {
    match model {
        TranscriptionModelId::ParakeetUnifiedEnglish => tr("English").into(),
        TranscriptionModelId::ParakeetV3 => tr("25 European languages").into(),
        TranscriptionModelId::GigaAmMultilingual => {
            tr("Russian, Kazakh, Kyrgyz, Uzbek and English, without punctuation").into()
        }
        TranscriptionModelId::GigaAmV3 => tr("Russian, with punctuation").into(),
        TranscriptionModelId::WhisperLargeV3Turbo => tr("Multilingual").into(),
        _ if transcription_models::definition(model).languages.len() > 1 => {
            tr("Multilingual").into()
        }
        _ => transcription_models::definition(model)
            .languages
            .iter()
            .map(|code| transcription_models::language_name(code))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// One row of the list as the first run shows it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ChoiceRow {
    pub(super) name: String,
    pub(super) line: String,
    pub(super) recommended: bool,
    pub(super) selected: bool,
}

/// The list, one radio row per model.
pub(super) fn rows(ui: &Ui, model: ItemStatus) -> Vec<ChoiceRow> {
    let selected = ui.found.picked.unwrap_or(0);
    entries(ui)
        .into_iter()
        .enumerate()
        .map(|(index, entry)| row(ui, &entry, index == selected, index == 0, model))
        .collect()
}

fn row(
    ui: &Ui,
    entry: &Entry<'_>,
    selected: bool,
    recommended: bool,
    model: ItemStatus,
) -> ChoiceRow {
    let name = match entry {
        Entry::Catalog { model, .. } => transcription_models::definition(*model).name,
        Entry::Found(found) => found.catalog.map_or(found.name.as_str(), |id| {
            transcription_models::definition(id).name
        }),
    };
    let started = selected && ui.found.picked.is_some();
    let line = match (entry, started, model) {
        (_, true, ItemStatus::Failed) => tr("Couldn't prepare this model").to_owned(),
        (_, true, ItemStatus::Ready) if !ui.found.adopting() => tr("Ready").to_owned(),
        (Entry::Catalog { on_disk: false, .. }, true, ItemStatus::Downloading(progress)) => {
            tr_fill(
                "Downloading… {}%",
                &((progress * 100.0).round() as u32).to_string(),
            )
        }
        (_, true, _) if ui.found.adopting() || model != ItemStatus::Ready => {
            tr("Checking…").to_owned()
        }
        (
            Entry::Catalog {
                model,
                on_disk: false,
            },
            ..,
        ) => tr_fill(
            "{} download",
            &transcription_models::definition(*model).size_label(),
        ),
        (
            Entry::Catalog {
                model,
                on_disk: true,
            },
            ..,
        ) => format!(
            "{} · {}",
            tr("On this computer"),
            transcription_models::definition(*model).size_label()
        ),
        (Entry::Found(found), ..) if found.catalog.is_some() => tr("On this computer").to_owned(),
        (Entry::Found(_), ..) => tr("On this computer · not tested by Hex").to_owned(),
    };
    let line = match entry {
        Entry::Catalog { model, .. } => format!("{line} · {}", languages(*model)),
        Entry::Found(_) => line,
    };
    ChoiceRow {
        name: name.to_owned(),
        line,
        recommended,
        selected,
    }
}

/// Whether the pick is under way or ready, so the first run can move on:
/// ours downloading, or a model on this computer checked and in use.
pub(super) fn started(ui: &Ui, model: ItemStatus) -> bool {
    ui.found.picked.is_some()
        && !ui.found.adopting()
        && matches!(model, ItemStatus::Ready | ItemStatus::Downloading(_))
}

/// Starts the selected row when the person continues without picking one.
pub(super) fn start_selected(ui: &mut Ui) {
    pick(ui, ui.found.picked.unwrap_or(0));
}

/// Picks row `index` and starts it. A newer pick supersedes one still
/// preparing, as the picker's does.
pub(super) fn pick(ui: &mut Ui, index: usize) {
    let failed = ui
        .tour
        .as_ref()
        .is_some_and(|tour| tour.flow.view().model == ItemStatus::Failed);
    if (ui.found.picked == Some(index) && !failed) || ui.found.adopting() {
        return;
    }
    // Preserve the initial recommendation even if the host saves a later pick.
    ui.found
        .recommended
        .get_or_insert_with(|| ui.settings.transcription.clone());
    let (catalog, found) = match entries(ui).get(index) {
        None => return,
        Some(Entry::Catalog { model, .. }) => (Some(*model), None),
        Some(Entry::Found(found)) => (found.catalog, Some((*found).clone())),
    };
    ui.found.picked = Some(index);
    ui.found.error = None;
    match catalog {
        Some(model) => {
            let language = transcription_models::definition(model)
                .language_for(&recommended(ui).language)
                .to_owned();
            prepare(ui, TranscriptionSelection { model, language });
        }
        // Hashed once and registered in place first, then prepared.
        None => {
            if let Some(found) = found {
                super::found::adopt(ui, found, true);
            }
        }
    }
}

/// Prepares `selection` for onboarding: through the tour the first time,
/// so its progress shows, and as a newer choice afterwards.
pub(super) fn prepare(ui: &mut Ui, selection: TranscriptionSelection) {
    let requested = ui.tour.as_ref().is_some_and(|tour| {
        !matches!(
            tour.flow.view().model,
            ItemStatus::Todo | ItemStatus::Failed
        )
    });
    ui.found.choice = Some(selection.clone());
    if requested {
        ui.send(super::Request::ChooseModel {
            model: selection.model,
            language: selection.language,
        });
    } else {
        super::onboarding::input(ui, Input::Grant(SetupItem::Model));
    }
}
