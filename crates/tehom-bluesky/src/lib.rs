//! Bluesky posting through the AT Protocol's documented XRPC API. The person
//! signs in with their handle and an app password (never the main password);
//! the video goes to Bluesky's video service with a service-auth token from
//! their own server, and the post is created with a video embed.
use std::time::Duration;

use serde_json::{Value, json};
use unicode_segmentation::UnicodeSegmentation;

pub const POST_LIMIT: usize = 300;
const ENTRYWAY: &str = "https://bsky.social";
const VIDEO_SERVICE: &str = "https://video.bsky.app";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The handle or app password was refused.
    LoginRequired,
    Rejected(String),
    Transport(String),
}
pub type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LoginRequired => f.write_str("Bluesky refused the handle or app password"),
            Self::Rejected(why) => write!(f, "Bluesky refused the request: {why}"),
            Self::Transport(why) => write!(f, "Could not reach Bluesky: {why}"),
        }
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub did: String,
    pub handle: String,
}

pub struct Endpoints<'a> {
    pub entryway: &'a str,
    pub video: &'a str,
}
pub const LIVE: Endpoints<'static> = Endpoints {
    entryway: ENTRYWAY,
    video: VIDEO_SERVICE,
};

struct Session {
    jwt: String,
    did: String,
    handle: String,
    /// The person's own server, from their DID document.
    pds: String,
}

fn transport(error: reqwest::Error) -> Error {
    Error::Transport(error.to_string())
}

async fn reply(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(body);
    }
    let why = body["message"]
        .as_str()
        .or(body["error"].as_str())
        .unwrap_or("no reason given");
    Err(Error::Rejected(format!("{status}: {why}")))
}

async fn login(
    http: &reqwest::Client,
    base: &str,
    handle: &str,
    password: &str,
) -> Result<Session> {
    let response = http
        .post(format!("{base}/xrpc/com.atproto.server.createSession"))
        .json(&json!({"identifier": handle.trim().trim_start_matches('@'), "password": password}))
        .send()
        .await
        .map_err(transport)?;
    if matches!(response.status().as_u16(), 400 | 401) {
        return Err(Error::LoginRequired);
    }
    let body = reply(response).await?;
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    let pds = body["didDoc"]["service"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["id"] == "#atproto_pds")
        .map_or_else(|| base.to_owned(), |s| text(&s["serviceEndpoint"]));
    let session = Session {
        jwt: text(&body["accessJwt"]),
        did: text(&body["did"]),
        handle: text(&body["handle"]),
        pds: pds.trim_end_matches('/').to_owned(),
    };
    if session.jwt.is_empty() || session.did.is_empty() {
        return Err(Error::Rejected("the sign-in reply had no session".into()));
    }
    Ok(session)
}

/// Checks the handle and app password and returns who they belong to.
pub async fn sign_in(handle: &str, password: &str) -> Result<Account> {
    sign_in_at(&reqwest::Client::new(), ENTRYWAY, handle, password).await
}
async fn sign_in_at(
    http: &reqwest::Client,
    base: &str,
    handle: &str,
    password: &str,
) -> Result<Account> {
    let session = login(http, base, handle, password).await?;
    Ok(Account {
        did: session.did,
        handle: session.handle,
    })
}

fn too_long(text: &str) -> bool {
    text.graphemes(true).count() > POST_LIMIT
}

