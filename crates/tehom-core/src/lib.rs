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
    /// Where this source's automatic clips are posted: `None` follows the
    /// default account, `Some(0)` posts nowhere, otherwise that account.
    #[serde(default)]
    pub publish_account: Option<i64>,
    /// Channel picture saved on this computer, when the platform gave one.
    #[serde(default)]
    pub avatar: Option<PathBuf>,
    /// Follower or subscriber count as the platform shows it ("1.2M subscribers").
    #[serde(default)]
    pub followers: Option<String>,
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
    /// Burned-in captions, timed relative to the clip start.
    #[serde(default)]
    pub captions: Vec<Cue>,
    #[serde(default)]
    pub caption_style: CaptionStyle,
    #[serde(default)]
    pub sound_effect: SoundEffect,
}

/// One caption line; times are milliseconds from the clip start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionSize {
    Small,
    #[default]
    Medium,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionPosition {
    Top,
    Middle,
    #[default]
    Bottom,
}

/// How burned-in captions look. Colors are `#RRGGBB`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptionStyle {
    pub enabled: bool,
    pub color: String,
    pub outline: String,
    pub size: CaptionSize,
    pub position: CaptionPosition,
    pub uppercase: bool,
    /// Font family; one of the families Tehom ships (`CAPTION_FONTS`).
    pub font: String,
    /// Exact height, as a percentage from the top of the frame (0–100).
    /// `None` uses `position`.
    pub vertical: Option<u8>,
}
impl Default for CaptionStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            color: "#FFFFFF".into(),
            outline: "#000000".into(),
            size: CaptionSize::Medium,
            position: CaptionPosition::Bottom,
            uppercase: false,
            font: "Staatliches".into(),
            vertical: None,
        }
    }
}
/// Caption font families bundled with Tehom (all cover Turkish letters).
pub const CAPTION_FONTS: [&str; 4] = ["Staatliches", "Anton", "Righteous", "Bebas Neue"];

/// A short synthesized sound at the start of a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundEffect {
    #[default]
    None,
    Whoosh,
    Pop,
}

/// A rough, name-based quality tier for an analysis model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strength {
    Fast,
    Balanced,
    Strongest,
}

/// One entry in the model picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    /// `None` for local models, whose quality cannot be known.
    pub strength: Option<Strength>,
    /// One plain-language line for non-technical users.
    pub note: Option<String>,
}

/// Local speech-to-text readiness: whisper.cpp program and model file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeechStatus {
    pub program: Option<PathBuf>,
    pub model: Option<PathBuf>,
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

fn enabled() -> bool {
    true
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

/// Where a clip can be published. Only Instagram is implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Instagram,
    YoutubeShorts,
    Tiktok,
}
impl Platform {
    pub const ALL: [Self; 3] = [Self::Instagram, Self::YoutubeShorts, Self::Tiktok];
    pub fn name(self) -> &'static str {
        match self {
            Self::Instagram => "instagram",
            Self::YoutubeShorts => "youtube_shorts",
            Self::Tiktok => "tiktok",
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|platform| platform.name() == name)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Instagram => "Instagram",
            Self::YoutubeShorts => "YouTube Shorts",
            Self::Tiktok => "TikTok",
        }
    }
    /// Whether publishing to this platform is implemented.
    pub fn available(self) -> bool {
        self == Self::Instagram
    }
}

/// A connected publishing account. Its session secret lives in the OS keyring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: i64,
    pub platform: Platform,
    pub username: String,
    pub full_name: String,
    /// The session expired or its secret is missing: sign in again.
    pub needs_login: bool,
}

/// One attempt to publish a clip to an account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Publication {
    pub id: i64,
    pub clip_id: i64,
    pub account_id: i64,
    /// "uploading", "published" or "failed".
    pub state: String,
    pub url: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountLogin {
    Done(Account),
    /// `app`: the code comes from an authenticator app; otherwise by SMS.
    TwoFactor {
        hint: Option<String>,
        app: bool,
    },
}

/// How sources are watched and clips are cut. Defaults match the behaviour
/// before these were settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutomationSettings {
    /// Look for new videos this often (1–1440).
    pub check_minutes: u32,
    /// At most this many clips per video (1–20).
    pub max_clips: u32,
    /// Clip length bounds in seconds (min ≥ 5, max ≤ 180, min < max).
    pub min_clip_seconds: u32,
    pub max_clip_seconds: u32,
    /// Keep a downloaded video after its clips are saved as MP4 files.
    pub keep_downloads: bool,
    /// Mode given to links added without choosing one.
    pub new_source_mode: AutomationMode,
    /// Transcribe on this computer when a video has no captions.
    pub transcribe_locally: bool,
}

