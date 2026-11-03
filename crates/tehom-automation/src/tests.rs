use super::*;
#[test]
fn youtube_inputs_are_strict() {
    assert!(channel_input("UC1234567890123456789012").is_ok());
    for bad in [
        "http://127.0.0.1/@foo",
        "https://youtube.com.evil/@foo",
        "https://u:p@youtube.com/@foo",
        "https://youtube.com/watch?v=foo",
        "https://youtube.com/@foo/../../watch",
        "https://youtube.com:443/@foo",
    ] {
        assert!(channel_input(bad).is_err(), "{bad}");
    }
    assert!(channel_input("https://www.youtube.com/@example").is_ok());
}
#[test]
fn timestamps_and_empty_subtitles_are_rejected() {
    assert!(parse_transcript("WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nHello").is_ok());
    for bad in [
        "",
        "WEBVTT",
        "00:00:03,000 --> 00:00:01,000\nNo",
        "-1:00:00.000 --> 00:00:03.000\nNo",
        "00:00:01.000 --> 00:00:03.000\n",
    ] {
        assert!(parse_transcript(bad).is_err());
    }
}
#[test]
fn untrusted_candidates_are_validated_and_deduplicated() {
    let input = vec![
        Candidate {
            start_ms: 0,
            end_ms: 30_000,
            title: "A".into(),
            reason: "useful".into(),
        },
        Candidate {
            start_ms: 10_000,
            end_ms: 40_000,
            title: "B".into(),
            reason: "useful".into(),
        },
        Candidate {
            start_ms: 90_000,
            end_ms: 130_000,
            title: "C".into(),
            reason: "useful".into(),
        },
    ];
    assert_eq!(validate_candidates(input, 100_000).len(), 1);
}
#[tokio::test]
async fn store_deduplicates_recovers_and_respects_pause() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().into()).await.unwrap();
    sqlx::query("INSERT INTO sources(url,channel_id,title,mode) VALUES ('u','UC1234567890123456789012','Channel','auto_draft')").execute(&engine.pool).await.unwrap();
    let entries = vec![Entry {
        id: "abcdefghijk".into(),
        title: "Video".into(),
    }];
    assert_eq!(engine.save_entries(1, &entries, false).await.unwrap(), 1);
    assert_eq!(engine.save_entries(1, &entries, false).await.unwrap(), 0);
    engine.set_mode(1, AutomationMode::Paused).await.unwrap();
    assert!(!engine.process_next().await.unwrap());
    engine.set_mode(1, AutomationMode::AutoDraft).await.unwrap();
    assert!(engine.process_next().await.unwrap());
    assert_eq!(
        engine.snapshot().await.unwrap().videos[0].state,
        "needs_input"
    );
    sqlx::query("UPDATE videos SET state='running'")
        .execute(&engine.pool)
        .await
        .unwrap();
    engine.pool.close().await;
    drop(engine);
    let engine = Automation::open(dir.path().into()).await.unwrap();
    assert_eq!(
        engine.snapshot().await.unwrap().videos[0].state,
        "retryable"
    );
    engine.cancel_video(1).await.unwrap();
    engine.pool.close().await;
    drop(engine);
    let engine = Automation::open(dir.path().into()).await.unwrap();
    assert_eq!(
        engine.snapshot().await.unwrap().videos[0].state,
        "cancelled"
    );
}
#[test]
fn rss_video_ids_and_order() {
    let xml = br#"<feed xmlns="http://www.w3.org/2005/Atom"><title>C</title><id>channel</id><entry><id>yt:video:abcdefghijk</id><title>A</title><updated>2026-01-01T00:00:00Z</updated></entry><entry><id>yt:video:bad/path</id><title>B</title></entry></feed>"#;
    let (_, entries) = parse_feed(xml).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "abcdefghijk");
}
#[tokio::test]
async fn baseline_only_queues_newest_and_later_entries_queue() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().into()).await.unwrap();
    sqlx::query("INSERT INTO sources(url,channel_id,title,mode) VALUES ('u','UC1234567890123456789012','Channel','auto_export')").execute(&engine.pool).await.unwrap();
    let entries = vec![
        Entry {
            id: "abcdefghijk".into(),
            title: "Newest".into(),
        },
        Entry {
            id: "12345678901".into(),
            title: "Old".into(),
        },
    ];
    engine.save_entries(1, &entries, true).await.unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(
        snapshot
            .videos
            .iter()
            .find(|v| v.external_id == "abcdefghijk")
            .unwrap()
            .state,
        "queued"
    );
    assert_eq!(
        snapshot
            .videos
            .iter()
            .find(|v| v.external_id == "12345678901")
            .unwrap()
            .state,
        "discovered"
    );
    engine
        .save_entries(
            1,
            &[Entry {
                id: "ZYXWVUTSRQP".into(),
                title: "Later".into(),
            }],
            false,
        )
        .await
        .unwrap();
    assert_eq!(engine.snapshot().await.unwrap().videos[0].state, "queued");
}
#[tokio::test]
async fn local_provider_request_and_response_are_real_http() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut buffer = [0; 8192];
        let size = stream.read(&mut buffer).unwrap();
        assert!(String::from_utf8_lossy(&buffer[..size]).contains("POST /api/chat"));
        let body = r#"{"message":{"content":"[{\"start_ms\":0,\"end_ms\":30000,\"title\":\"Test\",\"reason\":\"Useful\"}]"}}"#;
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().into()).await.unwrap();
    let candidates = engine
        .analyze(
            &AiConfig {
                endpoint,
                model: "test".into(),
            },
            "[0-30000] hello",
        )
        .await
        .unwrap();
    assert_eq!(validate_candidates(candidates, 30_000).len(), 1);
    server.join().unwrap();
}
#[test]
fn local_ai_rejects_external_credentials_and_accepts_loopback() {
    for endpoint in [
        "https://example.com",
        "http://192.168.1.1",
        "http://user:password@localhost",
        "http://localhost?token=secret",
    ] {
        assert!(
            ai_endpoint(&AiConfig {
                endpoint: endpoint.into(),
                model: "test".into()
            })
            .is_err()
        );
    }
    for endpoint in [
        "http://127.0.0.1:11434",
        "http://[::1]:11434",
        "http://localhost:8080/v1",
    ] {
        assert!(
            ai_endpoint(&AiConfig {
                endpoint: endpoint.into(),
                model: "test".into()
            })
            .is_ok()
        );
    }
}
#[cfg(unix)]
#[tokio::test]
async fn media_without_subtitles_stops_for_input_without_ai_or_asr() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Automation::open(dir.path().into()).await.unwrap();
    engine.downloader = fixture_downloader(dir.path(), false);
    engine
        .set_ai(AiConfig {
            endpoint: "http://127.0.0.1:1".into(),
            model: "selected-model".into(),
        })
        .await
        .unwrap();
    sqlx::query("INSERT INTO sources(url,channel_id,title,mode) VALUES ('u','UC1234567890123456789012','Channel','auto_export')").execute(&engine.pool).await.unwrap();
    engine
        .save_entries(
            1,
            &[Entry {
                id: "abcdefghijk".into(),
                title: "No captions".into(),
            }],
            false,
        )
        .await
        .unwrap();
    let folder = dir.path().join("videos/1");
    std::fs::create_dir_all(&folder).unwrap();
    assert!(
        std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=160x90:r=1",
                "-t",
                "1",
                "-c:v",
                "libx264"
            ])
            .arg(folder.join("source.mp4"))
            .status()
            .unwrap()
            .success()
    );
    assert!(engine.process_next().await.unwrap());
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(snapshot.videos[0].state, "needs_input");
    assert!(
        snapshot.videos[0]
            .error
            .as_ref()
            .unwrap()
            .contains("no usable platform subtitles")
    );
    assert!(snapshot.clips.is_empty());
    assert!(!snapshot.export_directory.exists());
}
#[tokio::test]
async fn second_engine_cannot_reset_an_active_job() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().into()).await.unwrap();
    sqlx::query("INSERT INTO sources(url,channel_id,title,mode) VALUES ('u','UC1234567890123456789012','Channel','auto_draft')").execute(&engine.pool).await.unwrap();
    engine
        .save_entries(
            1,
            &[Entry {
                id: "abcdefghijk".into(),
                title: "Video".into(),
            }],
            false,
        )
        .await
        .unwrap();
    sqlx::query("UPDATE videos SET state='running'")
        .execute(&engine.pool)
        .await
        .unwrap();
    assert!(Automation::open(dir.path().into()).await.is_err());
    assert_eq!(engine.snapshot().await.unwrap().videos[0].state, "running");
    let clone = engine.clone();
    drop(engine);
    assert!(Automation::open(dir.path().into()).await.is_err());
    clone.pool.close().await;
    drop(clone);
    let engine = Automation::open(dir.path().into()).await.unwrap();
    assert_eq!(
        engine.snapshot().await.unwrap().videos[0].state,
        "retryable"
    );
}
#[tokio::test]
async fn synthetic_media_and_local_http_model_export_a_valid_mp4() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut buffer = [0; 8192];
        let _ = stream.read(&mut buffer).unwrap();
        let body = r#"{"message":{"content":"[{\"start_ms\":0,\"end_ms\":20000,\"title\":\"Fixture clip\",\"reason\":\"Test fixture\"}]"}}"#;
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().into()).await.unwrap();
    engine
        .set_ai(AiConfig {
            endpoint,
            model: "fixture".into(),
        })
        .await
        .unwrap();
    sqlx::query("INSERT INTO sources(url,channel_id,title,mode) VALUES ('u','UC1234567890123456789012','Channel','auto_export')").execute(&engine.pool).await.unwrap();
    engine
        .save_entries(
            1,
            &[Entry {
                id: "abcdefghijk".into(),
                title: "Fixture".into(),
            }],
            false,
        )
        .await
        .unwrap();
    let folder = dir.path().join("videos/1");
    std::fs::create_dir_all(&folder).unwrap();
    assert!(
        std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=160x90:r=1",
                "-t",
                "25",
                "-c:v",
                "libx264"
            ])
            .arg(folder.join("source.mp4"))
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(
        folder.join("source.en.vtt"),
        "WEBVTT\n\n00:00:00.000 --> 00:00:25.000\nSynthetic fixture transcript\n",
    )
    .unwrap();
    assert!(engine.process_next().await.unwrap());
    server.join().unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(
        snapshot.videos[0].state, "complete",
        "{:?}",
        snapshot.videos[0].error
    );
    assert_eq!(snapshot.clips.len(), 1);
    let output = snapshot.clips[0].output_path.as_ref().unwrap();
    assert!((tehom_media::duration(output).unwrap() - 20.0).abs() < 0.2);
    engine.retry_video(1).await.unwrap();
    assert!(engine.process_next().await.unwrap());
    let retried = engine.snapshot().await.unwrap();
    assert_eq!(retried.videos[0].state, "complete");
    assert_eq!(
        retried.clips.len(),
        1,
        "retry must reuse drafts without contacting the stopped fixture model"
    );
}
#[tokio::test]
async fn legacy_source_migration_and_pause_resume_preserve_mode_and_cancelled_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(dir.path().join("automation.sqlite3"))
        .create_if_missing(true);
    let legacy = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE sources(id INTEGER PRIMARY KEY,url TEXT NOT NULL,channel_id TEXT NOT NULL UNIQUE,title TEXT NOT NULL,mode TEXT NOT NULL,last_checked TEXT,error TEXT); INSERT INTO sources VALUES(1,'u','UC1234567890123456789012','Legacy channel','auto_export',NULL,NULL);").execute(&legacy).await.unwrap();
    legacy.close().await;
    let engine = Automation::open(dir.path().into()).await.unwrap();
    engine
        .save_entries(
            1,
            &[Entry {
                id: "abcdefghijk".into(),
                title: "Cancelled video".into(),
            }],
            false,
        )
        .await
        .unwrap();
    engine.cancel_video(1).await.unwrap();
    engine.set_mode(1, AutomationMode::Paused).await.unwrap();
    engine.set_mode(1, AutomationMode::Paused).await.unwrap();
    engine.pool.close().await;
    drop(engine);
    let engine = Automation::open(dir.path().into()).await.unwrap();
    assert_eq!(
        engine.snapshot().await.unwrap().sources[0].mode,
        AutomationMode::Paused
    );
    engine.resume_source(1).await.unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(snapshot.sources[0].mode, AutomationMode::AutoExport);
    assert_eq!(snapshot.sources[0].title, "Legacy channel");
    assert_eq!(snapshot.videos[0].state, "cancelled");
    engine.set_mode(1, AutomationMode::AutoDraft).await.unwrap();
    engine.set_mode(1, AutomationMode::Paused).await.unwrap();
    engine.resume_source(1).await.unwrap();
    assert_eq!(
        engine.snapshot().await.unwrap().sources[0].mode,
        AutomationMode::AutoDraft
    );
    assert!(engine.resume_source(999).await.is_err());
}

