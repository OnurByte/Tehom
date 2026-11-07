//! Spec §16 configuration keys applied by the real binary.
use std::process::Command;

fn tehom(config: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tehom"));
    command.arg("--config").arg(config);
    command
}

#[test]
fn disabled_automation_blocks_daemon_and_default_mode_applies() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("tehom.toml");
    std::fs::write(
        &config,
        "[data]\ndirectory = \"data\"\n[automation]\nenabled = false\ndefault_mode = \"manual\"\n",
    )
    .unwrap();

    let daemon = tehom(&config).arg("daemon").output().unwrap();
    assert_eq!(daemon.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&daemon.stderr).contains("automation.enabled = false"));
    assert!(
        !folder.path().join("data").exists(),
        "daemon must not open the engine"
    );
    let service = Command::new(env!("CARGO_BIN_EXE_tehom"))
        .env("XDG_CONFIG_HOME", folder.path().join("xdg"))
        .args(["service", "install", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert_eq!(service.status.code(), Some(1));
    assert!(
        !folder
            .path()
            .join("xdg/systemd/user/tehom.service")
            .exists()
    );

    let video = folder.path().join("video.mp4");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x90:rate=10",
                "-t",
                "1",
                "-c:v",
                "libx264",
            ])
            .arg(&video)
            .status()
            .unwrap()
            .success()
    );
    let imported = tehom(&config)
        .arg("import-video")
        .arg(&video)
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let status = tehom(&config).arg("status").output().unwrap();
    let snapshot: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(snapshot["sources"][0]["mode"], "manual", "{snapshot}");
}

#[test]
fn export_orientation_is_applied_and_cli_profile_overrides() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("tehom.toml");
    std::fs::write(
        &config,
        "[data]\ndirectory = \"data\"\n[export]\norientation = \"horizontal\"\n",
    )
    .unwrap();
    let profile = |command: &mut Command| {
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let snapshot: serde_json::Value =
            serde_json::from_slice(&tehom(&config).arg("status").output().unwrap().stdout).unwrap();
        snapshot["export_profile"].as_str().unwrap().to_owned()
    };
    assert_eq!(profile(tehom(&config).arg("check")), "horizontal");
    let mut plain = Command::new(env!("CARGO_BIN_EXE_tehom"));
    plain
        .arg("--data")
        .arg(folder.path().join("data"))
        .args(["export-profile", "original"]);
    assert_eq!(profile(&mut plain), "original");
}
