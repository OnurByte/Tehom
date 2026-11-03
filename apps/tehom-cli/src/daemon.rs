use anyhow::Result;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tehom_automation::Automation;

async fn shutdown() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result?,
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}

pub async fn run(engine: &Automation, interval_minutes: u64) -> Result<()> {
    let mut checks = tokio::time::interval(Duration::from_secs(interval_minutes * 60));
    checks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let stopping = AtomicBool::new(false);
    let signal = shutdown();
    tokio::pin!(signal);
    loop {
        let step = async {
            tokio::select! {
                _ = checks.tick() => {
                    if stopping.load(Ordering::Acquire) { return; }
                    if let Err(error) = engine.check_now().await { eprintln!("Kaynak kontrolü: {error}"); }
                }
                _ = tokio::time::sleep(Duration::from_secs(2)) => {
                    if stopping.load(Ordering::Acquire) { return; }
                    if let Err(error) = engine.process_next().await { eprintln!("Klip işi: {error}"); }
                }
            }
        };
        tokio::pin!(step);
        tokio::select! {
            biased;
            result = &mut signal => {
                result?;
                stopping.store(true, Ordering::Release);
                // Recheck claims while draining; a database claim may finish after the signal.
                loop {
                    for video in engine.snapshot().await?.videos {
                        if video.state == "running" { engine.cancel_video(video.id).await?; }
                    }
                    tokio::select! {
                        () = &mut step => break,
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {},
                    }
                }
                return Ok(());
            }
            () = &mut step => {},
        }
    }
}
