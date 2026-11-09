//! End-to-end flows against a loopback wiremock server. No real Instagram traffic.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rsa::RsaPrivateKey;
use rsa::pkcs8::{EncodePublicKey, LineEnding};
use tehom_instagram::{Client, Login, Reel, TwoFactorMethod};
use wiremock::matchers::{method, path, path_regex, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn authorization() -> String {
    format!(
        "Bearer IGT:2:{}",
        STANDARD.encode(r#"{"ds_user_id":"4242","sessionid":"4242%3Aabc%3A1","should_use_header_over_cookies":true}"#)
    )
}

fn form_json(req: &Request) -> serde_json::Value {
    let body = String::from_utf8(req.body.clone()).unwrap();
    let encoded = body
        .strip_prefix("signed_body=SIGNATURE.")
        .expect("signed body");
    let mut bytes = Vec::new();
    let mut it = encoded.bytes();
    while let Some(b) = it.next() {
        match b {
            b'+' => bytes.push(b' '),
            b'%' => {
                let hex = [it.next().unwrap(), it.next().unwrap()];
                bytes.push(u8::from_str_radix(std::str::from_utf8(&hex).unwrap(), 16).unwrap());
            }
            _ => bytes.push(b),
        }
    }
    serde_json::from_slice(&bytes).unwrap()
}

fn header<'a>(req: &'a Request, name: &str) -> &'a str {
    req.headers
        .get(name)
        .unwrap_or_else(|| panic!("missing header {name} on {}", req.url))
        .to_str()
        .unwrap()
}

