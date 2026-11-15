//! Optional remote rewriting choices; the account is shared with Meetings.
use super::super::controls::{self, Choice as Pick};
use super::*;
use crate::app_settings::{RewriteLanguage, RewriteProvider, RewriteStyle};

#[allow(clippy::too_many_arguments)]
fn picker(
    ui: &Ui,
    p: Paint,
    key: &'static str,
    label: &str,
    value: &str,
    choices: Vec<Pick>,
    layer: &mut Vec<El>,
) -> El {
    let menu = Menu::Pick(key);
    let control = controls::select(
        p,
        key,
        label,
        value,
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
    setting_row(p, key, label, None, control)
}

pub(super) fn provider_rows(
    ui: &Ui,
    p: Paint,
    current: &DictationMode,
    layer: &mut Vec<El>,
) -> Vec<El> {
    let lab = ui.settings.labs.antigravity_rewriting;
    let selected = current.post_processing.provider;
    if !lab && selected != RewriteProvider::Antigravity {
        return Vec::new();
    }
    let providers = if lab {
        vec![RewriteProvider::OpenCode, RewriteProvider::Antigravity]
    } else {
        vec![RewriteProvider::OpenCode]
    };
    let label = |provider| match provider {
        RewriteProvider::OpenCode => tr("OpenCode"),
        RewriteProvider::Antigravity => tr("Antigravity"),
    };
    let choices = providers
        .into_iter()
        .map(|provider| Pick {
            selected: provider == selected,
            label: label(provider).into(),
            detail: None,
            choose: Box::new(move |ui| {
                update_mode(ui, |m| m.post_processing.provider = provider);
            }),
        })
        .collect();
    let mut rows = vec![picker(
        ui,
        p,
        "rewrite-provider",
        tr("Rewrite provider"),
        label(selected),
        choices,
        layer,
    )];
    if !lab {
        rows.push(note(
            "rewrite-lab-off",
            tr("Turn on Antigravity rewriting in Labs"),
            p.muted(),
        ));
    }
    rows
}

pub(super) fn profile_rows(
    ui: &Ui,
    p: Paint,
    current: &DictationMode,
    layer: &mut Vec<El>,
) -> Vec<El> {
    if !ui.settings.labs.antigravity_rewriting {
        return Vec::new();
    }
    let selected = current.post_processing.style;
    let label = |style| match style {
        RewriteStyle::Custom => tr("Custom instructions"),
        RewriteStyle::Punctuate => tr("Punctuate"),
        RewriteStyle::Translate => tr("Translate to…"),
    };
    let choices = [
        RewriteStyle::Custom,
        RewriteStyle::Punctuate,
        RewriteStyle::Translate,
    ]
    .into_iter()
    .map(|style| Pick {
        selected: style == selected,
        label: label(style).into(),
        detail: None,
        choose: Box::new(move |ui| {
            update_mode(ui, |m| m.post_processing.style = style);
        }),
    })
    .collect();
    let mut rows = vec![picker(
        ui,
        p,
        "rewrite-profile",
        tr("Rewrite profile"),
        label(selected),
        choices,
        layer,
    )];
    if selected == RewriteStyle::Translate {
        let language = current.post_processing.target_language;
        let label = |l| match l {
            RewriteLanguage::English => tr("English"),
            RewriteLanguage::Russian => tr("Russian"),
            RewriteLanguage::Kazakh => tr("Kazakh"),
        };
        let choices = [
            RewriteLanguage::English,
            RewriteLanguage::Russian,
            RewriteLanguage::Kazakh,
        ]
        .into_iter()
        .map(|target| Pick {
            selected: target == language,
            label: label(target).into(),
            detail: None,
            choose: Box::new(move |ui| {
                update_mode(ui, |m| m.post_processing.target_language = target);
            }),
        })
        .collect();
        rows.push(picker(
            ui,
            p,
            "rewrite-language",
            tr("Translation language"),
            label(language),
            choices,
            layer,
        ));
    }
    rows
}

pub(super) fn antigravity_rows(
    ui: &Ui,
    p: Paint,
    current: &DictationMode,
    layer: &mut Vec<El>,
) -> Vec<El> {
    let selected = &current.post_processing.antigravity_model;
    let choices = ui
        .antigravity
        .model_ids()
        .into_iter()
        .map(|model| Pick {
            selected: &model == selected,
            label: model.clone(),
            detail: None,
            choose: Box::new(move |ui| {
                update_mode(ui, |m| m.post_processing.antigravity_model = model.clone());
            }),
        })
        .collect();
    vec![
        super::super::antigravity::account_row(ui, p),
        picker(
            ui,
            p,
            "rewrite-antigravity-model",
            tr("Model"),
            selected,
            choices,
            layer,
        ),
        note(
            "rewrite-privacy",
            tr("Sends only this dictation to your Google account's Antigravity"),
            p.muted(),
        ),
    ]
}
