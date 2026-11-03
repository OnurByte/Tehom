use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::{path::PathBuf, time::Duration};
use tehom_automation::Automation;
use tehom_core::{AiConfig, AutomationMode};

#[derive(Parser)]
#[command(name = "tehom", version, about = "Yerel video ve kanal otomasyonu")]
struct Args {
    #[arg(long, global = true)]
    data: Option<PathBuf>,
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
        #[arg(long,default_value_t=30,value_parser=clap::value_parser!(u64).range(1..=1440))]
        interval_minutes: u64,
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
    let data = args
        .data
        .or_else(|| {
            directories::ProjectDirs::from("org", "Tehom", "Tehom")
                .map(|d| d.data_local_dir().to_path_buf())
        })
        .context("Veri klasörü belirlenemedi")?;
    let engine = Automation::open(data).await?;
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
            let mut checks = tokio::time::interval(Duration::from_secs(interval_minutes * 60));
            checks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let worker = async {
                loop {
                    tokio::select! {
                        _=checks.tick()=>if let Err(error)=engine.check_now().await {eprintln!("Kaynak kontrolü: {error}");},
                        _=tokio::time::sleep(Duration::from_secs(2))=>if let Err(error)=engine.process_next().await {eprintln!("Klip işi: {error}");},
                    }
                }
            };
            tokio::pin!(worker);
            tokio::select! {
                signal=tokio::signal::ctrl_c()=>{
                    signal?;
                    for video in engine.snapshot().await?.videos {
                        if video.state=="running" {engine.cancel_video(video.id).await?;}
                    }
                },
                ()=&mut worker=>{},
            }
        }
        Command::Ai { endpoint, model } => engine.set_ai(AiConfig { endpoint, model }).await?,
        Command::Models => {
            for model in engine.local_models().await? {
                println!("{model}");
            }
        }
        Command::ExportDirectory { path } => engine.set_export_directory(path).await?,
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
