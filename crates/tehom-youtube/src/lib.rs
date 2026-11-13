//! YouTube Shorts posting through the person's own browser. They sign in to
//! Google in a Chromium-based window with its own profile; the session
//! cookies and page settings are read from that window, and the upload then
//! follows YouTube Studio's own web flow (resumable upload, then
//! `createvideo`). The steps follow adasq/youtube-studio (MIT). No
//! attestation or request-signing code is implemented or vendored.
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use tehom_browser::{Chromium, Page};
pub use tehom_browser::{Error, Result};

const STUDIO: &str = "https://studio.youtube.com";
const FALLBACK_CLIENT_VERSION: &str = "1.20201130.03.00";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// The channel id (`UC…`).
    pub user_id: String,
    pub username: String,
    pub full_name: String,
}

/// What a signed-in Studio page knows, plus the cookies to act as that user.
struct Session {
    cookies: String,
    sapisid: String,
    api_key: String,
    delegated: String,
    channel_id: String,
    client_version: String,
    user_agent: String,
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// `SAPISIDHASH`, the header Google's own web apps send with the cookies.
fn authorization(sapisid: &str, unix_seconds: u64) -> String {
    let digest = Sha1::digest(format!("{unix_seconds} {sapisid} {STUDIO}"));
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("SAPISIDHASH {unix_seconds}_{hex}")
}

async fn session(page: &mut Page) -> Result<Session> {
    let wait = Duration::from_secs(30);
    let config = page
        .eval(
            "(() => { const c = window.ytcfg && window.ytcfg.get ? k => window.ytcfg.get(k) : () => ''; \
             return {host: location.hostname, key: c('INNERTUBE_API_KEY') || '', \
             delegated: c('DELEGATED_SESSION_ID') || '', channel: c('CHANNEL_ID') || '', \
             version: c('INNERTUBE_CONTEXT_CLIENT_VERSION') || '', agent: navigator.userAgent}; })()",
            wait,
        )
        .await?;
    let cookies = page.call("Network.getAllCookies", json!({}), wait).await?;
    let mut header = Vec::new();
    let mut sapisid = String::new();
    for cookie in cookies["cookies"].as_array().into_iter().flatten() {
        let domain = text(&cookie["domain"]);
        if !(domain.ends_with("youtube.com") || domain.ends_with("google.com")) {
            continue;
        }
        let (name, value) = (text(&cookie["name"]), text(&cookie["value"]));
        if name == "SAPISID" || (name == "__Secure-3PAPISID" && sapisid.is_empty()) {
            sapisid = value.clone();
        }
        header.push(format!("{name}={value}"));
    }
    if text(&config["host"]) != "studio.youtube.com"
        || sapisid.is_empty()
        || text(&config["channel"]).is_empty()
    {
        return Err(Error::LoginRequired);
    }
    Ok(Session {
        cookies: header.join("; "),
        sapisid,
        api_key: text(&config["key"]),
        delegated: text(&config["delegated"]),
        channel_id: text(&config["channel"]),
        client_version: Some(text(&config["version"]))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| FALLBACK_CLIENT_VERSION.into()),
        user_agent: text(&config["agent"]),
    })
}

impl Session {
    fn headers(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        request
            .header("authorization", authorization(&self.sapisid, now))
            .header("cookie", &self.cookies)
            .header("x-origin", STUDIO)
            .header("x-goog-authuser", "0")
            .header("user-agent", &self.user_agent)
    }
    fn delegation(&self) -> Value {
        json!({"roleType": {"channelRoleType": "CREATOR_CHANNEL_ROLE_TYPE_OWNER"}, "externalChannelId": self.channel_id})
    }
    /// The channel's display name, when Studio tells it; empty otherwise.
    async fn channel_name(&self, http: &reqwest::Client, studio: &str) -> String {
        let body = json!({
            "channelIds": [self.channel_id],
            "mask": {"channelId": true, "title": true},
            "context": {"client": {"clientName": 62, "clientVersion": self.client_version, "hl": "en", "gl": "US"},
                        "user": {"onBehalfOfUser": self.delegated}},
        });
        let answer = self
            .headers(http.post(format!(
                "{studio}/youtubei/v1/creator/get_creator_channels?alt=json&key={}",
                self.api_key
            )))
            .json(&body)
            .send()
            .await;
        let Ok(answer) = answer else {
            return String::new();
        };
        let value: Value = answer.json().await.unwrap_or(Value::Null);
        text(&value["channels"][0]["title"])
    }
}

async fn account(page: &mut Page) -> Result<Account> {
    let session = session(page).await?;
    let http = reqwest::Client::new();
    let name = session.channel_name(&http, STUDIO).await;
    Ok(Account {
        full_name: if name.is_empty() {
            session.channel_id.clone()
        } else {
            name.clone()
        },
        username: if name.is_empty() {
            session.channel_id.clone()
        } else {
            name
        },
        user_id: session.channel_id,
    })
}

