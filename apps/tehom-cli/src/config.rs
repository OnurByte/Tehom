//! `tehom.toml` schema from spec §16.
//!
//! Every accepted key is either applied or validated against what the engine
//! actually does; values describing unimplemented behavior are rejected with
//! an explanation instead of being silently ignored.

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::{
    io::Read,
    path::{Path, PathBuf},
};
use tehom_core::{AiConfig, AutomationMode};

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub data: Option<Directory>,
    pub automation: Option<Automation>,
    pub export: Option<Export>,
    pub ai: Option<Model>,
    pub server: Option<Server>,
    pub storage: Option<Storage>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    /// `false` lets Auto Export delete a video's downloaded source once all
    /// of its clips are saved. Defaults to keeping downloads.
    pub keep_downloads: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Directory {
    pub directory: PathBuf,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultMode {
    Manual,
    AutoDraft,
    AutoExport,
    Paused,
}
impl From<&DefaultMode> for AutomationMode {
    fn from(mode: &DefaultMode) -> Self {
        match mode {
            DefaultMode::Manual => Self::Manual,
            DefaultMode::AutoDraft => Self::AutoDraft,
            DefaultMode::AutoExport => Self::AutoExport,
            DefaultMode::Paused => Self::Paused,
        }
    }
}
fn enabled() -> bool {
    true
}
fn thirty() -> u64 {
    30
}
fn one() -> u32 {
    1
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Automation {
    /// `false` keeps the daemon from starting; manual CLI commands still work.
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// Mode for `add`/`import-video` when `--mode` is not given.
    pub default_mode: Option<DefaultMode>,
    #[serde(default = "thirty")]
    pub check_interval_minutes: u64,
    /// Upper bounds. The engine currently runs one job at a time, which
    /// satisfies any limit of at least one.
    #[serde(default = "one")]
    pub max_concurrent_downloads: u32,
    #[serde(default = "one")]
    pub max_concurrent_renders: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub directory: Option<PathBuf>,
    pub format: Option<String>,
    pub orientation: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub enum Provider {
    Ollama,
    #[serde(alias = "openai_compatible")]
    OpenaiCompatible,
    Openai,
    Anthropic,
    Gemini,
    Openrouter,
}
impl Provider {
    fn remote(&self) -> Option<tehom_core::AiProvider> {
        use tehom_core::AiProvider as P;
        match self {
            Self::Ollama | Self::OpenaiCompatible => None,
            Self::Openai => Some(P::Openai),
            Self::Anthropic => Some(P::Anthropic),
            Self::Gemini => Some(P::Gemini),
            Self::Openrouter => Some(P::Openrouter),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub provider: Option<Provider>,
    /// Local servers only; remote providers use their fixed API host.
    #[serde(default)]
    pub endpoint: Option<String>,
    pub model: String,
    /// Required for remote providers: captions are sent to that service.
    #[serde(default)]
    pub remote_consent: bool,
}
impl From<Model> for AiConfig {
    fn from(value: Model) -> Self {
        let remote = value.provider.as_ref().and_then(Provider::remote);
        Self {
            endpoint: value
                .endpoint
                .unwrap_or_else(|| AiConfig::default().endpoint),
            model: value.model,
            provider: remote.unwrap_or_default(),
            remote_consent: value.remote_consent,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    #[serde(default)]
    pub enabled: bool,
    pub listen: Option<String>,
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let path = path
            .canonicalize()
            .context("Configuration file not found")?;
        ensure!(path.is_file(), "The configuration must be a regular file");
        let mut bytes = Vec::new();
        std::fs::File::open(&path)?
            .take(65537)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 65536, "The configuration exceeds 64 KiB");
        Self::parse(
            std::str::from_utf8(&bytes)?,
            path.parent().context("Configuration folder not found")?,
        )
    }
    pub fn parse(text: &str, base: &Path) -> Result<Self> {
        let mut config: Self = toml::from_str(text).context("Invalid Tehom configuration")?;
        if let Some(automation) = &config.automation {
            ensure!(
                (1..=1440).contains(&automation.check_interval_minutes),
                "check_interval_minutes must be 1 to 1440"
            );
            ensure!(
                (1..=8).contains(&automation.max_concurrent_downloads),
                "max_concurrent_downloads must be 1 to 8"
            );
            ensure!(
                (1..=8).contains(&automation.max_concurrent_renders),
                "max_concurrent_renders must be 1 to 8"
            );
        }
        if let Some(export) = &config.export {
            if let Some(format) = &export.format {
                ensure!(
                    format == "mp4",
                    "export.format must be \"mp4\"; no other format is supported"
                );
            }
            if let Some(orientation) = &export.orientation {
                ensure!(
                    tehom_core::ExportProfile::parse(orientation).is_some(),
                    "export.orientation must be \"vertical\", \"horizontal\" or \"original\""
                );
            }
        }
        if let Some(ai) = &config.ai {
            ensure!(
                !ai.model.trim().is_empty(),
                "The model name cannot be empty"
            );
            let openai_style = ai
                .endpoint
                .as_deref()
                .unwrap_or("")
                .trim_end_matches('/')
                .ends_with("/v1");
            if let Some(remote) = ai.provider.as_ref().and_then(Provider::remote) {
                ensure!(
                    ai.remote_consent,
                    "provider = \"{}\" sends captions to {}; set remote_consent = true to allow it",
                    remote.name(),
                    remote.label()
                );
                ensure!(
                    ai.endpoint.is_none(),
                    "endpoint is only for local servers; {} uses its own API address",
                    remote.label()
                );
            }
            match ai.provider {
                Some(Provider::Ollama) => ensure!(
                    !openai_style,
                    "provider = \"ollama\" cannot use a /v1 endpoint"
                ),
                Some(Provider::OpenaiCompatible) => ensure!(
                    openai_style,
                    "provider = \"openai-compatible\" needs an endpoint ending in /v1"
                ),
                _ => {}
            }
        }
        if let Some(server) = &config.server {
            if server.enabled {
                bail!("server.enabled is not supported: there is no remote API yet");
            }
            if let Some(listen) = &server.listen {
                let address: std::net::SocketAddr = listen
                    .parse()
                    .context("server.listen must be ADDRESS:PORT")?;
                ensure!(
                    address.ip().is_loopback(),
                    "server.listen must be a loopback address"
                );
            }
        }
        let export = config.export.as_mut().and_then(|e| e.directory.as_mut());
        let data = config.data.as_mut().map(|d| &mut d.directory);
        for directory in [data, export].into_iter().flatten() {
            ensure!(
                !directory.as_os_str().is_empty(),
                "A folder path cannot be empty"
            );
            if directory.is_relative() {
                *directory = base.join(&*directory);
            }
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_example_configuration_is_accepted() {
        // The §16 example, verbatim apart from comments.
        let config = Config::parse(
            r#"
[data]
directory = "/srv/tehom"

[automation]
enabled = true
default_mode = "auto_draft"
check_interval_minutes = 30
max_concurrent_downloads = 2
max_concurrent_renders = 1

[export]
directory = "/srv/tehom/exports"
format = "mp4"
orientation = "vertical"

[ai]
provider = "ollama"
model = "user-selected-model"
endpoint = "http://127.0.0.1:11434"

[server]
enabled = false
listen = "127.0.0.1:8787"
"#,
            Path::new("/etc"),
        )
        .unwrap();
        let automation = config.automation.unwrap();
        assert!(automation.enabled);
        assert!(matches!(
            automation.default_mode,
            Some(DefaultMode::AutoDraft)
        ));
        assert_eq!(automation.max_concurrent_downloads, 2);
        assert_eq!(
            config.export.unwrap().directory.unwrap(),
            PathBuf::from("/srv/tehom/exports")
        );
    }

    #[test]
    fn unimplemented_or_unsafe_values_are_rejected() {
        let base = Path::new("/etc");
        for text in [
            "[automation]\nmax_concurrent_renders = 0",
            "[automation]\nmax_concurrent_downloads = 99",
            "[automation]\ndefault_mode = \"auto_publish\"",
            "[export]\nformat = \"mov\"",
            "[export]\norientation = \"square\"",
            "[export]\ndirectory = \"\"",
            "[ai]\nprovider = \"cloud\"\nendpoint = \"http://127.0.0.1:1\"\nmodel = \"m\"",
            "[ai]\nprovider = \"ollama\"\nendpoint = \"http://127.0.0.1:8080/v1\"\nmodel = \"m\"",
            "[ai]\nprovider = \"openai-compatible\"\nendpoint = \"http://127.0.0.1:11434\"\nmodel = \"m\"",
            "[server]\nenabled = true",
            "[server]\nlisten = \"0.0.0.0:8787\"",
            "[server]\nlisten = \"localhost\"",
            "[server]\ntoken = \"secret\"",
        ] {
            assert!(Config::parse(text, base).is_err(), "{text}");
        }
        let defaults = Config::parse("[automation]\nenabled = false", base).unwrap();
        let automation = defaults.automation.unwrap();
        assert!(!automation.enabled);
        assert_eq!(automation.check_interval_minutes, 30);
        assert_eq!(automation.max_concurrent_renders, 1);
        assert!(
            Config::parse(
                "[ai]\nprovider = \"openai-compatible\"\nendpoint = \"http://127.0.0.1:8080/v1/\"\nmodel = \"m\"",
                base
            )
            .is_ok()
        );
    }
}
