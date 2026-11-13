//! Facebook Reels through the documented Graph API (`video_reels`). The person
//! brings a Page id and a Page access token from their own Meta app; posting
//! as a Page needs `pages_manage_posts`, and apps outside development mode
//! need Meta's App Review.
use std::time::Duration;

use serde_json::Value;

pub const DESCRIPTION_LIMIT: usize = 2200;
const GRAPH: &str = "https://graph.facebook.com/v21.0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The token was refused or has expired.
    LoginRequired,
    Rejected(String),
    Transport(String),
}
pub type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LoginRequired => f.write_str("Facebook refused the Page access token"),
            Self::Rejected(why) => write!(f, "Facebook refused the request: {why}"),
            Self::Transport(why) => write!(f, "Could not reach Facebook: {why}"),
        }
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub page_id: String,
    pub name: String,
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
    let error = &body["error"];
    // Graph code 190 is an invalid or expired token.
    if status.as_u16() == 401 || error["code"] == 190 {
        return Err(Error::LoginRequired);
    }
    let why = error["message"].as_str().unwrap_or("no reason given");
    Err(Error::Rejected(format!("{status}: {why}")))
}

/// Checks the token against the Page and returns the Page.
pub async fn sign_in(page_id: &str, token: &str) -> Result<Account> {
    sign_in_at(&reqwest::Client::new(), GRAPH, page_id, token).await
}
async fn sign_in_at(
    http: &reqwest::Client,
    graph: &str,
    page_id: &str,
    token: &str,
) -> Result<Account> {
    let page_id = page_id.trim();
    let body = reply(
        http.get(format!("{graph}/{page_id}"))
            .query(&[("fields", "id,name"), ("access_token", token.trim())])
            .send()
            .await
            .map_err(transport)?,
    )
    .await?;
    let text = |key: &str| body[key].as_str().unwrap_or_default().to_owned();
    if text("id") != page_id {
        return Err(Error::Rejected(
            "the token does not belong to that Page".into(),
        ));
    }
    Ok(Account {
        page_id: text("id"),
        name: text("name"),
    })
}

/// Posts the video as a Reel; returns its web address.
pub async fn post_video(
    page_id: &str,
    token: &str,
    video: Vec<u8>,
    description: &str,
) -> Result<String> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(transport)?;
    post_video_at(&http, GRAPH, page_id, token, video, description).await
}

async fn post_video_at(
    http: &reqwest::Client,
    graph: &str,
    page_id: &str,
    token: &str,
    video: Vec<u8>,
    description: &str,
) -> Result<String> {
    let description = description.trim();
    if description.chars().count() > DESCRIPTION_LIMIT {
        return Err(Error::Rejected(format!(
            "descriptions can be at most {DESCRIPTION_LIMIT} characters"
        )));
    }
    let token = token.trim();
    let start = reply(
        http.post(format!("{graph}/{page_id}/video_reels"))
            .query(&[("upload_phase", "start"), ("access_token", token)])
            .send()
            .await
            .map_err(transport)?,
    )
    .await?;
    let id = start["video_id"].as_str().unwrap_or_default().to_owned();
    let upload_url = start["upload_url"].as_str().unwrap_or_default().to_owned();
    if id.is_empty() || upload_url.is_empty() {
        return Err(Error::Rejected("the upload did not start".into()));
    }
    let size = video.len();
    reply(
        http.post(&upload_url)
            .header("Authorization", format!("OAuth {token}"))
            .header("offset", "0")
            .header("file_size", size.to_string())
            .body(video)
            .send()
            .await
            .map_err(transport)?,
    )
    .await?;
    let done = reply(
        http.post(format!("{graph}/{page_id}/video_reels"))
            .query(&[
                ("upload_phase", "finish"),
                ("video_id", id.as_str()),
                ("video_state", "PUBLISHED"),
                ("description", description),
                ("access_token", token),
            ])
            .send()
            .await
            .map_err(transport)?,
    )
    .await?;
    if done["success"] != true {
        return Err(Error::Rejected("Facebook did not publish the Reel".into()));
    }
    Ok(format!("https://www.facebook.com/reel/{id}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn starts_uploads_and_finishes_the_reel() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/P/video_reels"))
            .and(query_param("upload_phase", "start"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"video_id": "V1", "upload_url": format!("{}/rupload/V1", server.uri())}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/rupload/V1"))
            .and(header("authorization", "OAuth TOKEN"))
            .and(header("file_size", "5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success": true})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/P/video_reels"))
            .and(query_param("upload_phase", "finish"))
            .and(query_param("video_state", "PUBLISHED"))
            .and(query_param("description", "hi"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success": true})))
            .expect(1)
            .mount(&server)
            .await;
        let url = post_video_at(
            &reqwest::Client::new(),
            &server.uri(),
            "P",
            "TOKEN",
            b"video".to_vec(),
            "hi",
        )
        .await
        .unwrap();
        assert_eq!(url, "https://www.facebook.com/reel/V1");
    }

    #[tokio::test]
    async fn expired_tokens_and_foreign_pages_are_told_apart() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/P"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(json!({"error": {"code": 190, "message": "expired"}})),
            )
            .mount(&server)
            .await;
        assert_eq!(
            sign_in_at(&reqwest::Client::new(), &server.uri(), "P", "t").await,
            Err(Error::LoginRequired)
        );
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "Q", "name": "n"})))
            .mount(&server)
            .await;
        let error = sign_in_at(&reqwest::Client::new(), &server.uri(), "P", "t")
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Rejected(_)));
    }
}
