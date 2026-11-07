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
                    ..Default::default()
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
                model: "test".into(),
                ..Default::default()
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
                model: "test".into(),
                ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
    // Spec §21: Auto Export contacts only the local model, never a social service.
    let requests = engine.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert!(requests[0].starts_with("http://127.0.0.1:"), "{requests:?}");
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(
        snapshot.videos[0].state, "complete",
        "{:?}",
        snapshot.videos[0].error
    );
    assert_eq!(snapshot.clips.len(), 1);
    assert_eq!(
        snapshot.videos[0].transcript_language.as_deref(),
        Some("en")
    );
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
    assert_eq!(engine.requests.lock().unwrap().len(), 1);
}
#[test]
fn outbound_requests_are_limited_to_youtube_and_loopback() {
    for allowed in [
        "https://www.youtube.com/feeds/videos.xml?channel_id=UC1234567890123456789012",
        "https://www.youtube.com/@example",
        "http://127.0.0.1:11434/api/chat",
        "http://[::1]:8080/v1/chat/completions",
        "http://localhost:11434/api/tags",
    ] {
        assert!(outbound(allowed).is_ok(), "{allowed}");
    }
    for refused in [
        "https://www.tiktok.com/upload",
        "https://open.tiktokapis.com/v2/post/publish/video/init/",
        "https://graph.facebook.com/v19.0/me/videos",
        "https://graph.instagram.com/me/media",
        "https://www.googleapis.com/upload/youtube/v3/videos",
        "https://upload.twitter.com/1.1/media/upload.json",
        "https://api.x.com/2/tweets",
        "https://kick.com/api/v2/channels",
        "http://www.youtube.com/@plaintext",
        "https://www.youtube.com:8443/@port",
        "https://user:pass@www.youtube.com/@creds",
        "https://www.youtube.com.evil.example/@lookalike",
        "http://192.168.1.10:11434/api/chat",
    ] {
        assert!(outbound(refused).is_err(), "{refused}");
    }
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
    engine
        .import_transcript(1, input.clone(), None)
        .await
        .unwrap();
    assert_eq!(
        engine.snapshot().await.unwrap().videos[0].transcript_language,
        None
    );
    tokio::fs::write(&input, "1\n00:00:00,000 --> 00:00:25,000\nSecond version\n")
        .await
        .unwrap();
    for invalid in ["", "turkish", "../tr", "tr_TR", "TR", "tr-"] {
        assert!(
            engine
                .import_transcript(1, input.clone(), Some(invalid))
                .await
                .is_err(),
            "{invalid}"
        );
    }
    engine
        .import_transcript(1, input.clone(), Some("tr"))
        .await
        .unwrap();
    assert_eq!(
        engine.snapshot().await.unwrap().videos[0]
            .transcript_language
            .as_deref(),
        Some("tr")
    );
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
    assert!(
        engine
            .import_transcript(1, input, Some("en"))
            .await
            .is_err()
    );
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
            .import_transcript(snapshot.videos[0].id, subtitles, None)
            .await
            .unwrap();
        let (endpoint, server) =
            model_fixture(r#"[{"start_ms":0,"end_ms":25000,"title":"Local draft"}]"#);
        engine
            .set_ai(AiConfig {
                endpoint,
                model: "fixture".into(),
                ..Default::default()
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

#[tokio::test]
async fn edited_ranges_persist_as_new_drafts_without_changing_exports() {
    let (dir, engine, output) = saved_draft_fixture().await;
    let input = dir.path().join("videos/1/source.mp4");
    sqlx::query("UPDATE videos SET local_path=? WHERE id=1")
        .bind(input.to_str().unwrap())
        .execute(&engine.pool)
        .await
        .unwrap();
    std::fs::write(&output, b"existing published file").unwrap();
    sqlx::query("UPDATE clips SET output_path=? WHERE id=1")
        .bind(output.to_str().unwrap())
        .execute(&engine.pool)
        .await
        .unwrap();
    let revision = engine.save_clip_revision(1, 2000, 24000).await.unwrap();
    assert_ne!(revision, 1);
    assert_eq!(
        engine.save_clip_revision(1, 2000, 24000).await.unwrap(),
        revision
    );
    assert!(engine.save_clip_revision(1, 5000, 26000).await.is_err());
    assert!(engine.save_clip_revision(1, 0, 10000).await.is_err());
    engine.pool.close().await;
    drop(engine);
    let reopened = Automation::open(dir.path().into()).await.unwrap();
    let clips = reopened.snapshot().await.unwrap().clips;
    assert_eq!(clips.len(), 2);
    let saved = clips.iter().find(|clip| clip.id == revision).unwrap();
    assert_eq!((saved.start_ms, saved.end_ms), (2000, 24000));
    assert!(saved.output_path.is_none());
    assert_eq!(
        clips
            .iter()
            .find(|clip| clip.id == 1)
            .unwrap()
            .output_path
            .as_ref(),
        Some(&output)
    );
    assert_eq!(std::fs::read(output).unwrap(), b"existing published file");
}

#[test]
fn language_tags_and_caption_file_names() {
    for valid in ["tr", "en", "en-US", "pt-BR", "zh-Hans", "en-orig", "fil"] {
        assert!(language_tag(valid), "{valid}");
    }
    for invalid in [
        "", "e", "english", "EN", "en_US", "en-", "-en", "en--US", "../tr", "tr.vtt",
    ] {
        assert!(!language_tag(invalid), "{invalid}");
    }
    let language = |name: &str| caption_language(std::path::Path::new(name));
    assert_eq!(language("/v/source.tr.vtt").as_deref(), Some("tr"));
    assert_eq!(language("/v/source.en-US.srt").as_deref(), Some("en-US"));
    assert_eq!(language("/v/source.vtt"), None);
    assert_eq!(language("/v/other.en.vtt"), None);
}
#[tokio::test]
async fn turkish_platform_captions_are_preferred_and_recorded() {
    let directory = tempfile::tempdir().unwrap();
    let folder = tokio::fs::canonicalize(directory.path()).await.unwrap();
    let caption = |text: &str| format!("WEBVTT\n\n00:00:00.000 --> 00:00:25.000\n{text}\n");
    tokio::fs::write(folder.join("source.en.vtt"), caption("English"))
        .await
        .unwrap();
    tokio::fs::write(folder.join("source.tr.vtt"), caption("Türkçe"))
        .await
        .unwrap();
    let (text, path) = load_transcript(&folder).await.unwrap().unwrap();
    assert!(text.contains("Türkçe"));
    assert_eq!(caption_language(&path).as_deref(), Some("tr"));
}
#[tokio::test]
async fn deleted_original_and_moved_export_are_reported_not_hidden() {
    // Spec §14: "Orijinal video silindiğinde taslakların durumunu doğru gösterme."
    let (dir, engine, output) = saved_draft_fixture().await;
    let input = dir.path().join("videos/1/source.mp4");
    sqlx::query("UPDATE videos SET local_path=? WHERE id=1")
        .bind(input.to_str().unwrap())
        .execute(&engine.pool)
        .await
        .unwrap();
    std::fs::write(&output, b"published clip").unwrap();
    sqlx::query("UPDATE clips SET output_path=? WHERE id=1")
        .bind(output.to_str().unwrap())
        .execute(&engine.pool)
        .await
        .unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    assert!(snapshot.videos[0].local_available);
    assert!(snapshot.clips[0].output_available);

    std::fs::remove_file(&input).unwrap();
    std::fs::rename(&output, dir.path().join("moved.mp4")).unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    assert!(!snapshot.videos[0].local_available);
    assert!(!snapshot.clips[0].output_available);
    // Records are kept so the user can restore the file; editing is refused clearly.
    assert_eq!(snapshot.clips.len(), 1);
    assert_eq!(
        snapshot.clips[0].output_path.as_deref(),
        Some(output.as_path())
    );
    assert!(engine.save_clip_revision(1, 0, 21000).await.is_err());

    // A directory at the old path is not a usable file either.
    std::fs::create_dir(&input).unwrap();
    assert!(!engine.snapshot().await.unwrap().videos[0].local_available);
}
#[cfg(unix)]
#[tokio::test]
async fn original_is_saved_separately_without_redownload_or_overwrite() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Automation::open(dir.path().join("data")).await.unwrap();
    let calls = dir.path().join("calls");
    let downloader = dir.path().join("downloader");
    std::fs::write(
        &downloader,
        format!(
            "#!/bin/sh\necho call >> '{}'\noutput=\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '-o' ]; then shift; output=\"$1\"; fi\n  shift\ndone\nexec ffmpeg -nostdin -v error -f lavfi -i testsrc2=size=160x90:rate=10 -t 2 -c:v libx264 \"${{output%%\\%*}}mp4\"\n",
            calls.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&downloader, std::fs::Permissions::from_mode(0o700)).unwrap();
    engine.downloader = downloader;
    sqlx::query("INSERT INTO sources(url,channel_id,title,mode) VALUES ('u','UC1234567890123456789012','Channel','manual')").execute(&engine.pool).await.unwrap();
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

    let target = dir.path().join("saved");
    std::fs::create_dir(&target).unwrap();
    let saved = engine.save_original(1, target.clone()).await.unwrap();
    assert_eq!(saved, target.join("tehom-original-abcdefghijk.mp4"));
    assert!((tehom_media::duration(&saved).unwrap() - 2.0).abs() < 0.2);
    let snapshot = engine.snapshot().await.unwrap();
    assert!(snapshot.videos[0].local_available);
    assert!(
        snapshot.clips.is_empty(),
        "saving an original creates no clips"
    );

    // Saving again reuses the download and never replaces an existing file.
    let bytes = std::fs::read(&saved).unwrap();
    assert!(engine.save_original(1, target.clone()).await.is_err());
    assert_eq!(std::fs::read(&saved).unwrap(), bytes);
    let named = target.join("copy.mp4");
    assert_eq!(engine.save_original(1, named.clone()).await.unwrap(), named);
    assert_eq!(std::fs::read(&named).unwrap(), bytes);
    assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 1);
    let leftovers = std::fs::read_dir(&target)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tehom-")
        })
        .count();
    assert_eq!(leftovers, 0);
    assert!(engine.save_original(99, target).await.is_err());
}
#[tokio::test]
async fn auto_export_uses_selected_profile_and_recovery_keeps_the_started_one() {
    // Fresh export with the selected profile.
    let (_dir, engine, output) = saved_draft_fixture().await;
    engine
        .set_export_profile(ExportProfile::Original)
        .await
        .unwrap();
    assert_eq!(
        engine.snapshot().await.unwrap().export_profile,
        ExportProfile::Original
    );
    assert!(engine.process_next().await.unwrap());
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(
        snapshot.videos[0].state, "complete",
        "{:?}",
        snapshot.videos[0].error
    );
    let info = tehom_media::inspect(&output).unwrap();
    assert_eq!((info.width, info.height), (160, 90));
    let stored: String = sqlx::query_scalar("SELECT export_profile FROM clips WHERE id=1")
        .fetch_one(&engine.pool)
        .await
        .unwrap();
    assert_eq!(stored, "original");

    // A crash after publishing a horizontal export: the profile setting has
    // since changed, but recovery verifies against the profile it started with.
    let (dir, engine, output) = saved_draft_fixture().await;
    tehom_media::export_framed(
        &dir.path().join("videos/1/source.mp4"),
        &output,
        0.0,
        20.0,
        &AtomicBool::new(false),
        "expected-identity",
        tehom_media::Framing::Fill {
            width: 1920,
            height: 1080,
        },
    )
    .unwrap();
    sqlx::query("UPDATE clips SET export_profile='horizontal' WHERE id=1")
        .execute(&engine.pool)
        .await
        .unwrap();
    engine
        .set_export_profile(ExportProfile::Vertical)
        .await
        .unwrap();
    let published = std::fs::read(&output).unwrap();
    assert!(engine.process_next().await.unwrap());
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(
        snapshot.videos[0].state, "complete",
        "{:?}",
        snapshot.videos[0].error
    );
    assert_eq!(std::fs::read(&output).unwrap(), published);
}
#[tokio::test]
async fn state_changes_are_recorded_as_events_and_survive_restart() {
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
    // No model configured: the job stops for input with an explanation.
    assert!(engine.process_next().await.unwrap());
    sqlx::query("UPDATE videos SET state='running'")
        .execute(&engine.pool)
        .await
        .unwrap();
    engine.pool.close().await;
    drop(engine);
    let engine = Automation::open(dir.path().into()).await.unwrap();
    engine.cancel_video(1).await.unwrap();
    engine.retry_video(1).await.unwrap();
    let events = engine.snapshot().await.unwrap().events;
    let states: Vec<&str> = events.iter().rev().map(|e| e.state.as_str()).collect();
    assert_eq!(
        states,
        [
            "queued",
            "running",
            "needs_input",
            "running",
            "retryable",
            "cancelled",
            "queued"
        ]
    );
    let stopped = events.iter().find(|e| e.state == "needs_input").unwrap();
    assert_eq!(stopped.level, "warning");
    assert!(stopped.message.as_deref().unwrap().contains("AI model"));
    assert!(
        events
            .iter()
            .all(|e| e.video_id == 1 && e.created_at.ends_with('Z'))
    );

    // History is bounded.
    sqlx::query("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<10050) INSERT INTO events(video_id,level,state) SELECT 1,'info','queued' FROM n")
        .execute(&engine.pool)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&engine.pool)
        .await
        .unwrap();
    assert!(count <= 10_000, "{count}");
    assert_eq!(engine.snapshot().await.unwrap().events.len(), 100);
}
#[tokio::test]
async fn export_progress_is_visible_while_running_and_cleared_after() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().into()).await.unwrap();
    engine
        .set_ai(AiConfig {
            endpoint: "http://127.0.0.1:1".into(),
            model: "fixture".into(),
            ..Default::default()
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
                "testsrc2=size=640x360:rate=30",
                "-t",
                "30",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
            ])
            .arg(folder.join("source.mp4"))
            .status()
            .unwrap()
            .success()
    );
    // Saved drafts skip the model; the long export exercises progress.
    sqlx::query("INSERT INTO clips(video_id,title,start_ms,end_ms,reason) VALUES(1,'Long',0,30000,'Fixture')")
        .execute(&engine.pool)
        .await
        .unwrap();
    let worker = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.process_next().await })
    };
    let mut seen = Vec::new();
    while !worker.is_finished() {
        if let Some(progress) = engine.snapshot().await.unwrap().videos[0].progress {
            seen.push(progress);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(worker.await.unwrap().unwrap());
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(
        snapshot.videos[0].state, "complete",
        "{:?}",
        snapshot.videos[0].error
    );
    assert_eq!(snapshot.videos[0].progress, None);
    assert!(
        seen.iter().any(|p| *p > 0.0 && *p < 1.0),
        "no intermediate progress observed: {seen:?}"
    );
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
    // Progress updates do not flood the event history.
    assert!(snapshot.events.len() <= 4, "{:?}", snapshot.events);
}
#[tokio::test]
async fn transcripts_round_trip_through_editable_srt_as_new_versions() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().join("data")).await.unwrap();
    sqlx::query("INSERT INTO sources(url,channel_id,title,mode) VALUES ('u','UC1234567890123456789012','C','manual')").execute(&engine.pool).await.unwrap();
    engine
        .save_entries(
            1,
            &[Entry {
                id: "abcdefghijk".into(),
                title: "V".into(),
            }],
            false,
        )
        .await
        .unwrap();
    let edited = dir.path().join("edited.srt");
    assert!(engine.export_transcript(1, edited.clone()).await.is_err());
    let folder = dir.path().join("data/videos/1");
    std::fs::create_dir_all(&folder).unwrap();
    let platform = "WEBVTT\n\n00:00:01.500 --> 00:00:04.000\nİlk satır\n\n01:02:03.004 --> 01:02:05.000\nSon satır\n";
    std::fs::write(folder.join("source.tr.vtt"), platform).unwrap();

    engine.export_transcript(1, edited.clone()).await.unwrap();
    let srt = std::fs::read_to_string(&edited).unwrap();
    assert_eq!(
        srt,
        "1\n00:00:01,500 --> 00:00:04,000\nİlk satır\n\n2\n01:02:03,004 --> 01:02:05,000\nSon satır\n\n"
    );
    assert_eq!(
        parse_transcript(&srt).unwrap(),
        parse_transcript(platform).unwrap()
    );
    // Exporting again never overwrites the user's edit target.
    std::fs::write(&edited, srt.replace("İlk satır", "Düzeltilmiş satır")).unwrap();
    assert!(engine.export_transcript(1, edited.clone()).await.is_err());

    engine
        .import_transcript(1, edited.clone(), Some("tr"))
        .await
        .unwrap();
    assert!(
        engine
            .uploaded_transcript(1)
            .await
            .unwrap()
            .unwrap()
            .contains("Düzeltilmiş satır")
    );
    // The platform original is untouched and still available.
    assert_eq!(
        std::fs::read_to_string(folder.join("source.tr.vtt")).unwrap(),
        platform
    );
    let second = dir.path().join("second.srt");
    engine.export_transcript(1, second.clone()).await.unwrap();
    assert!(
        std::fs::read_to_string(second)
            .unwrap()
            .contains("Düzeltilmiş satır")
    );
}
#[tokio::test]
async fn downloads_are_removed_only_by_user_preference_and_never_local_files() {
    // Default: downloads are kept after Auto Export.
    let (dir, engine, output) = saved_draft_fixture().await;
    let source = dir.path().join("videos/1/source.mp4");
    assert!(engine.process_next().await.unwrap());
    assert!(source.exists() && output.exists());

    // Explicit opt-out: the source goes, subtitles and the exported clip stay.
    let (dir, engine, output) = saved_draft_fixture().await;
    let source = dir.path().join("videos/1/source.mp4");
    let captions = dir.path().join("videos/1/source.en.vtt");
    std::fs::write(&captions, "WEBVTT\n\n00:00:00.000 --> 00:00:25.000\nKept\n").unwrap();
    engine.set_keep_downloads(false).await.unwrap();
    assert!(engine.process_next().await.unwrap());
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(
        snapshot.videos[0].state, "complete",
        "{:?}",
        snapshot.videos[0].error
    );
    assert!(!source.exists());
    assert!(captions.exists() && output.exists());
    assert!(!snapshot.videos[0].local_available);
    assert!(snapshot.clips[0].output_available);

    // Manual cleanup: dry run reports, real run deletes, local imports untouched.
    let (dir, engine, _output) = saved_draft_fixture().await;
    let source = dir.path().join("videos/1/source.mp4");
    assert!(engine.process_next().await.unwrap());
    let size = std::fs::metadata(&source).unwrap().len();
    let user_file = dir.path().join("my-video.mp4");
    std::fs::copy(&source, &user_file).unwrap();
    engine
        .import_video(user_file.clone(), AutomationMode::Manual)
        .await
        .unwrap();
    sqlx::query("UPDATE videos SET state='complete' WHERE url LIKE 'local:%'")
        .execute(&engine.pool)
        .await
        .unwrap();
    assert_eq!(
        engine.cleanup_downloads(true).await.unwrap(),
        vec![(1, size)]
    );
    assert!(source.exists());
    assert_eq!(
        engine.cleanup_downloads(false).await.unwrap(),
        vec![(1, size)]
    );
    assert!(!source.exists());
    assert!(user_file.exists());
    assert!(engine.cleanup_downloads(false).await.unwrap().is_empty());
}

