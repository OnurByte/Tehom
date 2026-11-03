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
    use std::io::{BufRead, Read, Write};
    for openai in [false, true] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!(
            "http://{}{}",
            listener.local_addr().unwrap(),
            if openai { "/v1" } else { "" }
        );
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = std::io::BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert!(line.contains(if openai {
                "POST /v1/chat/completions"
            } else {
                "POST /api/chat"
            }));
            let mut length = 0;
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            assert!(length > 0 && length < 8192);
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            drop(reader);
            let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let schema = if openai {
                &request["response_format"]["json_schema"]["schema"]
            } else {
                &request["format"]
            };
            assert_eq!(schema["type"], "array");
            assert_eq!(schema["items"]["properties"]["start_ms"]["type"], "integer");
            assert_eq!(schema["items"]["properties"]["title"]["type"], "string");
            let content = r#"[{"start_ms":0,"end_ms":30000,"title":"Test","reason":"Useful"}]"#;
            let body = if openai {
                serde_json::json!({"choices":[{"message":{"content":content}}]})
            } else {
                serde_json::json!({"message":{"content":content}})
            }
            .to_string();
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
#[cfg(unix)]
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

#[tokio::test]
async fn uploaded_transcripts_are_versioned_and_invalid_uploads_preserve_previous() {
    let directory = tempfile::tempdir().unwrap();
    let engine = Automation::open(directory.path().to_path_buf())
        .await
        .unwrap();
    sqlx::query("INSERT INTO sources(id,url,channel_id,title,mode) VALUES(1,'https://youtube.com/@fixture','UC1234567890123456789012','Fixture','auto_draft')").execute(&engine.pool).await.unwrap();
    sqlx::query("INSERT INTO videos(id,source_id,external_id,title,url,state) VALUES(1,1,'abcdefghijk','Fixture','https://youtube.com/watch?v=abcdefghijk','needs_input')").execute(&engine.pool).await.unwrap();
    let input = directory.path().join("caption.srt");
    tokio::fs::write(&input, "1\n00:00:00,000 --> 00:00:25,000\nFirst version\n")
        .await
        .unwrap();
    engine.import_transcript(1, input.clone()).await.unwrap();
    tokio::fs::write(&input, "1\n00:00:00,000 --> 00:00:25,000\nSecond version\n")
        .await
        .unwrap();
    engine.import_transcript(1, input.clone()).await.unwrap();
    assert!(
        engine
            .uploaded_transcript(1)
            .await
            .unwrap()
            .unwrap()
            .contains("Second version")
    );
    let paths: Vec<String> =
        sqlx::query_scalar("SELECT path FROM transcripts WHERE video_id=1 ORDER BY id")
            .fetch_all(&engine.pool)
            .await
            .unwrap();
    assert_eq!(paths.len(), 2);
    assert!(
        tokio::fs::read_to_string(&paths[0])
            .await
            .unwrap()
            .contains("First version")
    );
    tokio::fs::write(&input, "broken subtitle").await.unwrap();
    assert!(engine.import_transcript(1, input).await.is_err());
    assert!(
        engine
            .uploaded_transcript(1)
            .await
            .unwrap()
            .unwrap()
            .contains("Second version")
    );
    assert_eq!(
        engine.snapshot().await.unwrap().videos[0].state,
        "needs_input"
    );
}

#[test]
fn single_video_urls_are_canonical_and_strict() {
    for url in [
        "https://youtu.be/abcdefghijk?si=share",
        "https://www.youtube.com/watch?v=abcdefghijk&t=10",
        "https://youtube.com/shorts/abcdefghijk",
    ] {
        assert_eq!(video_input(url).unwrap(), "abcdefghijk");
    }
    for url in [
        "http://youtube.com/watch?v=abcdefghijk",
        "https://youtube.com:443/watch?v=abcdefghijk",
        "https://user@youtube.com/watch?v=abcdefghijk",
        "https://youtube.com.evil/watch?v=abcdefghijk",
        "https://youtu.be/abcdefghijk/other",
        "https://youtube.com/watch?v=abcdefghijk&v=lmnopqrstuv",
        "https://youtube.com/watch?v=short",
    ] {
        assert!(video_input(url).is_err(), "{url}");
    }
}
#[tokio::test]
async fn single_video_is_queued_once_and_never_polled_as_channel() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().into()).await.unwrap();
    let source = engine
        .add_source(
            "https://youtu.be/abcdefghijk?si=share",
            AutomationMode::AutoDraft,
        )
        .await
        .unwrap();
    let duplicate = engine
        .add_source(
            "https://youtube.com/watch?v=abcdefghijk",
            AutomationMode::AutoExport,
        )
        .await
        .unwrap();
    assert_eq!(source.id, duplicate.id);
    assert_eq!(engine.check_now().await.unwrap(), 0);
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(snapshot.sources.len(), 1);
    assert_eq!(snapshot.videos.len(), 1);
    assert_eq!(snapshot.videos[0].state, "queued");
    assert_eq!(
        snapshot.videos[0].url,
        "https://www.youtube.com/watch?v=abcdefghijk"
    );
    assert_eq!(snapshot.sources[0].mode, AutomationMode::AutoDraft);
    engine.process_next().await.unwrap();
    assert_eq!(
        engine.snapshot().await.unwrap().videos[0].state,
        "needs_input"
    );
}

