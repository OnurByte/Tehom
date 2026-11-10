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
    assert_eq!(
        validate_candidates(input, 100_000, &Default::default()).len(),
        1
    );
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
                &AutomationSettings::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            validate_candidates(candidates, 30_000, &Default::default()).len(),
            1
        );
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
fn outbound_requests_are_limited_to_youtube_kick_videos_and_loopback() {
    for allowed in [
        "https://www.youtube.com/feeds/videos.xml?channel_id=UC1234567890123456789012",
        "https://www.youtube.com/@example",
        "https://kick.com/api/v2/channels/some_streamer/videos",
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
        "https://kick.com/api/v2/channels/a/b/videos",
        "https://kick.com/api/v2/channels/a/clips",
        "https://kick.com:8443/api/v2/channels/a/videos",
        "https://kick.com.evil.example/api/v2/channels/a/videos",
        "http://kick.com/api/v2/channels/a/videos",
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
fn kick_channels_are_strict_and_lowercased() {
    for (input, slug) in [
        ("https://kick.com/Some_Streamer", Some("some_streamer")),
        (" https://www.kick.com/abc/ ", Some("abc")),
        ("https://www.youtube.com/@example", None),
        ("not a url", None),
    ] {
        assert_eq!(
            kick_channel_input(input).unwrap().as_deref(),
            slug,
            "{input}"
        );
    }
    for invalid in [
        "http://kick.com/abc",
        "https://kick.com:443/abc",
        "https://u@kick.com/abc",
        "https://kick.com/abc?x=1",
        "https://kick.com/video",
        "https://kick.com/a/b",
        "https://kick.com/",
    ] {
        assert!(kick_channel_input(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn kick_video_list_skips_live_and_malformed_entries() {
    let uuid = "0123abcd-4567-89ef-0123-456789abcdef";
    let json = format!(
        r#"[{{"is_live":true,"video":{{"uuid":"{uuid}"}}}},
            {{"is_live":false,"session_title":"Finale","video":{{"uuid":"fedcba98-4567-89ef-0123-456789abcdef"}}}},
            {{"video":{{"uuid":"{uuid}"}}}},{{"video":{{"uuid":"nope"}}}}]"#
    );
    let entries = parse_kick_videos(json.as_bytes()).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].title, "Finale");
    assert_eq!(entries[1].id, format!("kick:{uuid}"));
    assert_eq!(
        video_url(&entries[1].id),
        format!("https://kick.com/video/{uuid}")
    );
    assert!(parse_kick_videos(b"{}").is_err());
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
        let found = engine
            .analyze(&ai, "[0-30000] Hello\n", &AutomationSettings::default())
            .await
            .unwrap();
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
    assert!(
        engine
            .analyze(&ai, "[0-30000] Hi\n", &AutomationSettings::default())
            .await
            .is_err()
    );
    // With consent but no key, nothing is sent and the job asks for the key.
    ai.remote_consent = true;
    engine.set_ai(ai.clone()).await.unwrap();
    engine.remote_base = Some("http://127.0.0.1:9".into());
    let missing = engine.ai_ready(&ai).unwrap_err().to_string();
    assert!(
        missing.starts_with("Needs input:") && missing.contains("API key"),
        "{missing}"
    );
    assert!(
        engine
            .analyze(&ai, "[0-30000] Hi\n", &AutomationSettings::default())
            .await
            .is_err()
    );
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
        .analyze(&ai, "[0-30000] Hi\n", &AutomationSettings::default())
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

#[cfg(unix)]
fn cli_fixture(directory: &std::path::Path, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = directory.join("agent-cli");
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[cfg(unix)]
#[tokio::test]
async fn agent_clis_run_without_tools_and_return_structured_clips() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("args");
    let stdin = dir.path().join("stdin");
    let clips = r#"{"clips":[{"start_ms":0,"end_ms":25000,"title":"A","reason":"B"}]}"#;
    // Claude Code prints one JSON result with structured_output.
    let claude = cli_fixture(
        dir.path(),
        &format!(
            "for a in \"$@\"; do printf '%s\\n' \"$a\" >> '{}'; done\ncat > '{}'\nprintf '%s' '{{\"type\":\"result\",\"is_error\":false,\"structured_output\":{clips}}}'",
            log.display(),
            stdin.display()
        ),
    );
    let mut engine = Automation::open(dir.path().join("data")).await.unwrap();
    engine.cli_program = Some(claude);
    let ai = AiConfig {
        model: "haiku".into(),
        provider: AiProvider::ClaudeCode,
        remote_consent: true,
        ..Default::default()
    };
    engine.set_ai(ai.clone()).await.unwrap();
    engine.ai_ready(&ai).unwrap();
    let found = engine
        .analyze(&ai, "[0-25000] Hello\n", &AutomationSettings::default())
        .await
        .unwrap();
    assert_eq!((found[0].start_ms, found[0].end_ms), (0, 25000));
    let args = std::fs::read_to_string(&log).unwrap();
    let args: Vec<&str> = args.lines().collect();
    let tools = args.iter().position(|a| *a == "--tools").unwrap();
    assert_eq!(args[tools + 1], "", "Claude Code must run with no tools");
    for flag in [
        "-p",
        "--no-session-persistence",
        "--strict-mcp-config",
        "--json-schema",
    ] {
        assert!(args.contains(&flag), "{flag}");
    }
    assert_eq!(
        std::fs::read_to_string(&stdin).unwrap(),
        "[0-25000] Hello\n"
    );

    // Codex writes its last message to the requested file.
    std::fs::remove_file(&log).unwrap();
    let codex = cli_fixture(
        &dir.path().join("data"),
        &format!(
            "out=\nwhile [ \"$#\" -gt 0 ]; do printf '%s\\n' \"$1\" >> '{}'; [ \"$1\" = --output-last-message ] && {{ shift; printf '%s\\n' \"$1\" >> '{}'; out=\"$1\"; }}; shift; done\ncat > /dev/null\nprintf '%s' '{clips}' > \"$out\"",
            log.display(),
            log.display()
        ),
    );
    engine.cli_program = Some(codex);
    let ai = AiConfig {
        model: String::new(),
        provider: AiProvider::Codex,
        remote_consent: true,
        ..Default::default()
    };
    engine.set_ai(ai.clone()).await.unwrap();
    let found = engine
        .analyze(&ai, "[0-25000] Hello\n", &AutomationSettings::default())
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    let args = std::fs::read_to_string(&log).unwrap();
    assert!(
        args.contains("exec\n")
            && args.contains("--sandbox\nread-only\n")
            && args.contains("--ephemeral\n")
    );
    assert!(
        !args.contains("\n-m\n"),
        "an empty model uses the user's Codex default"
    );

    // A signed-out CLI is a clear "Needs input", and consent stays required.
    let signed_out = cli_fixture(
        &dir.path().join("data"),
        "cat > /dev/null\nprintf '%s' '{\"type\":\"result\",\"is_error\":true,\"result\":\"Not logged in\"}'",
    );
    engine.cli_program = Some(signed_out);
    let ai = AiConfig {
        model: "haiku".into(),
        provider: AiProvider::ClaudeCode,
        remote_consent: true,
        ..Default::default()
    };
    let error = engine
        .analyze(&ai, "[0-25000] Hello\n", &AutomationSettings::default())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.starts_with("Needs input:") && error.contains("check its sign-in"),
        "{error}"
    );
    let no_consent = AiConfig {
        remote_consent: false,
        ..ai
    };
    assert!(engine.set_ai(no_consent).await.is_err());
}

fn cue(start_ms: u64, end_ms: u64, text: &str) -> Cue {
    Cue {
        start_ms,
        end_ms,
        text: text.into(),
    }
}

fn candidate(start_ms: u64, end_ms: u64) -> Candidate {
    Candidate {
        start_ms,
        end_ms,
        title: "T".into(),
        reason: String::new(),
    }
}

#[test]
fn clip_edges_snap_to_nearby_caption_edges_within_limits() {
    let cues = [
        cue(0, 2_500, "a"),
        cue(2_500, 5_000, "b"),
        cue(20_000, 22_500, "c"),
        cue(22_500, 60_000, "long"),
    ];
    let snapped = snap_candidates(
        vec![
            candidate(1_000, 22_000),
            candidate(3_000, 23_000),
            candidate(30_000, 55_000),
        ],
        &cues,
        100_000,
        &Default::default(),
    );
    let ranges: Vec<_> = snapped.iter().map(|c| (c.start_ms, c.end_ms)).collect();
    // Edges more than SNAP_MS from a caption edge stay where the model put them.
    assert_eq!(ranges, [(0, 22_500), (2_500, 22_500), (30_000, 55_000)]);
    // A snap that would break the 20–90 s rule or pass the end is skipped.
    let cues = [cue(0, 2_000, "a"), cue(21_000, 24_000, "b")];
    let snapped = snap_candidates(
        vec![candidate(1_500, 21_500)],
        &cues,
        100_000,
        &Default::default(),
    );
    assert_eq!((snapped[0].start_ms, snapped[0].end_ms), (2_000, 24_000));
    let snapped = snap_candidates(
        vec![candidate(1_500, 21_500)],
        &cues,
        23_000,
        &Default::default(),
    );
    assert_eq!((snapped[0].start_ms, snapped[0].end_ms), (0, 21_000));
}

#[test]
fn transcript_slices_are_clip_relative_clamped_and_never_stacked() {
    let cues = transcript_cues("[0-3000] a\n[2000-5000] b\n[6000-9000]  \n[8000-12000] c\n");
    assert_eq!(cues.len(), 4);
    assert_eq!(
        slice_cues(&cues, 1_000, 10_000),
        [
            cue(0, 1_000, "a"),
            cue(1_000, 4_000, "b"),
            cue(7_000, 9_000, "c")
        ]
    );
    assert!(slice_cues(&cues, 20_000, 30_000).is_empty());
}

#[test]
fn model_names_map_to_plain_strength_tiers() {
    for (id, strength) in [
        ("gpt-4o-mini", Strength::Fast),
        ("o3-mini", Strength::Fast),
        ("gemini-2.5-flash-lite", Strength::Fast),
        ("claude-haiku-5-5", Strength::Fast),
        ("mistralai/mistral-small", Strength::Fast),
        ("claude-opus-5", Strength::Strongest),
        ("gemini-2.5-pro", Strength::Strongest),
        ("openai/o3", Strength::Strongest),
        ("o1-preview", Strength::Strongest),
        ("gpt-5", Strength::Strongest),
        ("gpt-5-mini", Strength::Fast),
        ("gpt-4o", Strength::Balanced),
        ("claude-sonnet-4-5", Strength::Balanced),
    ] {
        assert_eq!(model_strength(id), strength, "{id}");
    }
    assert!(chat_model("gpt-4o") && !chat_model("text-embedding-3-small"));
    assert!(!chat_model("gpt-4o-realtime-preview") && !chat_model("tts-1"));
}

#[tokio::test]
async fn model_lists_are_fetched_per_provider_filtered_and_strongest_first() {
    let cases = [
        (
            AiProvider::Openai,
            "GET /v1/models ",
            "authorization: bearer sk-test-123456",
            r#"{"data":[{"id":"gpt-4o-mini"},{"id":"text-embedding-3-small"},{"id":"gpt-5"},{"id":"whisper-1"},{"id":"gpt-4o"}]}"#,
            vec!["gpt-5", "gpt-4o", "gpt-4o-mini"],
            "gpt-5",
        ),
        (
            AiProvider::Anthropic,
            "GET /v1/models?limit=1000 ",
            "x-api-key: sk-test-123456",
            r#"{"data":[{"id":"claude-haiku-5-5","display_name":"Claude Haiku 5.5"},{"id":"claude-opus-5","display_name":"Claude Opus 5"}],"has_more":false}"#,
            vec!["claude-opus-5", "claude-haiku-5-5"],
            "Claude Opus 5",
        ),
        (
            AiProvider::Gemini,
            "GET /v1beta/models?pageSize=1000 ",
            "x-goog-api-key: sk-test-123456",
            r#"{"models":[{"name":"models/gemini-2.5-flash","displayName":"Gemini 2.5 Flash","supportedGenerationMethods":["generateContent","countTokens"]},{"name":"models/text-embedding-004","displayName":"Embedding","supportedGenerationMethods":["embedContent"]},{"name":"models/gemini-2.5-pro","displayName":"Gemini 2.5 Pro","supportedGenerationMethods":["generateContent"]}]}"#,
            vec!["gemini-2.5-pro", "gemini-2.5-flash"],
            "Gemini 2.5 Pro",
        ),
        (
            AiProvider::Openrouter,
            "GET /api/v1/models ",
            "authorization: bearer sk-test-123456",
            r#"{"data":[{"id":"anthropic/claude-sonnet-4.5","name":"Anthropic: Claude Sonnet 4.5","architecture":{"output_modalities":["text"]}},{"id":"vendor/picture-maker","name":"Pictures","architecture":{"output_modalities":["image"]}}]}"#,
            vec!["anthropic/claude-sonnet-4.5"],
            "Anthropic: Claude Sonnet 4.5",
        ),
    ];
    for (provider, request_line, auth, body, ids, first_label) in cases {
        let (base, server) = provider_fixture("200 OK", body.into());
        let dir = tempfile::tempdir().unwrap();
        let mut engine = Automation::open(dir.path().into()).await.unwrap();
        engine.remote_base = Some(base);
        engine.test_key = Some("sk-test-123456".into());
        let config = AiConfig {
            provider,
            ..Default::default()
        };
        let models = engine.models(&config).await.unwrap();
        let request = server.join().unwrap();
        assert!(request.starts_with(request_line), "{provider:?}: {request}");
        assert!(request.to_ascii_lowercase().contains(auth), "{provider:?}");
        if provider == AiProvider::Anthropic {
            assert!(request.contains("anthropic-version: 2023-06-01"));
        }
        let found: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(found, ids, "{provider:?}");
        assert_eq!(models[0].label, first_label);
        assert!(
            models
                .iter()
                .all(|m| m.strength.is_some() && m.note.is_some())
        );
    }
    // A rejected key is "Needs input"; no key sends nothing.
    let (base, server) = provider_fixture("401 Unauthorized", "{}".into());
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Automation::open(dir.path().into()).await.unwrap();
    engine.remote_base = Some(base);
    engine.test_key = Some("sk-test-123456".into());
    let openai = AiConfig {
        provider: AiProvider::Openai,
        ..Default::default()
    };
    let error = engine.models(&openai).await.unwrap_err().to_string();
    server.join().unwrap();
    assert!(
        error.starts_with("Needs input:") && error.contains("rejected"),
        "{error}"
    );
    engine.test_key = None;
    engine.remote_base = None;
    // Subscription CLIs: a fixed list for Claude Code, none for Codex.
    let claude = engine
        .models(&AiConfig {
            provider: AiProvider::ClaudeCode,
            ..Default::default()
        })
        .await
        .unwrap();
    let ids: Vec<&str> = claude.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["opus", "sonnet", "haiku"]);
    assert_eq!(claude[0].strength, Some(Strength::Strongest));
    assert!(
        engine
            .models(&AiConfig {
                provider: AiProvider::Codex,
                ..Default::default()
            })
            .await
            .unwrap()
            .is_empty()
    );
    // Local servers are listed without a rating.
    let (endpoint, server) = provider_fixture(
        "200 OK",
        r#"{"models":[{"name":"qwen3:8b"},{"name":"llama3.2"}]}"#.into(),
    );
    let local = engine
        .models(&AiConfig {
            endpoint,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(server.join().unwrap().starts_with("GET /api/tags "));
    assert_eq!(local.len(), 2);
    assert!(
        local
            .iter()
            .all(|m| m.strength.is_none() && m.note.is_none())
    );
}

#[test]
fn speech_model_hosts_are_allowed_only_over_https() {
    for allowed in [
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        "https://us.aws.cdn.hf.co/xet-bridge-us/abc?x=1",
    ] {
        assert!(outbound(allowed).is_ok(), "{allowed}");
    }
    for refused in [
        "http://huggingface.co/model.bin",
        "https://huggingface.co:8443/model.bin",
        "https://huggingface.co.evil.example/model.bin",
        "https://evilhf.co/model.bin",
        "https://user:pw@huggingface.co/model.bin",
    ] {
        assert!(outbound(refused).is_err(), "{refused}");
    }
}

#[tokio::test]
async fn speech_model_download_is_validated_and_published_atomically() {
    let model = format!("lmgg{}", "x".repeat(1024 * 1024));
    let (base, server) = provider_fixture("200 OK", model.clone());
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Automation::open(dir.path().into()).await.unwrap();
    assert_eq!(engine.speech_status().await.model, None);
    engine.remote_base = Some(base);
    let path = engine.download_speech_model().await.unwrap();
    assert!(server.join().unwrap().starts_with("GET /ggml-base.bin "));
    assert_eq!(std::fs::read(&path).unwrap(), model.as_bytes());
    assert_eq!(engine.speech_status().await.model, Some(path.clone()));
    // Present already: no second download.
    engine.remote_base = Some("http://127.0.0.1:9".into());
    assert_eq!(engine.download_speech_model().await.unwrap(), path);
    // Something that is not a model is refused and leaves nothing behind.
    let (base, server) = provider_fixture("200 OK", "<html>not a model</html>".into());
    let other = tempfile::tempdir().unwrap();
    let mut engine = Automation::open(other.path().into()).await.unwrap();
    engine.remote_base = Some(base);
    assert!(engine.download_speech_model().await.is_err());
    server.join().unwrap();
    assert_eq!(
        std::fs::read_dir(other.path().join("models"))
            .unwrap()
            .count(),
        0
    );
}

#[cfg(unix)]
fn whisper_fixture(directory: &std::path::Path, log: &std::path::Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = directory.join("whisper-cli");
    let script = format!(
        r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
out=
while [ "$#" -gt 0 ]; do
    if [ "$1" = '-of' ]; then shift; out="$1"; fi
    shift
done
echo 'whisper_full_with_state: auto-detected language: de (p = 0.97)' >&2
cat > "$out.srt" <<'SRT'
1
00:00:00,000 --> 00:00:02,500
Did you know this?

2
00:00:02,500 --> 00:00:20,000
Middle part of the story

3
00:00:20,000 --> 00:00:22,500
And that is the end.

4
00:00:22,500 --> 00:00:25,000
Outro
SRT
"#,
        log.display()
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[cfg(unix)]
#[tokio::test]
async fn videos_without_captions_are_transcribed_locally_and_get_editable_captions() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Automation::open(dir.path().join("data")).await.unwrap();
    let data = dir.path().join("data").canonicalize().unwrap();
    engine.downloader = fixture_downloader(dir.path(), false);
    let args = dir.path().join("whisper-args");
    engine.speech_program = Some(whisper_fixture(dir.path(), &args));
    let (endpoint, model_server) = model_fixture(
        r#"[{"start_ms":1000,"end_ms":22000,"title":"Hook","reason":"Stands alone"}]"#,
    );
    engine
        .set_ai(AiConfig {
            endpoint,
            model: "fixture".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let style = CaptionStyle {
        color: "#FFCC00".into(),
        ..Default::default()
    };
    engine
        .set_default_caption_style(style.clone())
        .await
        .unwrap();
    assert!(
        engine
            .set_default_caption_style(CaptionStyle {
                color: "yellow".into(),
                ..Default::default()
            })
            .await
            .is_err()
    );
    sqlx::query("INSERT INTO sources(url,channel_id,title,mode) VALUES ('u','UC1234567890123456789012','Channel','auto_export')").execute(&engine.pool).await.unwrap();
    engine
        .save_entries(
            1,
            &[Entry {
                id: "abcdefghijk".into(),
                title: "Talk".into(),
            }],
            false,
        )
        .await
        .unwrap();
    let folder = data.join("videos/1");
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
                "color=c=black:s=160x90:r=2",
                "-f",
                "lavfi",
                "-i",
                "sine=f=300:r=48000",
                "-t",
                "25",
                "-c:v",
                "libx264",
                "-c:a",
                "aac"
            ])
            .arg(folder.join("source.mp4"))
            .status()
            .unwrap()
            .success()
    );
    // Without a model the video waits for the user, naming what is missing.
    assert!(engine.process_next().await.unwrap());
    let parked = engine.snapshot().await.unwrap();
    assert_eq!(parked.videos[0].state, "needs_input");
    let error = parked.videos[0].error.clone().unwrap();
    assert!(error.contains("download a speech model"), "{error}");
    assert!(!error.contains("install whisper"), "{error}");
    std::fs::create_dir_all(data.join("models")).unwrap();
    std::fs::write(data.join("models/ggml-tiny.bin"), b"fixture").unwrap();
    let status = engine.speech_status().await;
    assert!(status.program.is_some());
    assert_eq!(status.model, Some(data.join("models/ggml-tiny.bin")));

    engine.retry_video(1).await.unwrap();
    assert!(engine.process_next().await.unwrap());
    model_server.join().unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(
        snapshot.videos[0].state, "complete",
        "{:?}",
        snapshot.videos[0].error
    );
    assert_eq!(
        snapshot.videos[0].transcript_language.as_deref(),
        Some("de")
    );
    assert!(
        snapshot
            .events
            .iter()
            .any(|event| event.state == "transcribing")
    );
    let whisper = std::fs::read_to_string(&args).unwrap();
    for expected in [
        "-l\nauto\n",
        "-osrt\n",
        "--max-len\n32\n",
        "--split-on-word\n",
    ] {
        assert!(whisper.contains(expected), "{expected}: {whisper}");
    }
    // No audio extraction or whisper output is left in the video folder.
    let leftovers: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .filter_map(|entry| entry.unwrap().file_name().into_string().ok())
        .filter(|name| name.starts_with(".tehom") || name.ends_with(".srt"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    // The model's 1.0–22.0 s pick snapped to whole captions.
    let clip = snapshot.clips[0].clone();
    assert_eq!((clip.start_ms, clip.end_ms), (0, 22_500));
    assert_eq!(
        clip.captions,
        [
            cue(0, 2_500, "Did you know this?"),
            cue(2_500, 20_000, "Middle part of the story"),
            cue(20_000, 22_500, "And that is the end.")
        ]
    );
    assert_eq!(clip.caption_style, style);
    let output = clip.output_path.clone().unwrap();
    let info = tehom_media::inspect(&output).unwrap();
    assert_eq!((info.width, info.height), (1080, 1920));
    assert!((info.duration - 22.5).abs() < 0.25);

    // An untouched draft follows the transcript when its range changes.
    let moved = engine
        .save_clip_revision(clip.id, 2_500, 25_000)
        .await
        .unwrap();
    let find = |snapshot: &EngineSnapshot, id: i64| {
        snapshot.clips.iter().find(|c| c.id == id).unwrap().clone()
    };
    let revision = find(&engine.snapshot().await.unwrap(), moved);
    assert_eq!(
        revision.captions[0],
        cue(0, 17_500, "Middle part of the story")
    );
    assert_eq!(revision.captions[2], cue(20_000, 22_500, "Outro"));

    // Edits are validated, kept, and shifted rather than replaced.
    for bad in [
        vec![cue(0, 30_000, "too long")],
        vec![cue(2_000, 3_000, "b"), cue(1_000, 2_000, "a")],
        vec![cue(0, 1_000, " ")],
    ] {
        assert!(
            engine
                .save_clip_captions(clip.id, bad, CaptionStyle::default(), SoundEffect::None)
                .await
                .is_err()
        );
    }
    let edited = CaptionStyle {
        uppercase: true,
        position: tehom_core::CaptionPosition::Top,
        ..Default::default()
    };
    engine
        .save_clip_captions(
            clip.id,
            vec![cue(0, 2_000, "EDITED {hook}"), cue(5_000, 8_000, "second")],
            edited.clone(),
            SoundEffect::Pop,
        )
        .await
        .unwrap();
    let shifted = engine
        .save_clip_revision(clip.id, 1_000, 22_500)
        .await
        .unwrap();
    let revision = find(&engine.snapshot().await.unwrap(), shifted);
    assert_eq!(
        revision.captions,
        [cue(0, 1_000, "EDITED {hook}"), cue(4_000, 7_000, "second")]
    );
    assert_eq!(revision.caption_style, edited);
    assert_eq!(revision.sound_effect, SoundEffect::Pop);

    // Re-rendering replaces the clip's own export in place after validation.
    let before: String = sqlx::query_scalar("SELECT export_identity FROM clips WHERE id=?")
        .bind(clip.id)
        .fetch_one(&engine.pool)
        .await
        .unwrap();
    let rendered = engine.render_clip(clip.id).await.unwrap();
    assert_eq!(rendered, output);
    let after = tehom_media::export_identity(&output).unwrap().unwrap();
    assert_ne!(after, before);
    assert!(tehom_media::has_audio(&output).unwrap());
    // A clip never exported gets its automatic export path.
    let fresh = engine.render_clip(shifted).await.unwrap();
    assert_eq!(
        fresh.file_name().unwrap().to_str().unwrap(),
        format!("tehom-1-{shifted}.mp4")
    );
    assert!(find(&engine.snapshot().await.unwrap(), shifted).output_available);
    // A user file at the export path is never overwritten.
    std::fs::write(&output, b"user file").unwrap();
    let error = engine.render_clip(clip.id).await.unwrap_err().to_string();
    assert!(error.starts_with("Needs input:"), "{error}");
    assert_eq!(std::fs::read(&output).unwrap(), b"user file");
}

// Instagram publishing against a loopback mock; real Instagram is never contacted.
mod instagram {
    use super::*;
    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// `Bearer IGT:2:` + base64 of {"ds_user_id":"4242","sessionid":"4242%3Aabc%3A1"}.
    const AUTH: &str =
        "Bearer IGT:2:eyJkc191c2VyX2lkIjoiNDI0MiIsInNlc3Npb25pZCI6IjQyNDIlM0FhYmMlM0ExIn0=";
    /// Base64 of a throwaway RSA-1024 public key PEM for password encryption.
    const PUBLIC_KEY: &str = "LS0tLS1CRUdJTiBQVUJMSUMgS0VZLS0tLS0KTUlHZk1BMEdDU3FHU0liM0RRRUJBUVVBQTRHTkFEQ0JpUUtCZ1FDcWErTkRjSWdpK2RrWlE5aUI4KzZxTy9FZwpWazRuUm92UVRSNmx4ODBiR09zaWordng5Q1NwdUI4dFZuNThDcWltcS9vYUVVcitiRkdZdHpCcm8yQ3l6cnhiCnRyUW1mVnQ1ZktXQ0ZFcVRiS0o3aXVBME9hZ0pvbjEwdUE2ZVZJOWk4empIQ1BIamZrZkQycXBFN1RMTkdHNk0KSjZNL0Z5V2Y4VE9obmowcXNRSURBUUFCCi0tLS0tRU5EIFBVQkxJQyBLRVktLS0tLQo=";
    const USER: &str = r#"{"pk":4242,"username":"tehom","full_name":"Tehom Test"}"#;

    async fn server(login_status: u16, login_body: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/launcher/sync/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("ig-set-password-encryption-key-id", "201")
                    .insert_header("ig-set-password-encryption-pub-key", PUBLIC_KEY)
                    .set_body_string(r#"{"status":"ok"}"#),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/accounts/login/"))
            .respond_with(
                ResponseTemplate::new(login_status)
                    .insert_header("ig-set-authorization", AUTH)
                    .set_body_string(login_body),
            )
            .mount(&server)
            .await;
        server
    }
    fn logged_in() -> String {
        format!(r#"{{"logged_in_user":{USER},"status":"ok"}}"#)
    }
    /// Upload endpoints; `configure` answers the final Reel configure call.
    async fn mount_upload(server: &MockServer, status: u16, configure: &str) {
        Mock::given(path_regex(
            "^/(upload_settings|rupload_igvideo|rupload_igphoto)/",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"status":"ok"}"#))
        .mount(server)
        .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/media/configure_to_clips/"))
            .respond_with(ResponseTemplate::new(status).set_body_string(configure))
            .mount(server)
            .await;
    }
    const PUBLISHED: &str =
        r#"{"media":{"pk":3300000000000000001,"code":"DAbc123"},"status":"ok"}"#;
    async fn engine(dir: &std::path::Path, server: &MockServer) -> Automation {
        let mut engine = Automation::open(dir.into()).await.unwrap();
        engine.instagram_base = Some(server.uri());
        engine.auto_publish_spacing = Duration::ZERO;
        engine
    }
    fn mp4(path: &std::path::Path, size: &str) {
        assert!(
            std::process::Command::new("ffmpeg")
                .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
                .arg(format!("color=c=blue:s={size}:r=10"))
                .args(["-t", "2", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
                .arg(path)
                .status()
                .unwrap()
                .success()
        );
    }
    /// An exported clip of `size`; returns its id.
    async fn exported_clip(engine: &Automation, dir: &std::path::Path, size: &str) -> i64 {
        sqlx::query("INSERT OR IGNORE INTO sources(id,url,channel_id,title,mode) VALUES (1,'u','UC1234567890123456789012','Channel','manual')").execute(&engine.pool).await.unwrap();
        sqlx::query("INSERT OR IGNORE INTO videos(id,source_id,external_id,title,url,state) VALUES (1,1,'abcdefghijk','Video','u','complete')").execute(&engine.pool).await.unwrap();
        let file = dir.join(format!("clip-{size}.mp4"));
        mp4(&file, size);
        sqlx::query_scalar("INSERT INTO clips(video_id,title,start_ms,end_ms,output_path,export_identity) VALUES (1,'Clip',0,20000+(SELECT COUNT(*) FROM clips),?,?) RETURNING id")
            .bind(file.to_str().unwrap())
            .bind(format!("rev-{size}"))
            .fetch_one(&engine.pool)
            .await
            .unwrap()
    }
    async fn login(engine: &Automation) -> Account {
        match engine
            .instagram_login(" tehom ", "s3cret pass")
            .await
            .unwrap()
        {
            AccountLogin::Done(account) => account,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn login_publish_duplicate_and_remove() {
        let server = server(200, &logged_in()).await;
        mount_upload(&server, 200, PUBLISHED).await;
        let dir = tempfile::tempdir().unwrap();
        let engine = engine(dir.path(), &server).await;
        let account = login(&engine).await;
        assert_eq!(
            (account.username.as_str(), account.full_name.as_str()),
            ("tehom", "Tehom Test")
        );
        assert!(!account.needs_login);
        // Signing in again updates the same account.
        assert_eq!(login(&engine).await.id, account.id);
        let snapshot = engine.snapshot().await.unwrap();
        assert_eq!(snapshot.accounts.len(), 1);
        assert_eq!(snapshot.accounts[0].platform, Platform::Instagram);
        assert_eq!(
            engine
                .secrets
                .lock()
                .unwrap()
                .get("instagram:4242")
                .map(String::as_str),
            Some(AUTH)
        );
        let stored: String = sqlx::query_scalar("SELECT session FROM accounts")
            .fetch_one(&engine.pool)
            .await
            .unwrap();
        assert!(
            !stored.contains("IGT:2") && !stored.contains("s3cret"),
            "{stored}"
        );

        let clip = exported_clip(&engine, dir.path(), "90x160").await;
        let long = "x".repeat(2201);
        assert!(engine.publish_clip(clip, account.id, &long).await.is_err());
        let publication = engine
            .publish_clip(clip, account.id, "  Hello #tehom  ")
            .await
            .unwrap();
        assert_eq!(publication.state, "published");
        assert_eq!(
            publication.url.as_deref(),
            Some("https://www.instagram.com/reel/DAbc123/")
        );
        let requests = server.received_requests().await.unwrap();
        let cover = requests
            .iter()
            .find(|r| r.url.path().starts_with("/rupload_igphoto/"))
            .unwrap();
        assert!(cover.body.starts_with(&[0xff, 0xd8]), "cover is a JPEG");
        let configure = requests
            .iter()
            .rfind(|r| r.url.path() == "/api/v1/media/configure_to_clips/")
            .unwrap();
        assert!(String::from_utf8_lossy(&configure.body).contains("Hello+%23tehom%22"));

        let duplicate = engine
            .publish_clip(clip, account.id, "again")
            .await
            .unwrap_err()
            .to_string();
        assert!(duplicate.contains("already published"), "{duplicate}");

        let landscape = exported_clip(&engine, dir.path(), "160x90").await;
        let error = engine
            .publish_clip(landscape, account.id, "")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("vertical video"), "{error}");
        let unexported: i64 = sqlx::query_scalar("INSERT INTO clips(video_id,title,start_ms,end_ms) VALUES (1,'Draft',0,30000) RETURNING id").fetch_one(&engine.pool).await.unwrap();
        let error = engine
            .publish_clip(unexported, account.id, "")
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "Needs input: export this clip first");

        let snapshot = engine.snapshot().await.unwrap();
        let states: Vec<_> = snapshot
            .publications
            .iter()
            .map(|p| p.state.as_str())
            .collect();
        assert_eq!(states, ["failed", "published"]);

        engine.set_auto_publish(Some(account.id)).await.unwrap();
        assert_eq!(
            engine.snapshot().await.unwrap().auto_publish_account,
            Some(account.id)
        );
        // Each source can post to its own account, to none, or follow the default.
        engine
            .set_source_publish(1, Some(account.id))
            .await
            .unwrap();
        assert_eq!(
            engine.snapshot().await.unwrap().sources[0].publish_account,
            Some(account.id)
        );
        assert!(engine.set_source_publish(1, Some(999)).await.is_err());
        assert!(engine.set_source_publish(999, None).await.is_err());
        engine.set_source_publish(1, Some(0)).await.unwrap();
        engine
            .set_source_publish(1, Some(account.id))
            .await
            .unwrap();
        engine.remove_account(account.id).await.unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        assert!(snapshot.accounts.is_empty());
        assert_eq!(snapshot.auto_publish_account, None);
        assert_eq!(snapshot.sources[0].publish_account, None);
        assert!(engine.secrets.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn two_factor_sign_in() {
        let server = server(400, r#"{"message":"","two_factor_required":true,"two_factor_info":{"username":"tehom","two_factor_identifier":"TFID","sms_two_factor_on":true,"totp_two_factor_on":false,"obfuscated_phone_number":"12"},"error_type":"two_factor_required","status":"fail"}"#).await;
        Mock::given(method("POST"))
            .and(path("/api/v1/accounts/two_factor_login/"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_string(r#"{"message":"This code isn't valid","status":"fail"}"#),
            )
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/accounts/two_factor_login/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("ig-set-authorization", AUTH)
                    .set_body_string(logged_in()),
            )
            .with_priority(2)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let engine = engine(dir.path(), &server).await;
        assert!(engine.instagram_two_factor("123456").await.is_err());
        let login = engine.instagram_login("tehom", "pw").await.unwrap();
        assert!(
            matches!(&login, AccountLogin::TwoFactor { hint: Some(hint), app: false } if hint == "12"),
            "{login:?}"
        );
        let error = engine.instagram_two_factor("000000").await.unwrap_err();
        assert!(
            error.to_string().contains("code was not accepted"),
            "{error}"
        );
        // A rejected code keeps the sign-in pending.
        let account = engine.instagram_two_factor(" 123456 ").await.unwrap();
        assert_eq!(account.username, "tehom");
        assert!(
            engine
                .secrets
                .lock()
                .unwrap()
                .contains_key("instagram:4242")
        );
        assert!(engine.instagram_two_factor("123456").await.is_err());
    }

    #[tokio::test]
    async fn expired_session_marks_account() {
        let server = server(200, &logged_in()).await;
        Mock::given(path_regex("^/upload_settings/"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_string(r#"{"message":"login_required","status":"fail"}"#),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let engine = engine(dir.path(), &server).await;
        let account = login(&engine).await;
        let clip = exported_clip(&engine, dir.path(), "90x160").await;
        let error = engine.publish_clip(clip, account.id, "").await.unwrap_err();
        assert_eq!(error.to_string(), "Needs input: sign in to Instagram again");
        let snapshot = engine.snapshot().await.unwrap();
        assert!(snapshot.accounts[0].needs_login);
        assert_eq!(snapshot.publications[0].state, "failed");
        // A failed attempt does not block a later one; a missing secret also asks to sign in.
        engine.secrets.lock().unwrap().clear();
        sqlx::query("UPDATE accounts SET needs_login=0")
            .execute(&engine.pool)
            .await
            .unwrap();
        let error = engine.publish_clip(clip, account.id, "").await.unwrap_err();
        assert_eq!(error.to_string(), "Needs input: sign in to Instagram again");
        assert!(engine.snapshot().await.unwrap().accounts[0].needs_login);
    }

    #[tokio::test]
    async fn interrupted_upload_is_failed_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Automation::open(dir.path().into()).await.unwrap();
        sqlx::query("INSERT INTO publications(clip_id,account_id,revision,state) VALUES (1,1,'r','uploading')").execute(&engine.pool).await.unwrap();
        engine.pool.close().await;
        drop(engine);
        let engine = Automation::open(dir.path().into()).await.unwrap();
        let publication = &engine.snapshot().await.unwrap().publications[0];
        assert_eq!(publication.state, "failed");
        assert!(
            publication
                .error
                .as_deref()
                .unwrap()
                .starts_with("Interrupted")
        );
    }

    #[tokio::test]
    async fn auto_publish_failure_keeps_the_export() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let model = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let _ = stream.read(&mut [0; 8192]).unwrap();
            let body = r#"{"message":{"content":"[{\"start_ms\":0,\"end_ms\":20000,\"title\":\"Fixture clip\",\"reason\":\"Test fixture\"}]"}}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let server = server(200, &logged_in()).await;
        mount_upload(
            &server,
            400,
            r#"{"message":"feedback_required","feedback_message":"Blocked","status":"fail"}"#,
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let engine = engine(dir.path(), &server).await;
        let account = login(&engine).await;
        engine.set_auto_publish(Some(account.id)).await.unwrap();
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
        model.join().unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        assert_eq!(
            snapshot.videos[0].state, "complete",
            "{:?}",
            snapshot.videos[0].error
        );
        assert!(snapshot.clips[0].output_available);
        let publication = &snapshot.publications[0];
        assert_eq!(publication.state, "failed");
        assert_eq!(
            publication.error.as_deref(),
            Some("Instagram refused the upload: feedback_required: Blocked")
        );
        assert!(
            snapshot.events.iter().any(|e| e.state == "publish_failed"),
            "{:?}",
            snapshot.events
        );
    }
}

#[tokio::test]
async fn generic_feeds_and_media_must_be_public_https() {
    for allowed in [
        "https://example.com/feed.xml",
        "https://podcasts.example.org:443/rss",
        "https://93.184.216.34/video.mp4",
    ] {
        assert!(public_url(allowed).is_ok(), "{allowed}");
    }
    for refused in [
        "http://example.com/feed.xml",
        "https://user:pass@example.com/feed.xml",
        "https://example.com:8443/feed.xml",
        "https://localhost/feed.xml",
        "https://printer.local/feed.xml",
        "https://intranet/feed.xml",
        "https://127.0.0.1/feed.xml",
        "https://10.1.2.3/feed.xml",
        "https://172.16.0.1/feed.xml",
        "https://192.168.1.1/feed.xml",
        "https://169.254.169.254/latest",
        "https://100.64.0.1/feed.xml",
        "https://224.0.0.1/feed.xml",
        "https://0.0.0.0/feed.xml",
        "https://255.255.255.255/feed.xml",
        "https://[::1]/feed.xml",
        "https://[fe80::1]/feed.xml",
        "https://[fd00::1]/feed.xml",
        "https://[::ffff:127.0.0.1]/feed.xml",
        "https://[64:ff9b::a00:1]/feed.xml",
        "ftp://example.com/feed.xml",
    ] {
        assert!(public_url(refused).is_err(), "{refused}");
    }
    assert!(public_ip("2606:4700::1".parse().unwrap()));
    use reqwest::dns::Resolve;
    let local = PublicOnly.resolve("localhost".parse().unwrap()).await;
    assert!(local.is_err(), "localhost must never resolve for a feed");
}

#[test]
fn feed_entries_map_to_video_enclosures_then_links() {
    let xml = br#"<?xml version="1.0"?><rss version="2.0"><channel><title>Fixture show</title>
<item><guid>ep-3</guid><title>Video episode</title><pubDate>Wed, 03 Jan 2024 00:00:00 GMT</pubDate>
<enclosure url="https://cdn.example.com/ep3.mp4" type="video/mp4" length="1"/><link>https://example.com/ep3</link></item>
<item><guid>ep-2</guid><title>Audio episode</title><pubDate>Tue, 02 Jan 2024 00:00:00 GMT</pubDate>
<enclosure url="https://cdn.example.com/ep2.mp3" type="audio/mpeg" length="1"/><link>https://example.com/ep2</link></item>
<item><guid>ep-1</guid><title>Audio only</title><pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
<enclosure url="https://cdn.example.com/ep1.mp3" type="audio/mpeg" length="1"/></item>
<item><guid>ep-3</guid><title>Duplicate</title><link>https://example.com/dup</link></item>
<item><title>No id</title><enclosure url="https://cdn.example.com/noid.mp4" type="video/mp4" length="1"/></item>
</channel></rss>"#;
    let base = reqwest::Url::parse("https://example.com/feed.xml").unwrap();
    let (title, items) = feed_items(xml, &base).unwrap();
    assert_eq!(title, "Fixture show");
    let found: Vec<_> = items
        .iter()
        .map(|i| (i.id.as_str(), i.media.as_str()))
        .collect();
    assert_eq!(found[0], ("ep-3", "https://cdn.example.com/ep3.mp4"));
    assert_eq!(found[1], ("ep-2", "https://example.com/ep2"));
    assert!(found.iter().all(|(id, _)| *id != "ep-1"));
    assert_eq!(found.iter().filter(|(id, _)| *id == "ep-3").count(), 1);
    assert!(
        found
            .iter()
            .any(|(_, media)| *media == "https://cdn.example.com/noid.mp4")
    );
    let namespaced = feed_found(7, items);
    assert!(namespaced.iter().all(|i| i.id.starts_with("feed:7:")));
    assert!(feed_items(b"<html><body>Hello</body></html>", &base).is_err());
}

#[tokio::test]
async fn automation_settings_are_validated_persisted_and_shape_clip_rules() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().into()).await.unwrap();
    assert_eq!(
        engine.snapshot().await.unwrap().automation,
        AutomationSettings::default()
    );
    let custom = AutomationSettings {
        check_minutes: 5,
        max_clips: 2,
        min_clip_seconds: 10,
        max_clip_seconds: 30,
        keep_downloads: false,
        new_source_mode: AutomationMode::Manual,
        transcribe_locally: false,
    };
    engine.set_automation(custom.clone()).await.unwrap();
    assert!(
        engine
            .set_automation(AutomationSettings {
                max_clip_seconds: 200,
                ..custom.clone()
            })
            .await
            .is_err()
    );
    assert_eq!(engine.snapshot().await.unwrap().automation, custom);
    assert!(!engine.keep_downloads().await.unwrap());
    // The CLI's [storage] keep_downloads still overrides.
    engine.set_keep_downloads(true).await.unwrap();
    assert!(engine.automation().await.unwrap().keep_downloads);
    let prompt = clip_instructions(&custom);
    assert!(prompt.contains("at most 2 objects") && prompt.contains("10000–30000ms"));
    let candidates = (0..4)
        .map(|n| Candidate {
            start_ms: n * 40_000,
            end_ms: n * 40_000 + if n == 0 { 35_000 } else { 15_000 },
            title: "T".into(),
            reason: String::new(),
        })
        .collect();
    let valid = validate_candidates(candidates, 1_000_000, &custom);
    assert_eq!(valid.len(), 2, "35 s is too long and only two are kept");
    assert!(valid.iter().all(|c| c.start_ms > 0));
}

/// Serves `routes` (path, status line, content type, body) on loopback for
/// as many requests as arrive.
#[cfg(unix)]
fn route_fixture(routes: Vec<(String, String, String, Vec<u8>)>) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&buffer[..n]),
                }
            }
            let text = String::from_utf8_lossy(&request);
            let path = text.split_whitespace().nth(1).unwrap_or("").to_owned();
            let (status, kind, body) = routes
                .iter()
                .find(|(route, ..)| *route == path)
                .map(|(_, s, k, b)| (s.clone(), k.clone(), b.clone()))
                .unwrap_or(("404 Not Found".into(), "text/plain".into(), Vec::new()));
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    base
}

#[cfg(unix)]
#[tokio::test]
async fn generic_feed_is_watched_downloaded_directly_and_clipped() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Automation::open(dir.path().join("data")).await.unwrap();
    let data = dir.path().join("data").canonicalize().unwrap();
    engine.feed_loopback = true;
    // yt-dlp must not be used for a direct enclosure.
    engine.downloader = dir.path().join("missing-yt-dlp");
    let args = dir.path().join("whisper-args");
    engine.speech_program = Some(whisper_fixture(dir.path(), &args));
    std::fs::create_dir_all(data.join("models")).unwrap();
    std::fs::write(data.join("models/ggml-tiny.bin"), b"fixture").unwrap();
    let video = dir.path().join("talk.mp4");
    assert!(
        std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=160x90:r=2",
                "-f",
                "lavfi",
                "-i",
                "sine=f=300:r=48000",
                "-t",
                "25",
                "-c:v",
                "libx264",
                "-c:a",
                "aac",
            ])
            .arg(&video)
            .status()
            .unwrap()
            .success()
    );
    let feed = |base: &str| {
        format!(
            r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Fixture show</title>
<item><guid>new</guid><title>Newest talk</title><pubDate>Tue, 02 Jan 2024 00:00:00 GMT</pubDate><enclosure url="{base}/talk.mp4" type="video/mp4" length="1"/></item>
<item><guid>old</guid><title>Older talk</title><pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate><enclosure url="{base}/old.mp4" type="video/mp4" length="1"/></item>
<item><guid>audio</guid><title>Audio only</title><enclosure url="{base}/audio.mp3" type="audio/mpeg" length="1"/></item>
</channel></rss>"#
        )
    };
    // Routes are fixed before the port is known, so the feed is served from
    // a second fixture that points at the first.
    let media = route_fixture(vec![(
        "/talk.mp4".into(),
        "200 OK".into(),
        "video/mp4".into(),
        std::fs::read(&video).unwrap(),
    )]);
    let server = route_fixture(vec![
        (
            "/feed.xml".into(),
            "200 OK".into(),
            "application/rss+xml".into(),
            feed(&media).into_bytes(),
        ),
        (
            "/moved".into(),
            "302 Found\r\nLocation: /feed.xml".into(),
            "text/plain".into(),
            Vec::new(),
        ),
        (
            "/evil".into(),
            "302 Found\r\nLocation: https://10.0.0.1/feed.xml".into(),
            "text/plain".into(),
            Vec::new(),
        ),
        (
            "/page".into(),
            "200 OK".into(),
            "text/html".into(),
            b"<html><body>Hi</body></html>".to_vec(),
        ),
    ]);
    // A redirect to a private address and a plain web page are refused.
    assert!(
        engine
            .add_source(&format!("{server}/evil"), AutomationMode::AutoDraft)
            .await
            .is_err()
    );
    assert!(
        engine
            .add_source(&format!("{server}/page"), AutomationMode::AutoDraft)
            .await
            .is_err()
    );
    assert!(engine.snapshot().await.unwrap().sources.is_empty());
    let (endpoint, model_server) = model_fixture(
        r#"[{"start_ms":1000,"end_ms":22000,"title":"Hook","reason":"Stands alone"}]"#,
    );
    engine
        .set_ai(AiConfig {
            endpoint,
            model: "fixture".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    engine
        .set_automation(AutomationSettings {
            transcribe_locally: false,
            ..Default::default()
        })
        .await
        .unwrap();
    let source = engine
        .add_source(&format!("{server}/moved"), AutomationMode::AutoDraft)
        .await
        .unwrap();
    assert_eq!(source.title, "Fixture show");
    assert!(source.channel_id.starts_with("feed:"));
    let snapshot = engine.snapshot().await.unwrap();
    let states: Vec<_> = snapshot
        .videos
        .iter()
        .map(|v| (v.title.as_str(), v.state.as_str()))
        .collect();
    assert_eq!(
        states.len(),
        2,
        "the audio-only episode is skipped: {states:?}"
    );
    assert!(states.contains(&("Newest talk", "queued")));
    assert!(states.contains(&("Older talk", "discovered")));
    let id = snapshot
        .videos
        .iter()
        .find(|v| v.title == "Newest talk")
        .unwrap()
        .id;
    assert!(
        snapshot
            .videos
            .iter()
            .all(|v| v.external_id.starts_with("feed:"))
    );

    // Transcription off: the video is downloaded, then waits for captions.
    assert!(engine.process_next().await.unwrap());
    let parked = engine.snapshot().await.unwrap();
    let video = parked.videos.iter().find(|v| v.id == id).unwrap();
    assert_eq!(video.state, "needs_input", "{:?}", video.error);
    assert!(video.error.as_ref().unwrap().contains("turned off"));
    assert!(data.join(format!("videos/{id}/source.mp4")).is_file());

    engine
        .set_automation(AutomationSettings::default())
        .await
        .unwrap();
    engine.retry_video(id).await.unwrap();
    assert!(engine.process_next().await.unwrap());
    model_server.join().unwrap();
    let done = engine.snapshot().await.unwrap();
    let video = done.videos.iter().find(|v| v.id == id).unwrap();
    assert_eq!(video.state, "complete", "{:?}", video.error);
    assert_eq!(done.clips.len(), 1);
    // Later checks find nothing new and touch only loopback.
    assert_eq!(engine.check_now().await.unwrap(), 0);
    let requests = engine.requests.lock().unwrap().clone();
    assert!(
        requests.iter().all(|r| r.starts_with("http://127.0.0.1:")),
        "{requests:?}"
    );
    // Production rules: a non-HTTPS feed gets today's YouTube error.
    engine.feed_loopback = false;
    let error = engine
        .add_source("http://example.com/feed.xml", AutomationMode::AutoDraft)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("YouTube"), "{error}");
    let error = engine
        .add_source(&format!("{server}/feed.xml"), AutomationMode::AutoDraft)
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Credentials and ports are not allowed");
}

#[test]
fn speech_program_is_found_in_the_data_folder_before_path() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join(format!("whisper-cli{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&program, b"fixture").unwrap();
    // Test binaries have no whisper-cli beside them.
    assert_eq!(speech_program(dir.path()), Some(program));
}

#[tokio::test]
async fn appearance_and_sidebar_width_persist() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Automation::open(dir.path().into()).await.unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(snapshot.appearance, Appearance::System);
    assert_eq!(snapshot.sidebar_width, tehom_core::SIDEBAR_WIDTH);
    engine.set_appearance(Appearance::Dark).await.unwrap();
    engine.set_accent(tehom_core::Accent::Pink).await.unwrap();
    engine.set_sidebar_width(360).await.unwrap();
    assert!(engine.set_sidebar_width(1000).await.is_err());
    engine.pool.close().await;
    drop(engine);
    let snapshot = Automation::read_snapshot(dir.path().into()).await.unwrap();
    assert_eq!(snapshot.appearance, Appearance::Dark);
    assert_eq!(snapshot.accent, tehom_core::Accent::Pink);
    assert_eq!(snapshot.sidebar_width, 360);
}

#[tokio::test]
async fn gemini_skips_thought_parts_and_reports_empty_selection_stage() {
    for (response, expected) in [
        (
            r#"{"candidates":[{"finishReason":"STOP","content":{"parts":[{"thought":true,"text":"private reasoning"},{"text":"{\"clips\":[]}" }]}}]}"#,
            None,
        ),
        (
            r#"{"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":""}]}}]}"#,
            Some("Gemini clip selection is empty"),
        ),
        (
            r#"{"candidates":[{"finishReason":"MAX_TOKENS","content":{"parts":[{"text":"{"}]}}]}"#,
            Some("Gemini generation did not finish"),
        ),
        (
            r#"{"candidates":[]}"#,
            Some("Gemini returned no candidates"),
        ),
    ] {
        let (base, server) = provider_fixture("200 OK", response.into());
        let dir = tempfile::tempdir().unwrap();
        let mut engine = Automation::open(dir.path().into()).await.unwrap();
        engine.remote_base = Some(base);
        engine.test_key = Some("test-key".into());
        let ai = AiConfig {
            provider: AiProvider::Gemini,
            model: "gemini-2.5-flash".into(),
            remote_consent: true,
            ..Default::default()
        };
        let result = engine
            .analyze(&ai, "[0-30000] Hello", &AutomationSettings::default())
            .await;
        let request = server.join().unwrap();
        assert!(
            request.contains("object containing"),
            "remote prompt must match schema"
        );
        match expected {
            None => assert!(result.unwrap().is_empty()),
            Some(expected) => assert!(format!("{:#}", result.unwrap_err()).contains(expected)),
        }
    }
}

#[tokio::test]
async fn ai_retries_rate_limits_but_not_auth_or_invalid_json() {
    use std::sync::atomic::AtomicUsize;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    for (status, expected_calls, succeeds) in [
        (429, 2, true),
        (503, 2, true),
        (401, 1, false),
        (200, 1, false),
    ] {
        let server = MockServer::start().await;
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        Mock::given(method("POST"))
            .respond_with(move |_: &wiremock::Request| {
                if count.fetch_add(1, Ordering::SeqCst) == 0 {
                    ResponseTemplate::new(status)
                        .insert_header("Retry-After", "0")
                        .set_body_string("private-provider-error")
                } else {
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({
                        "candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"{\"clips\":[]}"}]}}]
                    }))
                }
            }).mount(&server).await;
        let dir = tempfile::tempdir().unwrap();
        let mut engine = Automation::open(dir.path().into()).await.unwrap();
        engine.remote_base = Some(server.uri());
        engine.test_key = Some("test-key".into());
        let ai = AiConfig {
            provider: AiProvider::Gemini,
            model: "gemini-flash".into(),
            remote_consent: true,
            ..Default::default()
        };
        let result = engine
            .analyze(&ai, "private-transcript", &AutomationSettings::default())
            .await;
        assert_eq!(result.is_ok(), succeeds, "status {status}: {result:?}");
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
        if let Err(error) = result {
            assert!(!format!("{error:#}").contains("private-provider-error"));
        }
    }
}

#[tokio::test]
async fn ai_rate_limit_retry_budget_is_finite() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .expect(3)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Automation::open(dir.path().into()).await.unwrap();
    engine.remote_base = Some(server.uri());
    engine.test_key = Some("test-key".into());
    let ai = AiConfig {
        provider: AiProvider::Gemini,
        model: "gemini-flash".into(),
        remote_consent: true,
        ..Default::default()
    };
    let error = engine
        .analyze(&ai, "private-transcript", &AutomationSettings::default())
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("429"));
}

#[tokio::test]
async fn ai_failure_after_first_chunk_resumes_without_reanalysis() {
    use std::sync::atomic::AtomicUsize;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("actual.mp4");
    assert!(
        std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x90:rate=2",
                "-t",
                "60",
                "-c:v",
                "libx264",
                "-threads",
                "1"
            ])
            .arg(&video)
            .status()
            .unwrap()
            .success()
    );
    let original = std::fs::read(&video).unwrap();
    let data = dir.path().join("data");
    let mut engine = Automation::open(data.clone()).await.unwrap();
    engine
        .import_video(video.clone(), AutomationMode::AutoDraft)
        .await
        .unwrap();
    let transcript = dir.path().join("captions.srt");
    std::fs::write(
        &transcript,
        format!(
            "1\n00:00:00,000 --> 00:00:25,000\n{}\n\n2\n00:00:30,000 --> 00:00:55,000\n{}\n",
            "first ".repeat(2300),
            "second ".repeat(2000)
        ),
    )
    .unwrap();
    engine.import_transcript(1, transcript, None).await.unwrap();
    let server = MockServer::start().await;
    let first_calls = Arc::new(AtomicUsize::new(0));
    let second_calls = Arc::new(AtomicUsize::new(0));
    let first = first_calls.clone();
    let second = second_calls.clone();
    Mock::given(method("POST"))
        .respond_with(move |request: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let text = body["contents"][0]["parts"][0]["text"].as_str().unwrap();
            let (start, end) = if text.contains("first") {
                first.fetch_add(1, Ordering::SeqCst);
                (0, 25000)
            } else {
                if second.fetch_add(1, Ordering::SeqCst) == 0 {
                    return ResponseTemplate::new(200).set_body_json(serde_json::json!({
                        "candidates":[{"finishReason":"STOP","content":{"parts":[{"text":""}]}}]
                    }));
                }
                (30000, 55000)
            };
            let selection = serde_json::json!({"clips":[{
                "start_ms":start,"end_ms":end,"title":"Complete thought","reason":"Independent"
            }]})
            .to_string();
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "candidates":[{"finishReason":"STOP","content":{"parts":[{"text":selection}]}}]
            }))
        })
        .mount(&server)
        .await;
    let ai = AiConfig {
        provider: AiProvider::Gemini,
        model: "gemini-flash".into(),
        remote_consent: true,
        ..Default::default()
    };
    engine.remote_base = Some(server.uri());
    engine.test_key = Some("test-key".into());
    engine.set_ai(ai).await.unwrap();
    engine.process_next().await.unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(snapshot.videos[0].state, "failed");
    let error = snapshot.videos[0].error.as_deref().unwrap();
    assert!(error.contains("chunk 2"), "{error}");
    assert!(error.contains("Gemini clip selection is empty"), "{error}");
    assert!(snapshot.clips.is_empty());
    drop(engine);
    // Checkpoint must survive a real engine reopen, not just another call.
    let mut engine = Automation::open(data).await.unwrap();
    engine.remote_base = Some(server.uri());
    engine.test_key = Some("test-key".into());
    engine
        .set_mode(1, AutomationMode::AutoExport)
        .await
        .unwrap();
    engine
        .set_export_profile(ExportProfile::Original)
        .await
        .unwrap();
    engine
        .set_default_caption_style(CaptionStyle {
            enabled: false,
            ..Default::default()
        })
        .await
        .unwrap();
    let exports = dir.path().join("exports");
    engine.set_export_directory(exports.clone()).await.unwrap();
    let collision = exports.join("tehom-1-1.mp4");
    std::fs::write(&collision, b"unrelated user file").unwrap();
    engine.retry_video(1).await.unwrap();
    engine.process_next().await.unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(snapshot.videos[0].state, "failed");
    assert_eq!(
        snapshot.clips.len(),
        2,
        "complete analysis must be committed before export"
    );
    assert!(snapshot.clips.iter().all(|clip| clip.output_path.is_none()));
    assert_eq!(std::fs::read(&collision).unwrap(), b"unrelated user file");
    // Remove only the collision created by this test, then resume the committed drafts.
    std::fs::remove_file(&collision).unwrap();
    engine.retry_video(1).await.unwrap();
    engine.process_next().await.unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(
        snapshot.videos[0].state, "complete",
        "{:?}",
        snapshot.videos[0].error
    );
    assert_eq!(snapshot.clips.len(), 2);
    for clip in &snapshot.clips {
        let info = tehom_media::inspect(clip.output_path.as_ref().unwrap()).unwrap();
        assert!((info.duration - 25.0).abs() < 0.25);
        assert_eq!((info.width, info.height), (160, 90));
    }
    assert_eq!(first_calls.load(Ordering::SeqCst), 1);
    assert_eq!(second_calls.load(Ordering::SeqCst), 2);
    assert_eq!(std::fs::read(&video).unwrap(), original);
}