impl Default for AutomationSettings {
    fn default() -> Self {
        Self {
            check_minutes: 30,
            max_clips: 5,
            min_clip_seconds: 20,
            max_clip_seconds: 90,
            keep_downloads: true,
            new_source_mode: AutomationMode::AutoDraft,
            transcribe_locally: true,
        }
    }
}

impl AutomationSettings {
    /// A plain-language reason these settings cannot be used, if any.
    pub fn problem(&self) -> Option<&'static str> {
        if !(1..=1440).contains(&self.check_minutes) {
            Some("Check for new videos every 1 to 1440 minutes.")
        } else if !(1..=20).contains(&self.max_clips) {
            Some("Choose 1 to 20 clips per video.")
        } else if self.min_clip_seconds < 5 {
            Some("The shortest clip must be at least 5 seconds.")
        } else if self.max_clip_seconds > 180 {
            Some("The longest clip can be at most 180 seconds.")
        } else if self.min_clip_seconds >= self.max_clip_seconds {
            Some("The shortest clip must be shorter than the longest clip.")
        } else if self.new_source_mode == AutomationMode::Paused {
            Some("New links cannot start paused.")
        } else {
            None
        }
    }
}

/// Light or dark look; `System` follows the operating system.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

/// The colour of primary buttons and highlights.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    #[default]
    Violet,
    Blue,
    Green,
    Orange,
    Pink,
    /// Black and white only.
    Mono,
}
impl Accent {
    pub const ALL: [Self; 6] = [
        Self::Violet,
        Self::Blue,
        Self::Green,
        Self::Orange,
        Self::Pink,
        Self::Mono,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Violet => "Violet",
            Self::Blue => "Blue",
            Self::Green => "Green",
            Self::Orange => "Orange",
            Self::Pink => "Pink",
            Self::Mono => "Black and white",
        }
    }
}

/// Desktop sidebar width in pixels: default and bounds.
pub const SIDEBAR_WIDTH: u32 = 280;
pub const SIDEBAR_WIDTHS: std::ops::RangeInclusive<u32> = 200..=480;
fn sidebar_width() -> u32 {
    SIDEBAR_WIDTH
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
    /// Look for a new release when the desktop app starts.
    #[serde(default = "enabled")]
    pub check_updates: bool,
    /// Default caption look for newly created clips.
    #[serde(default)]
    pub caption_style: CaptionStyle,
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// Newest first (at most 100).
    #[serde(default)]
    pub publications: Vec<Publication>,
    /// Account that automatic exports are published to; off when `None`.
    #[serde(default)]
    pub auto_publish_account: Option<i64>,
    #[serde(default)]
    pub automation: AutomationSettings,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub accent: Accent,
    #[serde(default = "sidebar_width")]
    pub sidebar_width: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automation_settings_default_to_the_old_behaviour_and_are_bounded() {
        let old: AutomationSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old, AutomationSettings::default());
        assert_eq!((old.min_clip_seconds, old.max_clip_seconds), (20, 90));
        assert!(old.problem().is_none());
        for bad in [
            AutomationSettings {
                check_minutes: 0,
                ..old.clone()
            },
            AutomationSettings {
                check_minutes: 1441,
                ..old.clone()
            },
            AutomationSettings {
                max_clips: 21,
                ..old.clone()
            },
            AutomationSettings {
                min_clip_seconds: 4,
                ..old.clone()
            },
            AutomationSettings {
                max_clip_seconds: 181,
                ..old.clone()
            },
            AutomationSettings {
                min_clip_seconds: 60,
                max_clip_seconds: 60,
                ..old.clone()
            },
            AutomationSettings {
                new_source_mode: AutomationMode::Paused,
                ..old.clone()
            },
        ] {
            assert!(bad.problem().is_some(), "{bad:?}");
        }
    }
    #[test]
    fn old_clips_load_with_caption_defaults() {
        let clip: Clip = serde_json::from_str(
            r#"{"id":1,"video_id":2,"title":"t","start_ms":0,"end_ms":1,"output_path":null}"#,
        )
        .unwrap();
        assert!(clip.captions.is_empty());
        assert_eq!(clip.caption_style, CaptionStyle::default());
        assert!(clip.caption_style.enabled);
        assert_eq!(clip.sound_effect, SoundEffect::None);
        let style: CaptionStyle = serde_json::from_str(r#"{"size":"large"}"#).unwrap();
        assert_eq!(style.size, CaptionSize::Large);
        assert_eq!(style.color, "#FFFFFF");
    }
}
