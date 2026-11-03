mod config;
mod daemon;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use config::Config;
use std::path::PathBuf;
use tehom_automation::Automation;
use tehom_core::{AiConfig, AutomationMode};

#[derive(Parser)]
#[command(name = "tehom", version, about = "Yerel video ve kanal otomasyonu")]
struct Args {
    #[arg(long, global = true)]
    data: Option<PathBuf>,
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    Manual,
    AutoDraft,
    AutoExport,
    Paused,
}
impl From<Mode> for AutomationMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Manual => Self::Manual,
            Mode::AutoDraft => Self::AutoDraft,
            Mode::AutoExport => Self::AutoExport,
            Mode::Paused => Self::Paused,
        }
    }
}
#[derive(Subcommand)]
enum Command {
    Status,
    Add {
        url: String,
        #[arg(long, value_enum, default_value = "auto-draft")]
        mode: Mode,
    },
    Mode {
        id: i64,
        #[arg(value_enum)]
        mode: Mode,
    },
    Resume {
        id: i64,
    },
    Check,
    RunOnce,
    Daemon {
        #[arg(long,value_parser=clap::value_parser!(u64).range(1..=1440))]
        interval_minutes: Option<u64>,
    },
    Ai {
        #[arg(long, default_value = "http://127.0.0.1:11434")]
        endpoint: String,
        #[arg(long)]
        model: String,
    },
    Models,
    ExportDirectory {
        path: PathBuf,
    },
    ImportVideo {
        path: PathBuf,
        #[arg(long, value_enum, default_value = "auto-draft")]
        mode: Mode,
    },
    ImportTranscript {
        id: i64,
        path: PathBuf,
    },
    Retry {
        id: i64,
    },
    Cancel {
        id: i64,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = args
        .config
        .as_deref()
        .map(Config::load)
        .transpose()?
        .unwrap_or_default();
    let configured_interval = config
        .automation
        .map(|a| a.check_interval_minutes)
        .unwrap_or(30);
    let data = args
        .data
        .or_else(|| config.data.map(|d| d.directory))
        .or_else(|| {
            directories::ProjectDirs::from("org", "Tehom", "Tehom")
                .map(|d| d.data_local_dir().to_path_buf())
        })
        .context("Veri klasörü belirlenemedi")?;
    let engine = Automation::open(data).await?;
    if let Some(ai) = config.ai {
        engine.set_ai(ai.into()).await?;
    }
    if let Some(export) = config.export {
        engine.set_export_directory(export.directory).await?;
    }
    match args.command {
        Command::Status => println!(
            "{}",
            serde_json::to_string_pretty(&engine.snapshot().await?)?
        ),
        Command::Add { url, mode } => println!(
            "{}",
            serde_json::to_string_pretty(&engine.add_source(&url, mode.into()).await?)?
        ),
        Command::Mode { id, mode } => engine.set_mode(id, mode.into()).await?,
        Command::Resume { id } => engine.resume_source(id).await?,
        Command::Check => println!("{} yeni video", engine.check_now().await?),
        Command::RunOnce => {
            engine.check_now().await?;
            while engine.process_next().await? {}
        }
        Command::Daemon { interval_minutes } => {
            let interval_minutes = interval_minutes.unwrap_or(configured_interval);
            daemon::run(&engine, interval_minutes).await?;
        }
        Command::Ai { endpoint, model } => engine.set_ai(AiConfig { endpoint, model }).await?,
        Command::Models => {
            for model in engine.local_models().await? {
                println!("{model}");
            }
        }
        Command::ExportDirectory { path } => engine.set_export_directory(path).await?,
        Command::ImportVideo { path, mode } => println!(
            "{}",
            serde_json::to_string_pretty(&engine.import_video(path, mode.into()).await?)?
        ),
        Command::ImportTranscript { id, path } => engine.import_transcript(id, path).await?,
        Command::Retry { id } => engine.retry_video(id).await?,
        Command::Cancel { id } => engine.cancel_video(id).await?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_daemon_interval_and_channel_mode() {
        assert!(Args::try_parse_from(["tehom", "daemon", "--interval-minutes", "0"]).is_err());
        let args = Args::try_parse_from([
            "tehom",
            "--data",
            "/tmp/example",
            "add",
            "https://youtube.com/@example",
            "--mode",
            "auto-export",
        ])
        .unwrap();
        assert!(matches!(
            args.command,
            Command::Add {
                mode: Mode::AutoExport,
                ..
            }
        ));
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;
    #[test]
    fn reads_config_and_resolves_paths() {
        let config = Config::parse("[data]\ndirectory='data'\n[automation]\ncheck_interval_minutes=12\n[export]\ndirectory='clips'", std::path::Path::new("/srv/config")).unwrap();
        assert_eq!(
            config.data.unwrap().directory,
            PathBuf::from("/srv/config/data")
        );
        assert_eq!(config.automation.unwrap().check_interval_minutes, 12);
        assert!(
            Config::parse(
                "[automation]\ncheck_interval_minutes=0",
                std::path::Path::new(".")
            )
            .is_err()
        );
        assert!(Config::parse("[ai]\nprovider='cloud'", std::path::Path::new(".")).is_err());
        assert!(Args::try_parse_from(["tehom", "daemon", "--config", "tehom.toml"]).is_ok());
    }
}
