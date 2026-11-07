//! Spec §21: "Geçersiz kanal URL'si uygulamayı çökertmiyor."
use std::process::Command;

#[test]
fn invalid_channel_and_video_urls_fail_cleanly_without_persisting() {
    let data = tempfile::tempdir().unwrap();
    for url in [
        "",
        "not a url",
        "ftp://www.youtube.com/@example",
        "https://youtube.com.evil.example/@example",
        "https://user:pass@www.youtube.com/@example",
        "https://www.youtube.com/@example/../../watch",
        "https://www.youtube.com:8443/@example",
        "https://www.youtube.com/watch?v=short",
        "https://www.youtube.com/@%00nul",
        &format!("https://www.youtube.com/@{}", "a".repeat(10_000)),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_tehom"))
            .arg("--data")
            .arg(data.path())
            .args(["add", url])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{url:?}: {stderr}");
        assert!(!stderr.contains("panicked"), "{url:?}: {stderr}");
        assert!(stderr.starts_with("Error:"), "{url:?}: {stderr}");
    }
    let status = Command::new(env!("CARGO_BIN_EXE_tehom"))
        .arg("--data")
        .arg(data.path())
        .arg("status")
        .output()
        .unwrap();
    assert!(status.status.success());
    let snapshot: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(snapshot["sources"], serde_json::json!([]));
    assert_eq!(snapshot["videos"], serde_json::json!([]));
}