/// Posts the video; returns the post's web address.
pub async fn post_video(
    handle: &str,
    password: &str,
    expected_did: &str,
    video: Vec<u8>,
    size: (u32, u32),
    text: &str,
) -> Result<String> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(transport)?;
    post_video_at(
        &http,
        &LIVE,
        handle,
        password,
        expected_did,
        video,
        size,
        text,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn post_video_at(
    http: &reqwest::Client,
    ends: &Endpoints<'_>,
    handle: &str,
    password: &str,
    expected_did: &str,
    video: Vec<u8>,
    (width, height): (u32, u32),
    text: &str,
) -> Result<String> {
    let text = text.trim();
    if too_long(text) {
        return Err(Error::Rejected(format!(
            "posts can be at most {POST_LIMIT} characters"
        )));
    }
    let session = login(http, ends.entryway, handle, password).await?;
    if session.did != expected_did {
        return Err(Error::Rejected(
            "this app password belongs to a different account; connect Bluesky again".into(),
        ));
    }
    let host = session
        .pds
        .split("://")
        .nth(1)
        .unwrap_or(&session.pds)
        .to_owned();
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        + 1800;
    let auth = reply(
        http.get(format!(
            "{}/xrpc/com.atproto.server.getServiceAuth",
            session.pds
        ))
        .bearer_auth(&session.jwt)
        .query(&[
            ("aud", format!("did:web:{host}")),
            ("lxm", "com.atproto.repo.uploadBlob".into()),
            ("exp", expires.to_string()),
        ])
        .send()
        .await
        .map_err(transport)?,
    )
    .await?;
    let token = auth["token"].as_str().unwrap_or_default();
    let started = http
        .post(format!("{}/xrpc/app.bsky.video.uploadVideo", ends.video))
        .bearer_auth(token)
        .query(&[("did", session.did.as_str()), ("name", "clip.mp4")])
        .header("content-type", "video/mp4")
        .body(video)
        .send()
        .await
        .map_err(transport)?;
    // A repeat upload of the same file answers 409 with the earlier job.
    let conflict = started.status().as_u16() == 409;
    let job = match reply(started).await {
        Ok(job) => job,
        Err(Error::Rejected(_)) if conflict => {
            return Err(Error::Rejected("this video was already uploaded".into()));
        }
        Err(error) => return Err(error),
    };
    let job_id = job["jobId"].as_str().unwrap_or_default().to_owned();
    let mut blob = job["blob"].clone();
    for _ in 0..90 {
        if !blob.is_null() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        let status = reply(
            http.get(format!("{}/xrpc/app.bsky.video.getJobStatus", ends.video))
                .query(&[("jobId", job_id.as_str())])
                .send()
                .await
                .map_err(transport)?,
        )
        .await?;
        let status = &status["jobStatus"];
        if status["state"] == "JOB_STATE_FAILED" {
            return Err(Error::Rejected(
                status["error"]
                    .as_str()
                    .or(status["message"].as_str())
                    .unwrap_or("video processing failed")
                    .to_owned(),
            ));
        }
        blob = status["blob"].clone();
    }
    if blob.is_null() {
        return Err(Error::Rejected("video processing took too long".into()));
    }
    let mut embed = json!({"$type": "app.bsky.embed.video", "video": blob});
    if width > 0 && height > 0 {
        embed["aspectRatio"] = json!({"width": width, "height": height});
    }
    let created = reply(
        http.post(format!(
            "{}/xrpc/com.atproto.repo.createRecord",
            session.pds
        ))
        .bearer_auth(&session.jwt)
        .json(&json!({
            "repo": session.did,
            "collection": "app.bsky.feed.post",
            "record": {
                "$type": "app.bsky.feed.post",
                "text": text,
                "createdAt": created_at(),
                "embed": embed,
            },
        }))
        .send()
        .await
        .map_err(transport)?,
    )
    .await?;
    let key = created["uri"]
        .as_str()
        .and_then(|uri| uri.rsplit('/').next())
        .unwrap_or_default();
    Ok(format!(
        "https://bsky.app/profile/{}/post/{key}",
        session.handle
    ))
}

/// The current time as an RFC 3339 UTC string.
fn created_at() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{bearer_token, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn timestamps_are_rfc3339() {
        let now = created_at();
        assert_eq!(now.len(), 24);
        assert!(now.starts_with("20") && now.ends_with(".000Z"));
    }
    #[test]
    fn the_limit_counts_graphemes() {
        assert!(!too_long(&"👨‍👩‍👧".repeat(300)));
        assert!(too_long(&"a".repeat(301)));
    }

    async fn pds(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.server.createSession"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "accessJwt": "JWT", "did": "did:plc:me", "handle": "me.bsky.social",
                "didDoc": {"service": [{"id": "#atproto_pds", "serviceEndpoint": server.uri()}]}
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn posts_a_video_through_the_video_service() {
        let server = MockServer::start().await;
        pds(&server).await;
        Mock::given(method("GET"))
            .and(path("/xrpc/com.atproto.server.getServiceAuth"))
            .and(bearer_token("JWT"))
            .and(query_param("lxm", "com.atproto.repo.uploadBlob"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token": "SVC"})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/xrpc/app.bsky.video.uploadVideo"))
            .and(bearer_token("SVC"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"jobId": "J", "state": "JOB_STATE_CREATED"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/xrpc/app.bsky.video.getJobStatus"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jobStatus": {"state": "JOB_STATE_COMPLETED", "blob": {"ref": {"$link": "bafy"}}}
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.createRecord"))
            .and(bearer_token("JWT"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"uri": "at://did:plc:me/app.bsky.feed.post/3kabc"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let uri = server.uri();
        let url = post_video_at(
            &reqwest::Client::new(),
            &Endpoints {
                entryway: &uri,
                video: &uri,
            },
            "@me.bsky.social",
            "app-pass",
            "did:plc:me",
            b"video".to_vec(),
            (1080, 1920),
            "hello",
        )
        .await
        .unwrap();
        assert_eq!(url, "https://bsky.app/profile/me.bsky.social/post/3kabc");
        let requests = server.received_requests().await.unwrap();
        let record: Value = serde_json::from_slice(&requests.last().unwrap().body).unwrap();
        assert_eq!(record["record"]["embed"]["video"]["ref"]["$link"], "bafy");
        assert_eq!(record["record"]["embed"]["aspectRatio"]["height"], 1920);
        assert_eq!(record["record"]["text"], "hello");
    }

    #[tokio::test]
    async fn refused_passwords_and_other_accounts_are_told_apart() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let uri = server.uri();
        assert_eq!(
            sign_in_at(&reqwest::Client::new(), &uri, "me", "bad").await,
            Err(Error::LoginRequired)
        );
        let server = MockServer::start().await;
        pds(&server).await;
        let uri = server.uri();
        let error = post_video_at(
            &reqwest::Client::new(),
            &Endpoints {
                entryway: &uri,
                video: &uri,
            },
            "me",
            "p",
            "did:plc:other",
            vec![1],
            (0, 0),
            "x",
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Rejected(m) if m.contains("different account")));
    }
}
