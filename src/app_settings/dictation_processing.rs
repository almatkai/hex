//! What happens to a dictation's text before it pastes: Global's steps and
//! the modes chosen by application or website.

use serde::{Deserialize, Serialize};

use super::{KeyboardModelChoice, TextReplacement};
use crate::dictation::vocabulary::{VocabularyEntry, serialize_saved};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DictationProcessingSettings {
    pub default_mode: DictationMode,
    pub modes: Vec<DictationMode>,
}

impl Default for DictationProcessingSettings {
    fn default() -> Self {
        Self {
            default_mode: DictationMode {
                name: "Global".into(),
                ..Default::default()
            },
            modes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DictationMode {
    pub name: String,
    pub applications: Vec<String>,
    pub browser_hosts: Vec<String>,
    pub replacements: Vec<TextReplacement>,
    pub transformations: Vec<String>,
    pub post_processing: DictationPostProcessing,
    /// The speech model and language this mode selects while it matches the
    /// foreground app (Labs › Switch model with app). None means don't switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech: Option<KeyboardModelChoice>,
    /// Names heard and spelled here ahead of the shared vocabulary.
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "serialize_saved"
    )]
    pub vocabulary: Vec<VocabularyEntry>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RewriteProvider {
    #[default]
    #[serde(rename = "opencode")]
    OpenCode,
    Antigravity,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RewriteStyle {
    #[default]
    Custom,
    Punctuate,
    Translate,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RewriteLanguage {
    #[default]
    English,
    Russian,
    Kazakh,
}

impl RewriteLanguage {
    pub const fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Russian",
            Self::Kazakh => "Kazakh",
        }
    }
}

/// Persisted rewrite settings for one mode. Old settings keep OpenCode.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct DictationPostProcessing {
    pub enabled: bool,
    pub provider: RewriteProvider,
    pub antigravity_model: String,
    pub style: RewriteStyle,
    pub target_language: RewriteLanguage,
    pub prompt: String,
    /// Optional `provider/model` key from the OpenCode catalog.
    pub model: Option<String>,
    pub variant: Option<String>,
    pub deadline_seconds: u64,
}

impl Default for DictationPostProcessing {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: RewriteProvider::default(),
            antigravity_model: crate::antigravity::DEFAULT_MODEL.into(),
            style: RewriteStyle::default(),
            target_language: RewriteLanguage::default(),
            prompt: "Rewrite the transcript into clear, natural text while preserving the speaker's intended meaning.".into(),
            model: None,
            variant: None,
            deadline_seconds: 30,
        }
    }
}
