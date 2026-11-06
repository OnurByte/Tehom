use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationMode {
    Manual,
    AutoDraft,
    AutoExport,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: i64,
    pub url: String,
    pub channel_id: String,
    pub title: String,
    pub mode: AutomationMode,
    pub last_checked: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Video {
    pub id: i64,
    pub source_id: i64,
    pub external_id: String,
    pub title: String,
    pub url: String,
    pub state: String,
    pub error: Option<String>,
    pub local_path: Option<PathBuf>,
    /// Language of the transcript in use: the latest upload, else platform captions.
    #[serde(default)]
    pub transcript_language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub id: i64,
    pub video_id: i64,
    pub title: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub output_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub endpoint: String,
    pub model: String,
}
impl Default for AiConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:11434".into(),
            model: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineSnapshot {
    pub sources: Vec<Source>,
    pub videos: Vec<Video>,
    pub clips: Vec<Clip>,
    pub ai: AiConfig,
    pub export_directory: PathBuf,
}
