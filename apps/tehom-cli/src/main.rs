mod config;
mod daemon;
mod service;
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
#[derive(Clone, Copy, ValueEnum)]
enum Profile {
    Vertical,
    Horizontal,
    Original,
}
impl From<Profile> for tehom_core::ExportProfile {
    fn from(profile: Profile) -> Self {
        match profile {
            Profile::Vertical => Self::Vertical,
            Profile::Horizontal => Self::Horizontal,
            Profile::Original => Self::Original,
        }
    }
}
#[derive(Subcommand)]
enum Command {
    Status,
    /// Recent job state changes (read-only; works while the daemon runs).
    Events {
        #[arg(long)]
        video: Option<i64>,
    },
    Add {
        url: String,
        /// Defaults to automation.default_mode, else auto-draft.
        #[arg(long, value_enum)]
        mode: Option<Mode>,
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
    /// Output geometry for automatic exports.
    ExportProfile {
        #[arg(value_enum)]
        profile: Profile,
    },
    ImportVideo {
        path: PathBuf,
        /// Defaults to automation.default_mode, else auto-draft.
        #[arg(long, value_enum)]
        mode: Option<Mode>,
    },
    ReviseClip {
        id: i64,
        #[arg(long)]
        start_ms: u64,
        #[arg(long)]
        end_ms: u64,
    },
    /// Delete downloaded source media of completed videos (never local files).
    Cleanup {
        /// Only report what would be freed.
        #[arg(long)]
        dry_run: bool,
    },
    /// Write the transcript in effect as an editable SRT (never overwrites).
    ExportTranscript {
        id: i64,
        path: PathBuf,
    },
    /// Save the original video file to PATH (a file or a folder).
    SaveOriginal {
        id: i64,
        path: PathBuf,
    },
    ImportTranscript {
        id: i64,
        path: PathBuf,
        /// Language tag of the subtitle, such as tr or en-US.
        #[arg(long)]
        language: Option<String>,
    },
    Retry {
        id: i64,
    },
    Cancel {
        id: i64,
    },
    /// Linux systemd user service management.
    #[command(subcommand)]
    Service(ServiceCommand),
}
#[derive(Subcommand)]
enum ServiceCommand {
    /// Writes ~/.config/systemd/user/tehom.service for `daemon --config`.
    Install {
        /// Also run `systemctl --user daemon-reload` and `enable --now`.
        #[arg(long)]
        enable: bool,
    },
    /// Removes the unit only if Tehom generated it.
    Uninstall {
        /// Run `systemctl --user disable --now` before removing the unit.
        #[arg(long)]
        disable: bool,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if let Command::Service(command) = &args.command {
        return manage_service(command, args.config.as_deref());
    }
    let config = args
        .config
        .as_deref()
        .map(Config::load)
        .transpose()?
        .unwrap_or_default();
    let configured_interval = config
        .automation
        .as_ref()
        .map(|a| a.check_interval_minutes)
        .unwrap_or(30);
    let automation_enabled = config.automation.as_ref().is_none_or(|a| a.enabled);
    let default_mode: AutomationMode = config
        .automation
        .as_ref()
        .and_then(|a| a.default_mode.as_ref())
        .map(Into::into)
        .unwrap_or(AutomationMode::AutoDraft);
    if matches!(args.command, Command::Daemon { .. }) {
        anyhow::ensure!(
            automation_enabled,
            "Otomasyon yapılandırmada kapalı (automation.enabled = false); daemon başlatılmadı"
        );
    }
    let data = args
        .data
        .or_else(|| config.data.map(|d| d.directory))
        .or_else(|| {
            directories::ProjectDirs::from("org", "Tehom", "Tehom")
                .map(|d| d.data_local_dir().to_path_buf())
        })
        .context("Veri klasörü belirlenemedi")?;
    if matches!(args.command, Command::Status) {
        println!(
            "{}",
            serde_json::to_string_pretty(&Automation::read_snapshot(data).await?)?
        );
        return Ok(());
    }
    if let Command::Events { video } = args.command {
        for event in Automation::read_snapshot(data)
            .await?
            .events
            .into_iter()
            .rev()
            .filter(|event| video.is_none_or(|id| event.video_id == id))
        {
            println!(
                "{} video={} {} {}{}",
                event.created_at,
                event.video_id,
                event.level,
                event.state,
                event
                    .message
                    .map(|message| format!(": {message}"))
                    .unwrap_or_default()
            );
        }
        return Ok(());
    }
    let engine = Automation::open(data).await?;
    if let Some(ai) = config.ai {
        engine.set_ai(ai.into()).await?;
    }
    if let Some(storage) = config.storage {
        engine.set_keep_downloads(storage.keep_downloads).await?;
    }
    if let Some(export) = config.export {
        if let Some(directory) = export.directory {
            engine.set_export_directory(directory).await?;
        }
        if let Some(profile) = export
            .orientation
            .as_deref()
            .and_then(tehom_core::ExportProfile::parse)
        {
            engine.set_export_profile(profile).await?;
        }
    }
    match args.command {
        Command::Status | Command::Events { .. } | Command::Service(_) => {
            unreachable!("handled before opening the writable engine")
        }
        Command::Add { url, mode } => println!(
            "{}",
            serde_json::to_string_pretty(
                &engine
                    .add_source(&url, mode.map_or(default_mode, Into::into))
                    .await?
            )?
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
        Command::ExportProfile { profile } => engine.set_export_profile(profile.into()).await?,
        Command::ImportVideo { path, mode } => println!(
            "{}",
            serde_json::to_string_pretty(
                &engine
                    .import_video(path, mode.map_or(default_mode, Into::into))
                    .await?
            )?
        ),
        Command::ReviseClip {
            id,
            start_ms,
            end_ms,
        } => println!("{}", engine.save_clip_revision(id, start_ms, end_ms).await?),
        Command::ImportTranscript { id, path, language } => {
            engine
                .import_transcript(id, path, language.as_deref())
                .await?
        }
        Command::Cleanup { dry_run } => {
            let freed = engine.cleanup_downloads(dry_run).await?;
            for (id, bytes) in &freed {
                println!("video={id} {:.1} MiB", *bytes as f64 / 1_048_576.0);
            }
            let total: u64 = freed.iter().map(|(_, bytes)| bytes).sum();
            println!(
                "{} {:.1} MiB",
                if dry_run {
                    "Boşaltılabilir:"
                } else {
                    "Boşaltıldı:"
                },
                total as f64 / 1_048_576.0
            );
        }
        Command::ExportTranscript { id, path } => engine.export_transcript(id, path).await?,
        Command::SaveOriginal { id, path } => {
            println!("{}", engine.save_original(id, path).await?.display())
        }
        Command::Retry { id } => engine.retry_video(id).await?,
        Command::Cancel { id } => engine.cancel_video(id).await?,
    }
    Ok(())
}

fn manage_service(command: &ServiceCommand, config: Option<&std::path::Path>) -> Result<()> {
    anyhow::ensure!(
        cfg!(target_os = "linux"),
        "systemd kullanıcı servisi yalnızca Linux'ta desteklenir"
    );
    let directory = service::unit_directory()?;
    match command {
        ServiceCommand::Install { enable } => {
            let config = config.context("Servis kurulumu için --config gerekli")?;
            // Validate before installing so the daemon does not fail at boot.
            let parsed = Config::load(config)?;
            anyhow::ensure!(
                parsed.automation.as_ref().is_none_or(|a| a.enabled),
                "automation.enabled = false olan yapılandırma için servis kurulmaz"
            );
            let config = config.canonicalize()?;
            let binary = std::env::current_exe()
                .context("Çalışan tehom dosyası bulunamadı")?
                .canonicalize()?;
            let text = service::unit(&binary, &config)?;
            match service::install(&directory, &text)? {
                service::Installed::Created => println!(
                    "Servis yazıldı: {}",
                    directory.join(service::UNIT_NAME).display()
                ),
                service::Installed::AlreadyPresent => println!("Servis zaten kurulu"),
            }
            if *enable {
                service::systemctl(&["daemon-reload"])?;
                service::systemctl(&["enable", "--now", service::UNIT_NAME])?;
                println!("Servis etkinleştirildi");
            } else {
                println!(
                    "Başlatmak için: systemctl --user daemon-reload && systemctl --user enable --now {}",
                    service::UNIT_NAME
                );
            }
        }
        ServiceCommand::Uninstall { disable } => {
            if *disable {
                service::systemctl(&["disable", "--now", service::UNIT_NAME])?;
            }
            match service::uninstall(&directory)? {
                service::Removed::Removed => {
                    println!("Servis dosyası silindi");
                    if *disable {
                        service::systemctl(&["daemon-reload"])?;
                    } else {
                        println!(
                            "Çalışıyorsa durdurun: systemctl --user disable --now {}",
                            service::UNIT_NAME
                        );
                    }
                }
                service::Removed::NotInstalled => println!("Kurulu servis yok"),
            }
        }
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
                mode: Some(Mode::AutoExport),
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