async fn mount_sync(server: &MockServer) {
    let key = RsaPrivateKey::new(&mut rand::rngs::OsRng, 1024).unwrap();
    let pem = key
        .to_public_key()
        .to_public_key_pem(LineEnding::LF)
        .unwrap();
    Mock::given(method("POST"))
        .and(path("/api/v1/launcher/sync/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ig-set-password-encryption-key-id", "201")
                .insert_header("ig-set-password-encryption-pub-key", STANDARD.encode(pem))
                .insert_header("ig-set-x-mid", "MID-123")
                .set_body_string(r#"{"configs":{},"status":"ok"}"#),
        )
        .mount(server)
        .await;
}

const USER: &str = r#"{"pk":4242,"username":"tehom","full_name":"Tehom Test"}"#;

#[tokio::test]
async fn login_then_upload_reel() {
    let server = MockServer::start().await;
    mount_sync(&server).await;
    Mock::given(method("POST"))
        .and(path("/api/v1/accounts/login/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ig-set-authorization", authorization())
                .insert_header("ig-set-ig-u-rur", "RUR-1")
                .set_body_string(format!(r#"{{"logged_in_user":{USER},"status":"ok"}}"#)),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex("^/upload_settings/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"status":"ok"}"#))
        .mount(&server)
        .await;
    Mock::given(path_regex("^/rupload_igvideo/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"offset":0,"status":"ok"}"#))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex("^/rupload_igphoto/"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"upload_id":"1","status":"ok"}"#),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/media/configure_to_clips/"))
        .and(query_param("video", "1"))
        .respond_with(
            ResponseTemplate::new(202)
                .set_body_string(r#"{"message":"Transcode not finished yet.","status":"fail"}"#),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/media/configure_to_clips/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"media":{"pk":3300000000000000001,"id":"3300000000000000001_4242","code":"DAbc123"},"status":"ok"}"#,
        ))
        .with_priority(2)
        .mount(&server)
        .await;

    let mut client = Client::new()
        .unwrap()
        .with_base_url(&server.uri())
        .with_configure_delay(Duration::from_millis(1));
    let account = match client.login("  tehom ", "s3cret pass").await.unwrap() {
        Login::Done(account) => account,
        Login::TwoFactor(_) => panic!("unexpected two-factor"),
    };
    assert_eq!(account.user_id, "4242");
    assert_eq!(account.full_name, "Tehom Test");
    let session = client.session();
    assert_eq!(session.authorization, authorization());
    assert_eq!(session.user_id.as_deref(), Some("4242"));
    assert_eq!(session.mid, "MID-123");
    assert_eq!(session.ig_u_rur, "RUR-1");

    let dir = std::env::temp_dir().join(format!("tehom-ig-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("clip.mp4");
    let cover = dir.join("cover.jpg");
    let video_bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&video, &video_bytes).unwrap();
    std::fs::write(&cover, b"\xff\xd8\xff\xe0fakejpeg").unwrap();

    // Restored client proves the session export is sufficient.
    let restored = Client::from_session(session)
        .unwrap()
        .with_base_url(&server.uri())
        .with_configure_delay(Duration::from_millis(1));
    let media = restored
        .upload_reel(&Reel {
            video: &video,
            cover: &cover,
            width: 1080,
            height: 1920,
            duration_ms: 15_500,
            caption: "Hello ünicode #tehom",
        })
        .await
        .unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(media.pk, "3300000000000000001");
    assert_eq!(media.code, "DAbc123");
    assert_eq!(media.url, "https://www.instagram.com/reel/DAbc123/");

    let requests = server.received_requests().await.unwrap();
    let paths: Vec<String> = requests
        .iter()
        .map(|r| format!("{} {}", r.method, r.url.path()))
        .collect();
    let expected_prefixes = [
        "POST /api/v1/launcher/sync/",
        "POST /api/v1/accounts/login/",
        "POST /upload_settings/",
        "GET /rupload_igvideo/",
        "POST /rupload_igvideo/",
        "POST /rupload_igphoto/",
        "POST /api/v1/media/configure_to_clips/",
        "POST /rupload_igphoto/",
        "POST /api/v1/media/configure_to_clips/",
    ];
    assert_eq!(paths.len(), expected_prefixes.len(), "{paths:#?}");
    for (p, e) in paths.iter().zip(expected_prefixes) {
        assert!(p.starts_with(e), "{p} vs {e}");
    }

    // Common private-API headers.
    for r in &requests {
        assert_eq!(header(r, "x-ig-app-id"), "567067343352427");
        assert!(header(r, "user-agent").starts_with("Instagram 449.0.0.52.84 Android (34/14;"));
    }
    let sync = form_json(&requests[0]);
    assert_eq!(sync["server_config_retrieval"], "1");
    assert_eq!(
        header(&requests[0], "content-type"),
        "application/x-www-form-urlencoded; charset=UTF-8"
    );

    let login = form_json(&requests[1]);
    assert_eq!(login["username"], "tehom");
    assert!(
        login["enc_password"]
            .as_str()
            .unwrap()
            .starts_with("#PWD_INSTAGRAM:4:")
    );
    assert!(!String::from_utf8_lossy(&requests[1].body).contains("s3cret"));
    assert_eq!(header(&requests[1], "x-mid"), "MID-123");

    for r in &requests[2..] {
        assert_eq!(header(r, "authorization"), authorization());
        assert_eq!(header(r, "ig-u-ds-user-id"), "4242");
        assert_eq!(header(r, "ig-u-rur"), "\"RUR-1\"");
    }

    let settings: serde_json::Value = serde_json::from_slice(&requests[2].body).unwrap();
    assert_eq!(
        settings["upload_setting_properties"]["video"]["video_original_file_size"],
        200_000
    );
    assert_eq!(header(&requests[2], "x-entity-name"), "upload_settings");

    let init = &requests[3];
    let params: serde_json::Value =
        serde_json::from_str(header(init, "x-instagram-rupload-params")).unwrap();
    assert_eq!(params["is_clips_video"], "1");
    assert_eq!(params["media_type"], "2");
    assert_eq!(params["upload_media_duration_ms"], "15500");
    let upload_id = params["upload_id"].as_str().unwrap().to_string();

    let upload = &requests[4];
    assert_eq!(upload.url.path(), init.url.path());
    assert!(upload.url.path().contains("-0-200000-"));
    assert_eq!(header(upload, "offset"), "0");
    assert_eq!(header(upload, "x-entity-length"), "200000");
    assert_eq!(header(upload, "content-length"), "200000");
    assert_eq!(header(upload, "content-type"), "application/octet-stream");
    assert!(upload.headers.get("transfer-encoding").is_none());
    assert_eq!(upload.body, video_bytes);

    let photo = &requests[5];
    assert!(
        photo
            .url
            .path()
            .starts_with(&format!("/rupload_igphoto/{upload_id}_0_"))
    );
    assert_eq!(header(photo, "x-entity-type"), "image/jpeg");
    assert_eq!(photo.body, b"\xff\xd8\xff\xe0fakejpeg");

    let configure = form_json(&requests[8]);
    assert_eq!(configure["upload_id"], upload_id.as_str());
    assert_eq!(configure["caption"], "Hello ünicode #tehom");
    assert_eq!(configure["clips_share_preview_to_feed"], "1");
    assert_eq!(configure["length"], 15.5);
    assert_eq!(
        configure["clips_segments_metadata"]["clips_segments"][0]["duration_ms"],
        15_500
    );
    assert_eq!(configure["device"]["model"], "Pixel 8 Pro");
    assert!(configure["_uuid"].is_string());
}

#[tokio::test]
async fn two_factor_login_then_account() {
    let server = MockServer::start().await;
    mount_sync(&server).await;
    Mock::given(method("POST"))
        .and(path("/api/v1/accounts/login/"))
        .respond_with(ResponseTemplate::new(400).set_body_string(
            r#"{"message":"","two_factor_required":true,"two_factor_info":{"username":"tehom","two_factor_identifier":"TFID","sms_two_factor_on":false,"totp_two_factor_on":true},"error_type":"two_factor_required","status":"fail"}"#,
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/accounts/two_factor_login/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ig-set-authorization", authorization())
                .set_body_string(format!(r#"{{"logged_in_user":{USER},"status":"ok"}}"#)),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/accounts/current_user/"))
        .and(query_param("edit", "true"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(format!(r#"{{"user":{USER},"status":"ok"}}"#)),
        )
        .mount(&server)
        .await;

    let mut client = Client::new().unwrap().with_base_url(&server.uri());
    assert!(client.account().await.is_err(), "no session yet");
    let challenge = match client.login("tehom", "pw").await.unwrap() {
        Login::TwoFactor(tf) => tf,
        Login::Done(_) => panic!("expected two-factor"),
    };
    assert_eq!(challenge.identifier, "TFID");
    assert_eq!(challenge.method, TwoFactorMethod::Totp);
    let account = client.two_factor(&challenge, " 123456 ").await.unwrap();
    assert_eq!(account.username, "tehom");
    assert_eq!(client.account().await.unwrap().user_id, "4242");

    let requests = server.received_requests().await.unwrap();
    let tf = requests
        .iter()
        .find(|r| r.url.path() == "/api/v1/accounts/two_factor_login/")
        .unwrap();
    let data = form_json(tf);
    assert_eq!(data["verification_code"], "123456");
    assert_eq!(data["two_factor_identifier"], "TFID");
    assert_eq!(data["verification_method"], "3");
    assert_eq!(data["_csrftoken"].as_str().unwrap().len(), 64);
    let current = requests.last().unwrap();
    assert_eq!(current.method.as_str(), "GET");
    assert_eq!(header(current, "authorization"), authorization());
}