/// Opens YouTube Studio and waits for the person to sign in to Google there
/// (up to ten minutes). The password is typed into Google's own page.
pub async fn sign_in(profile: &Path) -> Result<Account> {
    let browser = Chromium::launch(profile, STUDIO).await?;
    let result = async {
        let mut page = browser.page().await?;
        for _ in 0..600 {
            match account(&mut page).await {
                Ok(account) => return Ok(account),
                Err(Error::LoginRequired) => tokio::time::sleep(Duration::from_secs(1)).await,
                Err(other) => return Err(other),
            }
        }
        Err(Error::Rejected(
            "YouTube sign-in timed out; try again".into(),
        ))
    }
    .await;
    browser.close().await;
    // Closing the window before signing in is the person cancelling.
    result.map_err(|error| match error {
        Error::Transport(message) if message.contains("closed") => {
            Error::Rejected("The YouTube window was closed before sign-in finished".into())
        }
        other => other,
    })
}

/// Title (first line, at most 100 characters) and description of a Short.
fn title_and_description(caption: &str) -> (String, String) {
    let caption = caption.trim();
    let title: String = caption
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(100)
        .collect();
    let mut description = caption.to_owned();
    if !description.contains("#Shorts") {
        description.push_str("\n\n#Shorts");
    }
    (
        if title.is_empty() {
            "Short".into()
        } else {
            title
        },
        description,
    )
}

struct Endpoints<'a> {
    upload: &'a str,
    studio: &'a str,
}

async fn upload_and_create(
    http: &reqwest::Client,
    ends: &Endpoints<'_>,
    session: &Session,
    bytes: Vec<u8>,
    caption: &str,
    private: bool,
) -> Result<String> {
    let transport = |error: reqwest::Error| Error::Transport(error.to_string());
    let upload_id = format!("innertube_studio:{}:0", uuid::Uuid::new_v4());
    let file_name = format!("file-{}", upload_id.len() * bytes.len());
    let form = "application/x-www-form-urlencoded;charset=utf-8";
    let start = session
        .headers(http.post(format!("{}/upload/studio", ends.upload)))
        .header("content-type", form)
        .header("x-goog-upload-command", "start")
        .header("x-goog-upload-file-name", &file_name)
        .header("x-goog-upload-protocol", "resumable")
        .body(json!({"frontendUploadId": upload_id}).to_string())
        .send()
        .await
        .map_err(transport)?;
    if matches!(start.status().as_u16(), 401 | 403) {
        return Err(Error::LoginRequired);
    }
    let url = start
        .headers()
        .get("x-goog-upload-url")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| {
            Error::Rejected(format!(
                "YouTube did not start the upload (HTTP {})",
                start.status()
            ))
        })?;
    let sent = session
        .headers(http.post(url))
        .header("content-type", form)
        .header("x-goog-upload-command", "upload, finalize")
        .header("x-goog-upload-file-name", &file_name)
        .header("x-goog-upload-offset", "0")
        .body(bytes)
        .send()
        .await
        .map_err(transport)?;
    let status = sent.status();
    let sent: Value = sent.json().await.unwrap_or(Value::Null);
    let resource = text(&sent["scottyResourceId"]);
    if resource.is_empty() {
        return Err(Error::Rejected(format!(
            "YouTube did not accept the video bytes (HTTP {status})"
        )));
    }
    let (title, description) = title_and_description(caption);
    let body = json!({
        "channelId": session.channel_id,
        "resourceId": {"scottyResourceId": {"id": resource}},
        "frontendUploadId": upload_id,
        "initialMetadata": {
            "title": {"newTitle": title},
            "description": {"newDescription": description, "shouldSegment": true},
            "privacy": {"newPrivacy": if private { "PRIVATE" } else { "PUBLIC" }},
            "draftState": {"isDraft": false},
        },
        "context": {
            "client": {"clientName": 62, "clientVersion": session.client_version, "hl": "en", "gl": "US"},
            "request": {"returnLogEntry": true, "internalExperimentFlags": []},
            "user": {
                "onBehalfOfUser": session.delegated,
                "delegationContext": session.delegation(),
                "serializedDelegationContext": "",
            },
            "clientScreenNonce": "",
        },
        "delegationContext": session.delegation(),
    });
    let created = session
        .headers(http.post(format!(
            "{}/youtubei/v1/upload/createvideo?alt=json&key={}",
            ends.studio, session.api_key
        )))
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(transport)?;
    let status = created.status();
    let created: Value = created.json().await.unwrap_or(Value::Null);
    let id = text(&created["videoId"]);
    if id.is_empty() {
        let reason = text(&created["error"]["message"]);
        return Err(Error::Rejected(format!(
            "YouTube refused the video: {} (HTTP {status})",
            if reason.is_empty() {
                "no reason given".into()
            } else {
                reason
            }
        )));
    }
    Ok(id)
}