#[test]
fn kick_vod_urls_are_strict_and_canonical() {
    let uuid = "0123abcd-4567-89ef-0123-456789abcdef";
    for valid in [
        format!("https://kick.com/video/{uuid}"),
        format!("https://www.kick.com/video/{uuid}"),
        format!("https://kick.com/some_streamer/videos/{uuid}"),
        format!(" https://kick.com/video/{uuid}/ "),
    ] {
        assert_eq!(kick_vod_input(&valid).unwrap(), uuid, "{valid}");
    }
    for invalid in [
        format!("http://kick.com/video/{uuid}"),
        format!("https://kick.com:443/video/{uuid}"),
        format!("https://user@kick.com/video/{uuid}"),
        format!("https://kick.com.evil.example/video/{uuid}"),
        format!("https://kick.com/video/{uuid}?t=10"),
        format!("https://kick.com/video/../video/{uuid}"),
        "https://kick.com/video/not-a-uuid".to_owned(),
        format!("https://kick.com/video/{}", uuid.to_uppercase()),
        "https://kick.com/some_streamer".to_owned(),
        format!("https://kick.com/a/b/videos/{uuid}"),
    ] {
        assert!(kick_vod_input(&invalid).is_err(), "{invalid}");
    }
}
#[tokio::test]
async fn kick_vod_is_queued_once_as_experimental_and_never_polled() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().into()).await.unwrap();
    let url = "https://kick.com/streamer/videos/0123abcd-4567-89ef-0123-456789abcdef";
    let first = engine
        .add_source(url, AutomationMode::AutoDraft)
        .await
        .unwrap();
    let again = engine
        .add_source(
            "https://kick.com/video/0123abcd-4567-89ef-0123-456789abcdef",
            AutomationMode::AutoDraft,
        )
        .await
        .unwrap();
    assert_eq!(first.id, again.id);
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(snapshot.videos.len(), 1);
    let video = &snapshot.videos[0];
    assert_eq!(
        video.external_id,
        "kick:0123abcd-4567-89ef-0123-456789abcdef"
    );
    assert_eq!(
        video.url,
        "https://kick.com/video/0123abcd-4567-89ef-0123-456789abcdef"
    );
    assert_eq!(video.state, "queued");
    assert!(video.title.contains("deneysel"));
    // Single videos are never polled as channels, so no request is made.
    assert_eq!(engine.check_now().await.unwrap(), 0);
    assert!(engine.requests.lock().unwrap().is_empty());
}