#[tokio::test]
async fn ai_malformed_responses_are_diagnosed_without_private_content() {
    for (body, expected) in [
        ("", "response envelope is empty"),
        ("{", "response envelope contains invalid JSON"),
        (r#"{"candidates":[{"finishReason":"SAFETY"}]}"#, "SAFETY"),
        (
            r#"{"candidates":[{"finishReason":"MAX_TOKENS"}]}"#,
            "MAX_TOKENS",
        ),
        (
            r#"{"candidates":[{"finishReason":"STOP","content":{"parts":[{"thought":true,"text":"private-thought"}]}}]}"#,
            "clip selection is empty",
        ),
        (
            r#"{"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"{\"clips\":["}]}}]}"#,
            "clip selection contains invalid JSON",
        ),
        (
            r#"{"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"{\"clips\":[{\"start_ms\":\"private-transcript\",\"end_ms\":25000,\"title\":\"private-title\"}]}"}]}}]}"#,
            "clip contract",
        ),
    ] {
        let (base, server) = provider_fixture("200 OK", body.into());
        let dir = tempfile::tempdir().unwrap();
        let mut engine = Automation::open(dir.path().into()).await.unwrap();
        engine.remote_base = Some(base);
        engine.test_key = Some("private-key".into());
        let ai = AiConfig {
            provider: AiProvider::Gemini,
            model: "gemini-flash".into(),
            remote_consent: true,
            ..Default::default()
        };
        let error = engine
            .analyze(&ai, "private-transcript", &AutomationSettings::default())
            .await
            .unwrap_err();
        server.join().unwrap();
        let message = format!("{error:#}");
        assert!(message.contains(expected), "{expected}: {message}");
        for secret in [
            "private-thought",
            "private-transcript",
            "private-title",
            "private-key",
        ] {
            assert!(!message.contains(secret), "private content leaked");
        }
    }
}

#[tokio::test]
async fn analysis_checkpoint_uses_captured_limits_even_after_settings_change() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Automation::open(dir.path().into()).await.unwrap();
    sqlx::query(
        "INSERT INTO sources(id,url,channel_id,title,mode) VALUES(1,'u','c','t','auto_draft')",
    )
    .execute(&engine.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO videos(id,source_id,external_id,title,url,state) VALUES(1,1,'v','t','u','queued')")
        .execute(&engine.pool).await.unwrap();
    let captured = AutomationSettings {
        max_clips: 1,
        ..Default::default()
    };
    engine
        .set_automation(AutomationSettings {
            max_clips: 3,
            ..Default::default()
        })
        .await
        .unwrap();
    let (base, server) = provider_fixture(
        "200 OK",
        serde_json::json!({
            "candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"{\"clips\":[]}"}]}}]
        })
        .to_string(),
    );
    engine.remote_base = Some(base);
    engine.test_key = Some("test-key".into());
    let ai = AiConfig {
        provider: AiProvider::Gemini,
        model: "gemini-flash".into(),
        remote_consent: true,
        ..Default::default()
    };
    assert!(
        engine
            .analyze_chunk(1, 0, &ai, "captured input", &captured)
            .await
            .unwrap()
            .is_empty()
    );
    let request = server.join().unwrap();
    let body: serde_json::Value =
        serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(
        body["generationConfig"]["responseJsonSchema"]["properties"]["clips"]["maxItems"],
        1
    );
    assert!(
        body["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("at most 1 objects")
    );
    // No listener remains: this succeeds only when the committed checkpoint is reused.
    assert!(
        engine
            .analyze_chunk(1, 0, &ai, "captured input", &captured)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn ai_retries_disconnected_response_bodies() {
    use std::io::{Read, Write};
    for (status, delay) in [
        ("200 OK", 0u64),
        ("429 Too Many Requests", 0),
        ("503 Service Unavailable", 0),
        ("429 Too Many Requests", 3600),
    ] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let expected_calls = if delay > 30 { 1 } else { 2 };
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut calls = 0;
            while calls < expected_calls && std::time::Instant::now() < deadline {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut buffer = [0; 4096];
                stream.read(&mut buffer).unwrap();
                calls += 1;
                let size = if calls == 1 { 100 } else { 2 };
                let status = if calls == 1 { status } else { "200 OK" };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nRetry-After: {delay}\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n{{}}"
                )
                .unwrap();
            }
            calls
        });
        let result = ai_response(reqwest::Client::new().get(address), "Fixture AI", 1024).await;
        let calls = server.join().unwrap();
        if delay > 30 {
            let error = result.unwrap_err();
            assert!(error.to_string().contains("3600"), "{error:#}");
        } else {
            assert_eq!(
                result.unwrap_or_else(|error| panic!("{status}: {error:#}")),
                b"{}"
            );
        }
        assert_eq!(calls, expected_calls);
    }
}