#[tokio::test]
async fn imports_local_video_without_copying_or_downloading() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=s=64x64:d=25",
            "-c:v",
            "libx264",
            "-threads",
            "1",
        ])
        .arg(&original)
        .status()
        .unwrap();
    assert!(status.success());
    let before = std::fs::read(&original).unwrap();
    let engine = Automation::open(dir.path().join("data")).await.unwrap();
    let source = engine
        .import_video(original.clone(), AutomationMode::AutoDraft)
        .await
        .unwrap();
    let duplicate = engine
        .import_video(original.clone(), AutomationMode::AutoExport)
        .await
        .unwrap();
    assert_eq!(source.id, duplicate.id);
    assert!(
        engine
            .import_video(dir.path().into(), AutomationMode::Manual)
            .await
            .is_err()
    );
    assert_eq!(engine.check_now().await.unwrap(), 0);
    engine
        .set_ai(AiConfig {
            model: "fixture".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    engine.process_next().await.unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(snapshot.videos.len(), 1);
    assert_eq!(snapshot.videos[0].state, "needs_input");
    assert!(
        snapshot.videos[0]
            .error
            .as_deref()
            .unwrap()
            .contains("transcript")
    );
    assert_eq!(
        snapshot.videos[0].local_path,
        Some(original.canonicalize().unwrap())
    );
    #[cfg(unix)]
    {
        let subtitles = dir.path().join("local.srt");
        std::fs::write(
            &subtitles,
            "1\n00:00:00,000 --> 00:00:25,000\nLocal fixture transcript\n",
        )
        .unwrap();
        engine
            .import_transcript(snapshot.videos[0].id, subtitles)
            .await
            .unwrap();
        let (endpoint, server) =
            model_fixture(r#"[{"start_ms":0,"end_ms":25000,"title":"Local draft"}]"#);
        engine
            .set_ai(AiConfig {
                endpoint,
                model: "fixture".into(),
            })
            .await
            .unwrap();
        engine
            .set_mode(source.id, AutomationMode::AutoExport)
            .await
            .unwrap();
        engine.retry_video(snapshot.videos[0].id).await.unwrap();
        engine.process_next().await.unwrap();
        server.join().unwrap();
        let complete = engine.snapshot().await.unwrap();
        assert_eq!(complete.videos[0].state, "complete");
        assert_eq!(complete.clips.len(), 1);
        assert!(complete.clips[0].output_path.as_ref().unwrap().is_file());
    }
    assert_eq!(std::fs::read(original).unwrap(), before);
}

#[tokio::test]
async fn readonly_status_works_while_engine_owns_lock_without_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().join("data")).await.unwrap();
    sqlx::raw_sql("INSERT INTO sources(id,url,channel_id,title,mode) VALUES(1,'u','UC1234567890123456789012','fixture','manual'); INSERT INTO videos(source_id,external_id,title,url,state) VALUES(1,'abcdefghijk','fixture','u','running');").execute(&engine.pool).await.unwrap();
    let snapshot = Automation::read_snapshot(dir.path().join("data"))
        .await
        .unwrap();
    assert_eq!(snapshot.videos[0].state, "running");
    assert_eq!(engine.snapshot().await.unwrap().videos[0].state, "running");
    assert!(Automation::open(dir.path().join("data")).await.is_err());
    assert!(
        Automation::read_snapshot(dir.path().join("missing"))
            .await
            .is_err()
    );
    assert!(!dir.path().join("missing").exists());
}

#[tokio::test]
async fn closing_owner_releases_lock_even_if_descriptor_was_duplicated() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().into()).await.unwrap();
    let duplicated = engine._lock.file.try_clone().unwrap();
    engine.pool.close().await;
    drop(engine);
    let reopened = Automation::open(dir.path().into()).await;
    drop(duplicated);
    assert!(reopened.is_ok());
}