#[test]
fn subtitle_language_is_read_from_conventional_file_names() {
    let language = |name: &str| subtitle_name_language(std::path::Path::new(name));
    assert_eq!(language("/d/talk.tr.srt").as_deref(), Some("tr"));
    assert_eq!(language("/d/my.talk.en-US.vtt").as_deref(), Some("en-US"));
    assert_eq!(language("/d/talk.srt"), None);
    assert_eq!(language("/d/talk.final.srt"), None);
    assert_eq!(language("/d/talk.TR.srt"), None);
}
#[tokio::test]
async fn uploaded_subtitle_language_falls_back_to_the_file_name() {
    let directory = tempfile::tempdir().unwrap();
    let engine = Automation::open(directory.path().join("data"))
        .await
        .unwrap();
    sqlx::query("INSERT INTO sources(id,url,channel_id,title,mode) VALUES(1,'u','UC1234567890123456789012','F','manual')").execute(&engine.pool).await.unwrap();
    sqlx::query("INSERT INTO videos(id,source_id,external_id,title,url,state) VALUES(1,1,'abcdefghijk','F','u','needs_input')").execute(&engine.pool).await.unwrap();
    let file = directory.path().join("talk.de.srt");
    std::fs::write(&file, "1\n00:00:00,000 --> 00:00:25,000\nHallo\n").unwrap();
    engine
        .import_transcript(1, file.clone(), None)
        .await
        .unwrap();
    let language = |engine: &Automation| {
        let engine = engine.clone();
        async move {
            engine.snapshot().await.unwrap().videos[0]
                .transcript_language
                .clone()
        }
    };
    assert_eq!(language(&engine).await.as_deref(), Some("de"));
    // An explicit tag wins over the file name.
    engine.import_transcript(1, file, Some("tr")).await.unwrap();
    assert_eq!(language(&engine).await.as_deref(), Some("tr"));
}

