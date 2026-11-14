use serde::{Deserialize, Serialize};

/// Labs › Remote dictation. The listener never inherits the local API port.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct RemoteDictation {
    pub enabled: bool,
    pub port: u16,
    pub process_transcripts: bool,
}

impl Default for RemoteDictation {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 47821,
            process_transcripts: false,
        }
    }
}
