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
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub provider: Option<Provider>,
    pub endpoint: String,
    pub model: String,
}
impl From<Model> for AiConfig {
    fn from(value: Model) -> Self {
        Self {
            endpoint: value.endpoint,
            model: value.model,
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
            .context("Yapılandırma dosyası bulunamadı")?;
        ensure!(path.is_file(), "Yapılandırma normal bir dosya olmalı");
        let mut bytes = Vec::new();
        std::fs::File::open(&path)?
            .take(65537)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 65536, "Yapılandırma 64 KiB sınırını aşıyor");
        Self::parse(
            std::str::from_utf8(&bytes)?,
            path.parent().context("Yapılandırma klasörü bulunamadı")?,
        )
    }
    pub fn parse(text: &str, base: &Path) -> Result<Self> {
        let mut config: Self = toml::from_str(text).context("Geçersiz Tehom yapılandırması")?;
        if let Some(automation) = &config.automation {
            ensure!(
                (1..=1440).contains(&automation.check_interval_minutes),
                "Kontrol aralığı 1–1440 dakika olmalı"
            );
            ensure!(
                (1..=8).contains(&automation.max_concurrent_downloads),
                "max_concurrent_downloads 1–8 olmalı"
            );
            ensure!(
                (1..=8).contains(&automation.max_concurrent_renders),
                "max_concurrent_renders 1–8 olmalı"
            );
        }
        if let Some(export) = &config.export {
            if let Some(format) = &export.format {
                ensure!(
                    format == "mp4",
                    "export.format yalnız \"mp4\" olabilir; başka biçim desteklenmiyor"
                );
            }
            if let Some(orientation) = &export.orientation {
                ensure!(
                    orientation == "vertical",
                    "export.orientation yalnız \"vertical\" olabilir; yatay/genel profiller henüz yok"
                );
            }
        }
        if let Some(ai) = &config.ai {
            ensure!(!ai.model.trim().is_empty(), "Model adı boş olamaz");
            let openai_style = ai.endpoint.trim_end_matches('/').ends_with("/v1");
            match ai.provider {
                Some(Provider::Ollama) => ensure!(
                    !openai_style,
                    "provider = \"ollama\" ile /v1 uç noktası kullanılamaz"
                ),
                Some(Provider::OpenaiCompatible) => ensure!(
                    openai_style,
                    "provider = \"openai-compatible\" uç noktası /v1 ile bitmeli"
                ),
                None => {}
            }
        }
        if let Some(server) = &config.server {
            if server.enabled {
                bail!("server.enabled desteklenmiyor: uzak erişim API'si henüz yok");
            }
            if let Some(listen) = &server.listen {
                let address: std::net::SocketAddr =
                    listen.parse().context("server.listen ADRES:PORT olmalı")?;
                ensure!(
                    address.ip().is_loopback(),
                    "server.listen yalnız localhost olabilir"
                );
            }
        }
        let export = config.export.as_mut().and_then(|e| e.directory.as_mut());
        let data = config.data.as_mut().map(|d| &mut d.directory);
        for directory in [data, export].into_iter().flatten() {
            ensure!(!directory.as_os_str().is_empty(), "Klasör yolu boş olamaz");
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
            "[export]\norientation = \"horizontal\"",
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