/// Serves one HTTP response and returns the raw request it received.
fn provider_fixture(status: &str, body: String) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let status = status.to_owned();
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut head = String::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap();
            }
            head.push_str(&line);
            if line == "\r\n" {
                break;
            }
        }
        let mut request_body = vec![0; length];
        reader.read_exact(&mut request_body).unwrap();
        let mut stream = reader.into_inner();
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        head + &String::from_utf8(request_body).unwrap()
    });
    (base, handle)
}

#[tokio::test]
async fn byok_providers_send_their_own_request_shape_and_parse_clips() {
    let clips =
        r#"{"clips":[{"start_ms":0,"end_ms":30000,"title":"Opening","reason":"Self-contained"}]}"#;
    let quoted = serde_json::to_string(clips).unwrap();
    let cases = [
        (
            AiProvider::Openai,
            "/v1/chat/completions",
            "authorization: bearer sk-test-123456",
            format!(r#"{{"choices":[{{"message":{{"content":{quoted}}}}}]}}"#),
            "json_schema",
        ),
        (
            AiProvider::Openrouter,
            "/api/v1/chat/completions",
            "authorization: bearer sk-test-123456",
            format!(r#"{{"choices":[{{"message":{{"content":{quoted}}}}}]}}"#),
            "json_schema",
        ),
        (
            AiProvider::Anthropic,
            "/v1/messages",
            "x-api-key: sk-test-123456",
            format!(
                r#"{{"content":[{{"type":"text","text":"ok"}},{{"type":"tool_use","name":"clip_candidates","input":{clips}}}]}}"#
            ),
            "tool_choice",
        ),
        (
            AiProvider::Gemini,
            "/v1beta/models/gemini-2.5-flash:generateContent",
            "x-goog-api-key: sk-test-123456",
            format!(r#"{{"candidates":[{{"content":{{"parts":[{{"text":{quoted}}}]}}}}]}}"#),
            "responseJsonSchema",
        ),
    ];
    for (provider, path, auth, response, schema_key) in cases {
        let (base, server) = provider_fixture("200 OK", response);
        let dir = tempfile::tempdir().unwrap();
        let mut engine = Automation::open(dir.path().into()).await.unwrap();
        engine.remote_base = Some(base);
        engine.test_key = Some("sk-test-123456".into());
        let model = if provider == AiProvider::Gemini {
            "gemini-2.5-flash"
        } else {
            "test-model"
        };
        let ai = AiConfig {
            model: model.into(),
            provider,
            remote_consent: true,
            ..Default::default()
        };
        engine.set_ai(ai.clone()).await.unwrap();
        let found = engine.analyze(&ai, "[0-30000] Hello\n").await.unwrap();
        let request = server.join().unwrap();
        let lower = request.to_ascii_lowercase();
        assert!(
            request.starts_with(&format!("POST {path} ")),
            "{provider:?}: {request}"
        );
        assert!(lower.contains(auth), "{provider:?} auth header");
        assert!(
            request.contains(schema_key),
            "{provider:?} structured output"
        );
        assert!(
            request.contains("untrusted transcript"),
            "{provider:?} instructions"
        );
        assert_eq!(found.len(), 1, "{provider:?}");
        assert_eq!((found[0].start_ms, found[0].end_ms), (0, 30000));
    }
}

#[tokio::test]
async fn byok_requires_consent_and_a_key_and_never_leaks_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Automation::open(dir.path().into()).await.unwrap();
    // Choosing a remote provider without consent is refused outright.
    let mut ai = AiConfig {
        model: "gpt-4o-mini".into(),
        provider: AiProvider::Openai,
        ..Default::default()
    };
    assert!(engine.set_ai(ai.clone()).await.is_err());
    assert!(engine.analyze(&ai, "[0-30000] Hi\n").await.is_err());
    // With consent but no key, nothing is sent and the job asks for the key.
    ai.remote_consent = true;
    engine.set_ai(ai.clone()).await.unwrap();
    engine.remote_base = Some("http://127.0.0.1:9".into());
    let missing = engine.ai_ready(&ai).unwrap_err().to_string();
    assert!(
        missing.starts_with("Needs input:") && missing.contains("API key"),
        "{missing}"
    );
    assert!(engine.analyze(&ai, "[0-30000] Hi\n").await.is_err());
    assert!(
        engine.requests.lock().unwrap().is_empty(),
        "no request without a key"
    );
    // A rejected key becomes a clear "Needs input" error without echoing it.
    let (base, server) = provider_fixture(
        "401 Unauthorized",
        r#"{"error":{"message":"Incorrect API key provided: sk-secr****"}}"#.into(),
    );
    engine.remote_base = Some(base);
    engine.test_key = Some("sk-secret-value-123".into());
    let error = engine
        .analyze(&ai, "[0-30000] Hi\n")
        .await
        .unwrap_err()
        .to_string();
    server.join().unwrap();
    assert!(
        error.starts_with("Needs input:") && error.contains("rejected"),
        "{error}"
    );
    assert!(!error.contains("sk-secret-value-123"));
    // The stored configuration never contains a key.
    let stored: String = sqlx::query_scalar("SELECT value FROM settings WHERE key='ai'")
        .fetch_one(&engine.pool)
        .await
        .unwrap();
    assert!(!stored.contains("sk-"), "{stored}");
}

#[test]
fn byok_requests_may_only_reach_their_own_provider() {
    assert!(
        outbound_ai(
            "https://api.openai.com/v1/chat/completions",
            AiProvider::Openai
        )
        .is_ok()
    );
    assert!(
        outbound_ai(
            "https://api.anthropic.com/v1/messages",
            AiProvider::Anthropic
        )
        .is_ok()
    );
    assert!(
        outbound_ai(
            "https://generativelanguage.googleapis.com/v1beta/models/m:generateContent",
            AiProvider::Gemini
        )
        .is_ok()
    );
    assert!(
        outbound_ai(
            "https://openrouter.ai/api/v1/chat/completions",
            AiProvider::Openrouter
        )
        .is_ok()
    );
    for (url, provider) in [
        ("https://api.anthropic.com/v1/messages", AiProvider::Openai),
        (
            "http://api.openai.com/v1/chat/completions",
            AiProvider::Openai,
        ),
        (
            "https://api.openai.com:8443/v1/chat/completions",
            AiProvider::Openai,
        ),
        ("https://user:pw@api.openai.com/v1", AiProvider::Openai),
        ("https://api.openai.com.evil.example/v1", AiProvider::Openai),
        (
            "https://www.googleapis.com/upload/youtube/v3/videos",
            AiProvider::Gemini,
        ),
        ("https://www.youtube.com/feeds", AiProvider::Openai),
        ("http://127.0.0.1:11434/api/chat", AiProvider::Local),
    ] {
        assert!(outbound_ai(url, provider).is_err(), "{url}");
    }
    // The general allowlist still refuses every provider host.
    assert!(outbound("https://api.openai.com/v1/chat/completions").is_err());
    for bad in ["../../v1/files", "/v1/files", ".hidden", "a//b", "", "a b"] {
        assert!(!model_name(bad), "{bad}");
    }
    assert!(model_name("openai/gpt-4o-mini") && model_name("claude-haiku-5-5"));
}