#[tokio::test]
async fn ai_rate_limit_long_delay_is_not_retried_early() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).set_body_json(
            serde_json::json!({"error":{"details":[{
                "@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"3600s"
            }]}}),
        ))
        .mount(&server)
        .await;
    let error = ai_response(reqwest::Client::new().get(server.uri()), "Fixture AI", 1024)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("3600"), "{error}");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn ai_missing_model_requires_input_without_echoing_provider_body() {
    let (base, server) = provider_fixture("404 Not Found", "private provider body".into());
    let error = ai_response(reqwest::Client::new().get(base), "Fixture AI", 1024)
        .await
        .unwrap_err()
        .to_string();
    server.join().unwrap();
    assert!(
        error.starts_with("Needs input:") && error.contains("model"),
        "{error}"
    );
    assert!(!error.contains("private provider body"));
}

#[cfg(unix)]
#[tokio::test]
async fn speech_invalid_utf8_is_reported_and_saved_as_readable_captions() {
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("actual.mp4");
    assert!(
        std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=s=64x64:r=2",
                "-f",
                "lavfi",
                "-i",
                "sine=f=300:r=16000",
                "-t",
                "2",
                "-c:v",
                "libx264",
                "-threads",
                "1",
                "-c:a",
                "aac"
            ])
            .arg(&video)
            .status()
            .unwrap()
            .success()
    );
    let data = dir.path().join("data");
    let mut engine = Automation::open(data.clone()).await.unwrap();
    engine
        .import_video(video.clone(), AutomationMode::AutoDraft)
        .await
        .unwrap();
    std::fs::create_dir_all(data.join("models")).unwrap();
    std::fs::write(data.join("models/ggml-base.bin"), b"fixture").unwrap();
    let folder = data.join("videos/1");
    std::fs::create_dir_all(&folder).unwrap();
    let script = r#"out=
