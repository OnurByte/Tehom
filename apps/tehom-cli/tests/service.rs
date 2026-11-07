#![cfg(target_os = "linux")]
use std::process::Command;

fn tehom(home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tehom"));
    command
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"));
    command
}

#[test]
fn service_install_validates_config_and_preserves_existing_units() {
    let home = tempfile::tempdir().unwrap();
    let unit = home.path().join("config/systemd/user/tehom.service");

    let missing = tehom(home.path())
        .args(["service", "install"])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(!unit.exists());

    let invalid = home.path().join("invalid.toml");
    std::fs::write(&invalid, "[automation]\ncheck_interval_minutes = 0\n").unwrap();
    let rejected = tehom(home.path())
        .args(["service", "install", "--config"])
        .arg(&invalid)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(!unit.exists());

    let config = home.path().join("tehom.toml");
    std::fs::write(&config, "[data]\ndirectory = \"data\"\n").unwrap();
    for _ in 0..2 {
        let installed = tehom(home.path())
            .args(["service", "install", "--config"])
            .arg(&config)
            .output()
            .unwrap();
        assert!(
            installed.status.success(),
            "{}",
            String::from_utf8_lossy(&installed.stderr)
        );
    }
    let text = std::fs::read_to_string(&unit).unwrap();
    let binary = std::fs::canonicalize(env!("CARGO_BIN_EXE_tehom")).unwrap();
    assert!(text.contains(&format!(
        "ExecStart=\"{}\" daemon --config \"{}\"",
        binary.display(),
        config.canonicalize().unwrap().display()
    )));
    // Installation must not open or lock the data directory.
    assert!(!home.path().join("data").exists());

    let removed = tehom(home.path())
        .args(["service", "uninstall"])
        .output()
        .unwrap();
    assert!(removed.status.success());
    assert!(!unit.exists());

    std::fs::write(&unit, "[Service]\nExecStart=/usr/bin/true\n").unwrap();
    assert!(
        !tehom(home.path())
            .args(["service", "uninstall"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        !tehom(home.path())
            .args(["service", "install", "--config"])
            .arg(&config)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        std::fs::read_to_string(&unit).unwrap(),
        "[Service]\nExecStart=/usr/bin/true\n"
    );
}
