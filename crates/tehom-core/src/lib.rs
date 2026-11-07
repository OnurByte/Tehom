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
    /// Loopback server for `Local`; ignored for remote providers, whose
    /// endpoints are fixed so no other host can receive transcripts.
    pub endpoint: String,
    pub model: String,
    #[serde(default)]
    pub provider: AiProvider,
    /// Explicit permission to send transcripts to a remote provider (BYOK).
    #[serde(default)]
    pub remote_consent: bool,
}

/// Where clip analysis runs. Remote providers use the user's own API key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    /// Ollama, llama.cpp, LM Studio or vLLM on this computer.
    #[default]
    Local,
    Openai,
    Anthropic,
    Gemini,
    Openrouter,
    /// The signed-in Claude Code CLI on this computer (your subscription).
    ClaudeCode,
    /// The signed-in Codex CLI on this computer (your subscription).
    Codex,
}
impl AiProvider {
    pub const ALL: [Self; 7] = [
        Self::Local,
        Self::Openai,
        Self::Anthropic,
        Self::Gemini,
        Self::Openrouter,
        Self::ClaudeCode,
        Self::Codex,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Openai => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Openrouter => "openrouter",
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.name() == name)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "Local model",
            Self::Openai => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Google Gemini",
            Self::Openrouter => "OpenRouter",
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        }
    }
    pub fn is_remote(self) -> bool {
        self != Self::Local
    }
    /// The only host a remote provider's requests may go to.
    pub fn host(self) -> Option<&'static str> {
        match self {
            Self::Local => None,
            Self::Openai => Some("api.openai.com"),
            Self::Anthropic => Some("api.anthropic.com"),
            Self::Gemini => Some("generativelanguage.googleapis.com"),
            Self::Openrouter => Some("openrouter.ai"),
            Self::ClaudeCode | Self::Codex => None,
        }
    }
    /// Environment variable checked before the OS keyring (useful for the daemon).
    pub fn key_variable(self) -> Option<&'static str> {
        match self {
            Self::Local => None,
            Self::Openai => Some("OPENAI_API_KEY"),
            Self::Anthropic => Some("ANTHROPIC_API_KEY"),
            Self::Gemini => Some("GEMINI_API_KEY"),
            Self::Openrouter => Some("OPENROUTER_API_KEY"),
            Self::ClaudeCode | Self::Codex => None,
        }
    }
    /// A sensible default model, shown as a suggestion.
    pub fn default_model(self) -> &'static str {
        match self {
            Self::Local => "",
            Self::Openai => "gpt-4o-mini",
            Self::Anthropic => "claude-haiku-5-5",
            Self::Gemini => "gemini-2.5-flash",
            Self::Openrouter => "openai/gpt-4o-mini",
            Self::ClaudeCode => "haiku",
            Self::Codex => "",
        }
    }
    /// Providers reached over HTTPS with the user's own API key.
    pub fn uses_api_key(self) -> bool {
        self.host().is_some()
    }
    /// The local agent CLI that runs this provider, signed in by the user.
    pub fn program(self) -> Option<&'static str> {
        match self {
            Self::ClaudeCode => Some("claude"),
            Self::Codex => Some("codex"),
            _ => None,
        }
    }
}
impl Default for AiConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:11434".into(),
            model: String::new(),
            provider: AiProvider::Local,
            remote_consent: false,
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
    /// The first-run guide was completed or skipped.
    #[serde(default)]
    pub onboarded: bool,
}
