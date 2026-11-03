use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    io::Read,
    path::{Path, PathBuf},
};
use tehom_core::AiConfig;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub data: Option<Directory>,
    pub automation: Option<Automation>,
    pub export: Option<Directory>,
    pub ai: Option<Model>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Directory {
    pub directory: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Automation {
    pub check_interval_minutes: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
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
        }
        if let Some(ai) = &config.ai {
            ensure!(!ai.model.trim().is_empty(), "Model adı boş olamaz");
        }
        for folder in [&mut config.data, &mut config.export].into_iter().flatten() {
            ensure!(
                !folder.directory.as_os_str().is_empty(),
                "Klasör yolu boş olamaz"
            );
            if folder.directory.is_relative() {
                folder.directory = base.join(&folder.directory);
            }
        }
        Ok(config)
    }
}
