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
