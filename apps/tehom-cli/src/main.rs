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
#[command(
    name = "tehom",
    version,
    about = "Clips from videos and channels, made on your computer"
)]
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
#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum AiChoice {
    Local,
    Openai,
    Anthropic,
    Gemini,
    Openrouter,
    /// The signed-in Claude Code CLI (your subscription).
    ClaudeCode,
    /// The signed-in Codex CLI (your subscription).
    Codex,
}
impl From<AiChoice> for tehom_core::AiProvider {
    fn from(choice: AiChoice) -> Self {
        match choice {
            AiChoice::Local => Self::Local,
            AiChoice::Openai => Self::Openai,
            AiChoice::Anthropic => Self::Anthropic,
            AiChoice::Gemini => Self::Gemini,
            AiChoice::Openrouter => Self::Openrouter,
            AiChoice::ClaudeCode => Self::ClaudeCode,
            AiChoice::Codex => Self::Codex,
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
    /// Check for a new version and install it (keeps the previous one).
    Update {
        /// Only report whether an update is available.
        #[arg(long)]
        check: bool,
        /// Install without asking.
        #[arg(long)]
        yes: bool,
        /// Reinstall the latest release even if it is not newer.
        #[arg(long)]
        force: bool,
        /// Restore the version that the last update replaced.
        #[arg(long, conflicts_with_all = ["check", "force"])]
        rollback: bool,
    },
    /// Version, folders, AI and links.
    Info,
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
    /// Choose the AI that picks clips: a local server or your own API key.
    Ai {
        #[arg(long, value_enum, default_value = "local")]
        provider: AiChoice,
        /// Local servers only.
        #[arg(long, default_value = "http://127.0.0.1:11434")]
        endpoint: String,
        #[arg(long)]
        model: String,
        /// Allow captions to be sent to the chosen remote provider.
        #[arg(long)]
        consent: bool,
    },
    /// Save an API key in the system keyring. The key is read from stdin,
    /// never from the command line: `tehom ai-key openai` then paste it.
    AiKey {
        #[arg(value_enum)]
        provider: AiChoice,
        /// Remove the saved key instead.
        #[arg(long)]
        remove: bool,
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
    if let Command::Update {
        check,
        yes,
        force,
        rollback,
    } = &args.command
    {
        return run_update(*check, *yes, *force, *rollback).await;
    }
    if let Command::AiKey { provider, remove } = &args.command {
        let provider: tehom_core::AiProvider = (*provider).into();
        anyhow::ensure!(
            provider.uses_api_key(),
            "{} does not use an API key",
            provider.label()
        );
        if *remove {
            tehom_automation::delete_api_key(provider)?;
            println!("Removed the {} key", provider.label());
        } else {
            let key = if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                rpassword::prompt_password(format!("{} API key: ", provider.label()))?
            } else {
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                line
            };
            tehom_automation::store_api_key(provider, &key)?;
            println!("Saved the {} key in the system keyring", provider.label());
        }
        return Ok(());
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
            "Automation is off in the configuration (automation.enabled = false); the daemon did not start"
        );
    }
    let data = args
        .data
        .or_else(|| config.data.map(|d| d.directory))
        .or_else(|| {
            directories::ProjectDirs::from("org", "Tehom", "Tehom")
                .map(|d| d.data_local_dir().to_path_buf())
        })
        .context("Could not find a data folder")?;
    if matches!(args.command, Command::Status) {
        println!(
            "{}",
            serde_json::to_string_pretty(&Automation::read_snapshot(data).await?)?
        );
        return Ok(());
    }
    if matches!(args.command, Command::Info) {
        use tehom_automation::update;
        let snapshot = Automation::read_snapshot(data.clone()).await.ok();
        println!("Tehom {}", update::CURRENT_VERSION);
        println!("Data folder    {}", data.display());
        if let Some(snapshot) = &snapshot {
            println!("Clip folder    {}", snapshot.export_directory.display());
            let ai = &snapshot.ai;
            let model = if ai.model.is_empty() {
                "default model"
            } else {
                &ai.model
            };
            println!("AI             {} ({model})", ai.provider.label());
            println!("Clip shape     {}", snapshot.export_profile.name());
        } else {
            println!("Clip folder    not set up yet");
        }
        if let Some(path) = &args.config {
            println!("Configuration  {}", path.display());
        }
        println!(
            "Programs       {}",
            update::binary_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        );
        println!("Website        https://github.com/{}", update::REPOSITORY);
        println!(
            "Report a problem https://github.com/{}/issues",
            update::REPOSITORY
        );
        println!("Licence        AGPL-3.0-or-later; built with GPUI Kit (https://gpui-kit.com/)");
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
        Command::Status
        | Command::Events { .. }
        | Command::Service(_)
        | Command::AiKey { .. }
        | Command::Update { .. }
        | Command::Info => {
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
        Command::Ai {
            provider,
            endpoint,
            model,
            consent,
        } => {
            engine
                .set_ai(AiConfig {
                    endpoint,
                    model,
                    provider: provider.into(),
                    remote_consent: consent,
                })
                .await?
        }
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
                if dry_run { "Can be freed:" } else { "Freed:" },
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

async fn run_update(check: bool, yes: bool, force: bool, rollback: bool) -> Result<()> {
    use tehom_automation::update;
    let bin_dir = update::binary_dir()?;
    if rollback {
        let restored = update::rollback(&bin_dir)?;
        println!(
            "Restored the previous {}. Restart Tehom to use it.",
            restored.join(" and ")
        );
        return Ok(());
    }
    let status = update::check().await?;
    if !status.available && !force {
        println!("Tehom {} is up to date.", status.current);
        return Ok(());
    }
    if status.available {
        println!(
            "Tehom {} is available (you have {}). {}",
            status.latest.tag, status.current, status.latest.url
        );
    } else {
        println!("Reinstalling Tehom {}.", status.latest.tag);
    }
    if check {
        return Ok(());
    }
    if !yes {
        anyhow::ensure!(
            std::io::IsTerminal::is_terminal(&std::io::stdin()),
            "Run `tehom update --yes` to install without a prompt"
        );
        print!(
            "Install {} into {}? [Y/n] ",
            status.latest.tag,
            bin_dir.display()
        );
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim(), "" | "y" | "Y" | "yes") {
            println!("Not installed.");
            return Ok(());
        }
    }
    update::install(&status.latest.tag, &bin_dir).await?;
    println!(
        "Updated to {}. Restart Tehom to use it. Undo with `tehom update --rollback`.",
        status.latest.tag
    );
    Ok(())
}

fn manage_service(command: &ServiceCommand, config: Option<&std::path::Path>) -> Result<()> {
    anyhow::ensure!(
        cfg!(target_os = "linux"),
        "The systemd user service is only supported on Linux"
    );
    let directory = service::unit_directory()?;
    match command {
        ServiceCommand::Install { enable } => {
            let config = config.context("Installing the service needs --config")?;
            // Validate before installing so the daemon does not fail at boot.
            let parsed = Config::load(config)?;
            anyhow::ensure!(
                parsed.automation.as_ref().is_none_or(|a| a.enabled),
                "No service is installed for a configuration with automation.enabled = false"
            );
            let config = config.canonicalize()?;
            let binary = std::env::current_exe()
                .context("Could not locate the running tehom binary")?
                .canonicalize()?;
            let text = service::unit(&binary, &config)?;
            match service::install(&directory, &text)? {
                service::Installed::Created => println!(
                    "Service written: {}",
                    directory.join(service::UNIT_NAME).display()
                ),
                service::Installed::AlreadyPresent => println!("Service already installed"),
            }
            if *enable {
                service::systemctl(&["daemon-reload"])?;
                service::systemctl(&["enable", "--now", service::UNIT_NAME])?;
                println!("Service enabled");
            } else {
                println!(
                    "To start it: systemctl --user daemon-reload && systemctl --user enable --now {}",
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
                    println!("Service file removed");
                    if *disable {
                        service::systemctl(&["daemon-reload"])?;
                    } else {
                        println!(
                            "If it is running, stop it: systemctl --user disable --now {}",
                            service::UNIT_NAME
                        );
                    }
                }
                service::Removed::NotInstalled => println!("No service installed"),
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