#[tokio::test]
async fn failed_first_export_keeps_all_drafts_and_retries_without_model() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut buffer = [0; 8192];
        let _ = stream.read(&mut buffer).unwrap();
        let body = r#"{"message":{"content":"[{\"start_ms\":0,\"end_ms\":20000,\"title\":\"Fixture clip\",\"reason\":\"Test fixture\"},{\"start_ms\":20000,\"end_ms\":40000,\"title\":\"Second clip\",\"reason\":\"Second fixture\"}]"}}"#;
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().into()).await.unwrap();
    engine
        .set_ai(AiConfig {
            endpoint,
            model: "fixture".into(),
        })
        .await
        .unwrap();
    sqlx::query("INSERT INTO sources(url,channel_id,title,mode) VALUES ('u','UC1234567890123456789012','Channel','auto_export')").execute(&engine.pool).await.unwrap();
    engine
        .save_entries(
            1,
            &[Entry {
                id: "abcdefghijk".into(),
                title: "Fixture".into(),
            }],
            false,
        )
        .await
        .unwrap();
    let folder = dir.path().join("videos/1");
    std::fs::create_dir_all(&folder).unwrap();
    assert!(
        std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=160x90:r=1",
                "-t",
                "45",
                "-c:v",
                "libx264"
            ])
            .arg(folder.join("source.mp4"))
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(
        folder.join("source.en.vtt"),
        "WEBVTT\n\n00:00:00.000 --> 00:00:45.000\nSynthetic fixture transcript\n",
    )
    .unwrap();
    let exports = dir.path().join("exports");
    std::fs::create_dir_all(&exports).unwrap();
    let conflict = exports.join("tehom-1-1.mp4");
    std::fs::write(&conflict, b"existing user file").unwrap();
    assert!(engine.process_next().await.unwrap());
    server.join().unwrap();
    let failed = engine.snapshot().await.unwrap();
    assert_eq!(failed.videos[0].state, "failed");
    assert_eq!(std::fs::read(&conflict).unwrap(), b"existing user file");
    assert_eq!(
        failed.clips.len(),
        2,
        "the entire candidate batch must survive first-render failure"
    );
    assert!(failed.clips.iter().all(|clip| clip.output_path.is_none()));
    std::fs::remove_file(conflict).unwrap();
    engine.retry_video(1).await.unwrap();
    assert!(engine.process_next().await.unwrap());
    let retried = engine.snapshot().await.unwrap();
    assert_eq!(
        retried.videos[0].state, "complete",
        "{:?}",
        retried.videos[0].error
    );
    assert_eq!(retried.clips.len(), 2);
    for clip in retried.clips {
        assert!(
            (tehom_media::duration(clip.output_path.as_ref().unwrap()).unwrap() - 20.0).abs() < 0.2
        );
    }
}
#[cfg(unix)]
fn fixture_downloader(directory: &std::path::Path, captions: bool) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = directory.join("fixture-downloader");
    let script = if captions {
        r#"#!/bin/sh
skip=
output=
while [ "$#" -gt 0 ]; do
    if [ "$1" = '--skip-download' ]; then skip=yes; fi
    if [ "$1" = '-o' ]; then shift; output="$1"; fi
    shift
done
[ "$skip" = yes ] || exit 9
target="${output%%\%*}en.vtt"
cat > "$target" <<'CAPTIONS'
WEBVTT

00:00:00.000 --> 00:00:25.000
Later platform captions
CAPTIONS
"#
    } else {
        "#!/bin/sh\nexit 0\n"
    };
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn model_fixture(candidates: &str) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let body = serde_json::json!({"message":{"content":candidates}}).to_string();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut buffer = [0; 8192];
        let _ = stream.read(&mut buffer).unwrap();
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    (endpoint, server)
}
async fn saved_draft_fixture() -> (tempfile::TempDir, Automation, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().into()).await.unwrap();
    engine
        .set_ai(AiConfig {
            endpoint: "http://127.0.0.1:1".into(),
            model: "fixture".into(),
        })
        .await
        .unwrap();
    sqlx::query("INSERT INTO sources(url,channel_id,title,mode) VALUES ('u','UC1234567890123456789012','Fixture','auto_export')").execute(&engine.pool).await.unwrap();
    engine
        .save_entries(
            1,
            &[Entry {
                id: "abcdefghijk".into(),
                title: "Fixture".into(),
            }],
            false,
        )
        .await
        .unwrap();
    let folder = dir.path().join("videos/1");
    std::fs::create_dir_all(&folder).unwrap();
    assert!(
        std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=160x90:r=1",
                "-t",
                "25",
                "-c:v",
                "libx264"
            ])
            .arg(folder.join("source.mp4"))
            .status()
            .unwrap()
            .success()
    );
    let exports = dir.path().join("exports");
    std::fs::create_dir_all(&exports).unwrap();
    let output = exports.join("tehom-1-1.mp4");
    sqlx::query("INSERT INTO clips(video_id,title,start_ms,end_ms,reason,export_identity,export_intent_path) VALUES(1,'Fixture',0,20000,'Fixture','expected-identity',?)").bind(output.to_str().unwrap()).execute(&engine.pool).await.unwrap();
    (dir, engine, output)
}
#[tokio::test]
async fn crash_after_publish_adopts_only_persisted_matching_export() {
    let (dir, engine, output) = saved_draft_fixture().await;
    tehom_media::export_automated(
        &dir.path().join("videos/1/source.mp4"),
        &output,
        0.0,
        20.0,
        &AtomicBool::new(false),
        "expected-identity",
    )
    .unwrap();
    let published = std::fs::read(&output).unwrap();
    engine.pool.close().await;
    drop(engine);
    let engine = Automation::open(dir.path().into()).await.unwrap();
    assert!(engine.process_next().await.unwrap());
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(
        snapshot.videos[0].state, "complete",
        "{:?}",
        snapshot.videos[0].error
    );
    assert_eq!(snapshot.clips[0].output_path.as_ref(), Some(&output));
    assert_eq!(std::fs::read(output).unwrap(), published);
}
#[tokio::test]
async fn unrelated_same_duration_export_is_never_adopted_or_overwritten() {
    let (dir, engine, output) = saved_draft_fixture().await;
    tehom_media::export_automated(
        &dir.path().join("videos/1/source.mp4"),
        &output,
        0.0,
        20.0,
        &AtomicBool::new(false),
        "unrelated-identity",
    )
    .unwrap();
    let unrelated = std::fs::read(&output).unwrap();
    assert!(engine.process_next().await.unwrap());
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(snapshot.videos[0].state, "failed");
    assert!(snapshot.clips[0].output_path.is_none());
    assert_eq!(std::fs::read(output).unwrap(), unrelated);
}
#[cfg(unix)]
#[tokio::test]
async fn retry_fetches_later_platform_captions_without_redownloading_media() {
    let (dir, mut engine, _) = saved_draft_fixture().await;
    sqlx::query("DELETE FROM clips")
        .execute(&engine.pool)
        .await
        .unwrap();
    engine.set_mode(1, AutomationMode::AutoDraft).await.unwrap();
    engine.downloader = fixture_downloader(dir.path(), false);
    let source = dir.path().join("videos/1/source.mp4");
    let original = std::fs::read(&source).unwrap();
    assert!(engine.process_next().await.unwrap());
    assert_eq!(
        engine.snapshot().await.unwrap().videos[0].state,
        "needs_input"
    );
    engine.downloader = fixture_downloader(dir.path(), true);
    let (endpoint, server) = model_fixture(
        r#"[{"start_ms":0,"end_ms":20000,"title":"Later captions","reason":"Fixture"}]"#,
    );
    engine
        .set_ai(AiConfig {
            endpoint,
            model: "fixture".into(),
        })
        .await
        .unwrap();
    engine.retry_video(1).await.unwrap();
    assert!(engine.process_next().await.unwrap());
    server.join().unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(
        snapshot.videos[0].state, "complete",
        "{:?}",
        snapshot.videos[0].error
    );
    assert_eq!(snapshot.clips.len(), 1);
    assert_eq!(std::fs::read(source).unwrap(), original);
    let command = engine.download_command(
        &dir.path().join("videos/1"),
        "https://www.youtube.com/watch?v=abcdefghijk",
        true,
    );
    let args = command
        .as_std()
        .get_args()
        .map(|s| s.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(args.iter().any(|s| s == "--skip-download"));
    assert!(args.iter().any(|s| s == "--ignore-config"));
    assert!(!args.iter().any(|s| s == "-f"));
}

#[tokio::test]
async fn temporary_feed_failure_keeps_source_for_automatic_retry() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let directory = tempfile::tempdir().unwrap();
    let mut engine = Automation::open(directory.path().to_path_buf())
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut connection, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 2048];
        let received = connection.read(&mut request).await.unwrap();
        assert!(received > 0);
        connection.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
    });
    engine.client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::https(format!("http://{address}")).unwrap())
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let source = engine
        .add_source("UC1234567890123456789012", AutomationMode::AutoDraft)
        .await;
    server.await.unwrap();
    let source = source.expect("a temporary RSS error must not discard the configured source");
    assert!(source.error.is_some());
    assert!(source.last_checked.is_none());
    drop(engine);
    let reopened = Automation::open(directory.path().to_path_buf())
        .await
        .unwrap();
    assert_eq!(reopened.snapshot().await.unwrap().sources.len(), 1);
    assert_eq!(reopened.snapshot().await.unwrap().videos.len(), 0);
}