/// Uploads `video` as a Short. `expected_channel` is the channel the browser
/// must be signed in to. Returns the new video's id. The window stays open
/// for the few minutes this takes.
pub async fn post_video(
    profile: &Path,
    expected_channel: &str,
    video: &Path,
    caption: &str,
    private: bool,
) -> Result<String> {
    // shortcut: the whole file is read into memory; clips are small, upgrade to streaming if not.
    let bytes = tokio::fs::read(video)
        .await
        .map_err(|error| Error::File(error.to_string()))?;
    if bytes.is_empty() {
        return Err(Error::File("the video file is empty".into()));
    }
    let browser = Chromium::launch(profile, STUDIO).await?;
    let result = async {
        let mut page = browser.page().await?;
        page.settled().await?;
        let session = session(&mut page).await?;
        if session.channel_id != expected_channel {
            return Err(Error::Rejected(
                "The browser is signed in to a different YouTube channel; connect YouTube Shorts again".into(),
            ));
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|error| Error::Transport(error.to_string()))?;
        upload_and_create(
            &http,
            &Endpoints { upload: "https://upload.youtube.com", studio: STUDIO },
            &session,
            bytes,
            caption,
            private,
        )
        .await
    }
    .await;
    browser.close().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn session() -> Session {
        Session {
            cookies: "SAPISID=abc; SID=x".into(),
            sapisid: "abc".into(),
            api_key: "KEY".into(),
            delegated: "DELEGATED".into(),
            channel_id: "UCchannel".into(),
            client_version: "1.2".into(),
            user_agent: "UA".into(),
        }
    }

    #[test]
    fn authorization_hashes_time_cookie_and_origin() {
        let value = authorization("abc", 1_700_000_000);
        let expected = Sha1::digest(b"1700000000 abc https://studio.youtube.com");
        let hex: String = expected.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(value, format!("SAPISIDHASH 1700000000_{hex}"));
        assert_eq!(hex.len(), 40);
    }
    #[test]
    fn shorts_get_a_title_and_the_shorts_tag() {
        assert_eq!(
            title_and_description(" First line\nmore #fyp "),
            (
                "First line".to_string(),
                "First line\nmore #fyp\n\n#Shorts".to_string()
            )
        );
        assert_eq!(title_and_description("x #Shorts").1, "x #Shorts");
        assert_eq!(title_and_description("").0, "Short");
        assert_eq!(
            title_and_description(&"é".repeat(150)).0.chars().count(),
            100
        );
    }

    #[tokio::test]
    async fn uploads_then_creates_the_video_with_the_session_headers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload/studio"))
            .and(header("x-goog-upload-command", "start"))
            .and(header("x-origin", STUDIO))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-goog-upload-url", format!("{}/resumable/1", server.uri())),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/resumable/1"))
            .and(header("x-goog-upload-offset", "0"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"scottyResourceId": "RES"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/upload/createvideo"))
            .and(query_param("key", "KEY"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"videoId": "VID123"})))
            .expect(1)
            .mount(&server)
            .await;
        let uri = server.uri();
        let id = upload_and_create(
            &reqwest::Client::new(),
            &Endpoints {
                upload: &uri,
                studio: &uri,
            },
            &session(),
            b"video".to_vec(),
            "Hello #fyp",
            false,
        )
        .await
        .unwrap();
        assert_eq!(id, "VID123");
        let requests = server.received_requests().await.unwrap();
        let create: Value = serde_json::from_slice(&requests[2].body).unwrap();
        assert_eq!(create["resourceId"]["scottyResourceId"]["id"], "RES");
        assert_eq!(create["initialMetadata"]["privacy"]["newPrivacy"], "PUBLIC");
        assert_eq!(create["initialMetadata"]["title"]["newTitle"], "Hello #fyp");
        assert!(
            requests[2]
                .headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("SAPISIDHASH ")
        );
    }

    #[tokio::test]
    async fn a_refused_start_means_sign_in_again_and_other_failures_say_why() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let uri = server.uri();
        let ends = Endpoints {
            upload: &uri,
            studio: &uri,
        };
        let error = upload_and_create(
            &reqwest::Client::new(),
            &ends,
            &session(),
            vec![1],
            "x",
            true,
        )
        .await
        .unwrap_err();
        assert_eq!(error, Error::LoginRequired);

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let uri = server.uri();
        let ends = Endpoints {
            upload: &uri,
            studio: &uri,
        };
        let error = upload_and_create(
            &reqwest::Client::new(),
            &ends,
            &session(),
            vec![1],
            "x",
            true,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Rejected(m) if m.contains("did not start")));
    }
}
