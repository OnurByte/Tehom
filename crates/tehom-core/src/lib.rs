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

/// One recorded video state change (spec §14 `events`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub video_id: i64,
    pub level: String,
    pub state: String,
    pub message: Option<String>,
    pub created_at: String,
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
    /// Whether `local_path` currently names a regular file.
    #[serde(default)]
    pub local_available: bool,
    /// Export progress of a running job, 0.0–1.0 across all of its clips.
    #[serde(default)]
    pub progress: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub id: i64,
    pub video_id: i64,
    pub title: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub output_path: Option<PathBuf>,
    /// Whether `output_path` currently names a regular file; a moved or
    /// deleted export is reported instead of shown as available.
    #[serde(default)]
    pub output_available: bool,
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

/// Output geometry for automatic exports (spec v0.5 output profiles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportProfile {
    /// 1080×1920, filled and centre-cropped.
    #[default]
    Vertical,
    /// 1920×1080, filled and centre-cropped.
    Horizontal,
    /// Source geometry, rounded down to even dimensions for H.264.
    Original,
}
impl ExportProfile {
    pub const ALL: [Self; 3] = [Self::Vertical, Self::Horizontal, Self::Original];
    pub fn name(self) -> &'static str {
        match self {
            Self::Vertical => "vertical",
            Self::Horizontal => "horizontal",
            Self::Original => "original",
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|profile| profile.name() == name)
    }
    /// Expected output size for a source of `width`×`height`.
    pub fn dimensions(self, width: u32, height: u32) -> (u32, u32) {
        match self {
            Self::Vertical => (1080, 1920),
            Self::Horizontal => (1920, 1080),
            Self::Original => (width & !1, height & !1),
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
    #[serde(default)]
    pub export_profile: ExportProfile,
    /// Most recent state changes, newest first (at most 100).
    #[serde(default)]
    pub events: Vec<Event>,
}
