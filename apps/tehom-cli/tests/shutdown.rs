#![cfg(unix)]
use std::{
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[test]
fn daemon_handles_service_stop_and_releases_data_lock() {
    let folder = std::env::temp_dir().join(format!(
        "tehom-stop-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut child = Command::new(env!("CARGO_BIN_EXE_tehom"))
        .args(["daemon", "--data"])
        .arg(&folder)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !folder.join("automation.lock").exists() {
        assert!(Instant::now() < deadline, "daemon did not initialize");
        assert!(
            child.try_wait().unwrap().is_none(),
            "daemon exited during startup"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // Wait for signal registration, independently of database initialization.
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("daemon failed to stop");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let reopened = Command::new(env!("CARGO_BIN_EXE_tehom"))
        .args(["status", "--data"])
        .arg(&folder)
        .output()
        .unwrap();
    std::fs::remove_dir_all(folder).unwrap();
    assert!(
        status.success(),
        "service signal must exit cleanly: {status}"
    );
    assert!(reopened.status.success(), "data lock must be released");
}

#[tokio::test]
async fn stop_cancels_active_download_without_restart() {
    use std::os::unix::fs::PermissionsExt;
    let folder = std::env::temp_dir().join(format!(
        "tehom-active-stop-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let engine = tehom_automation::Automation::open(folder.clone())
        .await
        .unwrap();
    engine
        .set_ai(tehom_core::AiConfig {
            endpoint: "http://127.0.0.1:11434".into(),
            model: "fixture".into(),
        })
        .await
        .unwrap();
    drop(engine);
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(folder.join("automation.sqlite3")),
    )
    .await
    .unwrap();
    sqlx::raw_sql("INSERT INTO sources(id,url,channel_id,title,mode,last_checked) VALUES(1,'https://youtube.com/channel/UCaaaaaaaaaaaaaaaaaaaaaa','UCaaaaaaaaaaaaaaaaaaaaaa','fixture','paused',CURRENT_TIMESTAMP); INSERT INTO videos(id,source_id,external_id,title,url,state) VALUES(1,1,'abcdefghijk','fixture','https://www.youtube.com/watch?v=abcdefghijk','queued');").execute(&pool).await.unwrap();
    let bin = folder.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let downloader = bin.join("yt-dlp");
    std::fs::write(
        &downloader,
        "#!/bin/sh\necho $$ > \"$TEHOM_TEST_PID\"\nexec sleep 120\n",
    )
    .unwrap();
    std::fs::set_permissions(&downloader, std::fs::Permissions::from_mode(0o700)).unwrap();
    let pid_path = folder.join("downloader.pid");
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let mut child = Command::new(env!("CARGO_BIN_EXE_tehom"))
        .args(["daemon", "--data"])
        .arg(&folder)
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("TEHOM_TEST_PID", &pid_path)
        .spawn()
        .unwrap();
    // Skip the initial discovery tick without contacting YouTube.
    tokio::time::sleep(Duration::from_millis(300)).await;
    sqlx::query("UPDATE sources SET mode='manual' WHERE id=1")
        .execute(&pool)
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while !pid_path.exists() {
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("download did not start");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let downloader_pid = std::fs::read_to_string(pid_path).unwrap();
    assert!(
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("active job did not drain");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        !Command::new("kill")
            .args(["-0", downloader_pid.trim()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success(),
        "downloader survived shutdown"
    );
    let state: String = sqlx::query_scalar("SELECT state FROM videos WHERE id=1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "cancelled");
    pool.close().await;
    let reopened = tehom_automation::Automation::open(folder.clone())
        .await
        .unwrap();
    assert!(!reopened.process_next().await.unwrap());
    assert_eq!(
        reopened.snapshot().await.unwrap().videos[0].state,
        "cancelled"
    );
    drop(reopened);
    std::fs::remove_dir_all(folder).unwrap();
}