while [ "$#" -gt 0 ]; do
    if [ "$1" = '-of' ]; then shift; out="$1"; fi
    shift
done
printf '1\n00:00:00,000 --> 00:00:02,000\nG\303\274zel \377 konu\n' > "$out.srt"
"#;
    engine.speech_program = Some(cli_fixture(dir.path(), script));
    let transcript = engine
        .transcribe(1, &folder, &video, 2.0, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();
    assert!(transcript.contains("Güzel � konu"));
    assert_eq!(
        engine.uploaded_transcript(1).await.unwrap().unwrap(),
        transcript
    );
    let warning: String = sqlx::query_scalar(
        "SELECT message FROM events WHERE video_id=1 AND level='warning' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&engine.pool)
    .await
    .unwrap();
    assert!(warning.contains("review the captions"), "{warning}");
    assert!(!warning.contains("Güzel"));

    // Uploaded user text is still strict: do not silently rewrite their file.
    let upload = dir.path().join("user.srt");
    let bytes = b"1\n00:00:00,000 --> 00:00:02,000\nUser \xff text\n";
    std::fs::write(&upload, bytes).unwrap();
    assert!(
        engine
            .import_transcript(1, upload.clone(), None)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&upload).unwrap(), bytes);
    assert_eq!(
        engine.uploaded_transcript(1).await.unwrap().unwrap(),
        transcript
    );
}
