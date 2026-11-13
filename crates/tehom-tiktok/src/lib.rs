//! TikTok posting through the person's own browser. They sign in to TikTok
//! in a Chromium-based window with its own profile; uploads and the post
//! request are made from that logged-in page, so TikTok's scripts sign them.
//! The upload steps follow makiisthenes/TiktokAutoUploader (AGPL-3.0);
//! none of its request-signing code is used.
mod sign;

use std::path::Path;
use std::time::Duration;

use rand::Rng;
use serde_json::{Value, json};
use sign::{VodAuth, sign_vod};
use tokio::io::AsyncReadExt;

const LOGIN_URL: &str = "https://www.tiktok.com/login";
const STUDIO_URL: &str = "https://www.tiktok.com/tiktokstudio/upload";
const AID: &str = "1988";
const CHUNK: usize = 5 * 1024 * 1024;

use tehom_browser::{Chromium, Page};
pub use tehom_browser::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub user_id: String,
    pub username: String,
    pub full_name: String,
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

async fn account(page: &mut Page) -> Result<Account> {
    let (_, answer) = page
        .fetch(
            "GET",
            &format!("https://www.tiktok.com/passport/web/account/info/?aid={AID}"),
            &[("accept", "application/json, text/plain, */*".into())],
            None,
        )
        .await?;
    let data = &answer["data"];
    let user_id = text(&data["user_id"]);
    if user_id.is_empty() || user_id == "0" {
        return Err(Error::LoginRequired);
    }
    let (username, screen_name) = (text(&data["username"]), text(&data["screen_name"]));
    Ok(Account {
        user_id,
        full_name: if screen_name.is_empty() {
            username.clone()
        } else {
            screen_name.clone()
        },
        username: if username.is_empty() {
            screen_name
        } else {
            username
        },
    })
}

/// Opens the browser on TikTok's sign-in page and waits for the person to
/// finish signing in there (up to ten minutes). Their password is typed into
/// TikTok's own page; Tehom never sees it.
pub async fn sign_in(profile: &Path) -> Result<Account> {
    let browser = Chromium::launch(profile, LOGIN_URL).await?;
    let result = async {
        let mut page = browser.page().await?;
        let wait = Duration::from_secs(30);
        for _ in 0..600 {
            let cookies = page.call("Network.getAllCookies", json!({}), wait).await?;
            let has = |name: &str| {
                cookies["cookies"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|cookie| {
                        cookie["name"] == name
                            && cookie["domain"]
                                .as_str()
                                .is_some_and(|d| d.contains("tiktok.com"))
                            && cookie["value"].as_str().is_some_and(|v| !v.is_empty())
                    })
            };
            if has("sessionid") {
                page.call("Page.navigate", json!({"url": STUDIO_URL}), wait)
                    .await?;
                page.settled().await?;
                return account(&mut page).await;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Err(Error::Rejected(
            "TikTok sign-in timed out; try again".into(),
        ))
    }
    .await;
    browser.close().await;
    // Closing the window before signing in is the person cancelling.
    result.map_err(|error| match error {
        Error::Transport(message) if message.contains("closed") => {
            Error::Rejected("The TikTok window was closed before sign-in finished".into())
        }
        other => other,
    })
}

/// `#hashtags` as TikTok's web uploader marks them up, with UTF-16 offsets
/// like the JavaScript client. Mentions stay plain text.
fn caption_markup(caption: &str) -> (String, Vec<Value>) {
    let (mut markup, mut extra) = (String::new(), Vec::new());
    let chars: Vec<char> = caption.chars().collect();
    let (mut i, mut offset) = (0, 0);
    while i < chars.len() {
        let start = i;
        if chars[i] == '#' {
            let mut end = i + 1;
            while chars
                .get(end)
                .is_some_and(|c| c.is_alphanumeric() || *c == '_')
            {
                end += 1;
            }
            if end > i + 1 {
                let tag: String = chars[i..end].iter().collect();
                let width: usize = tag.chars().map(char::len_utf16).sum();
                let id = extra.len();
                extra.push(json!({
                    "start": offset, "end": offset + width, "hashtag_name": &tag[1..],
                    "tag_id": id.to_string(), "type": 1, "user_id": "",
                }));
                markup.push_str(&format!("<h id=\"{id}\">{tag}</h>"));
                offset += width;
                i = end;
                continue;
            }
        }
        markup.push(chars[start]);
        offset += chars[start].len_utf16();
        i += 1;
    }
    (markup, extra)
}

fn expect(what: &str, answer: (u16, Value)) -> Result<Value> {
    match answer {
        (200, body) => Ok(body),
        (401 | 403, _) => Err(Error::LoginRequired),
        (status, body) => Err(Error::Rejected(format!(
            "TikTok refused the {what} (HTTP {status}) {}",
            body.to_string().chars().take(160).collect::<String>()
        ))),
    }
}

/// Uploads `video` and posts it. `expected_user` is the account id the
/// browser must be signed in as. Returns TikTok's id for the new post.
/// The TikTok window stays open for the few minutes this takes.
pub async fn post_video(
    profile: &Path,
    expected_user: &str,
    video: &Path,
    caption: &str,
    private: bool,
) -> Result<String> {
    let size = tokio::fs::metadata(video)
        .await
        .map_err(|error| Error::File(error.to_string()))?
        .len();
    if size == 0 {
        return Err(Error::File("the video file is empty".into()));
    }
    let browser = Chromium::launch(profile, STUDIO_URL).await?;
    let result = async {
        let mut page = browser.page().await?;
        page.settled().await?;
        if account(&mut page).await?.user_id != expected_user {
            return Err(Error::Rejected(
                "The browser is signed in to a different TikTok account; connect TikTok again"
                    .into(),
            ));
        }
        upload_and_post(&mut page, video, size, caption, private).await
    }
    .await;
    browser.close().await;
    result
}

async fn upload_and_post(
    page: &mut Page,
    video: &Path,
    size: u64,
    caption: &str,
    private: bool,
) -> Result<String> {
    let json_headers = [("accept", "application/json, text/plain, */*".to_string())];
    let auth = expect(
        "upload authorization",
        page.fetch(
            "GET",
            &format!("https://www.tiktok.com/api/v1/video/upload/auth/?aid={AID}"),
            &json_headers,
            None,
        )
        .await?,
    )?;
    let token = &auth["video_token_v5"];
    let vod = VodAuth {
        access_key: text(&token["access_key_id"]),
        // The misspelling is TikTok's.
        secret_key: text(&token["secret_acess_key"]),
        session_token: text(&token["session_token"]),
    };
    if vod.access_key.is_empty() || vod.secret_key.is_empty() {
        return Err(Error::LoginRequired);
    }
    let creation_id: String = {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_";
        let mut rng = rand::thread_rng();
        (0..21)
            .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
            .collect()
    };
    let created = expect(
        "project",
        page.fetch(
            "POST",
            &format!("https://www.tiktok.com/api/v1/web/project/create/?creation_id={creation_id}&type=1&aid={AID}"),
            &json_headers,
            None,
        )
        .await?,
    )?;
    if created.get("project").is_none() {
        return Err(Error::LoginRequired);
    }

    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let size_text = size.to_string();
    let vod_call = |method: &'static str, query: Vec<(&str, &str)>, body: String| {
        let (query, mut headers) = sign_vod(method, &query, &body, &vod, unix);
        if !body.is_empty() {
            headers.push(("content-type", "text/plain;charset=UTF-8".into()));
        }
        (
            format!("https://www.tiktok.com/top/v1?{query}"),
            headers,
            body,
        )
    };
    let (url, headers, _) = vod_call(
        "GET",
        vec![
            ("Action", "ApplyUploadInner"),
            ("Version", "2020-11-19"),
            ("SpaceName", "tiktok"),
            ("FileType", "video"),
            ("IsInner", "1"),
            ("FileSize", &size_text),
            ("s", "g158iqx8434"),
        ],
        String::new(),
    );
    let applied = expect(
        "upload request",
        page.fetch("GET", &url, &headers, None).await?,
    )?;
    let node = &applied["Result"]["InnerUploadAddress"]["UploadNodes"][0];
    let store = &node["StoreInfos"][0];
    let (host, uri, store_auth) = (
        text(&node["UploadHost"]),
        text(&store["StoreUri"]),
        text(&store["Auth"]),
    );
    let (video_id, session_key) = (text(&node["Vid"]), text(&node["SessionKey"]));
    if host.is_empty() || uri.is_empty() || video_id.is_empty() {
        return Err(Error::Protocol("TikTok offered no upload address".into()));
    }

    // The bytes go to TikTok's storage host, not through the page.
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|error| Error::Transport(error.to_string()))?;
    let upload_id = uuid::Uuid::new_v4().to_string();
    let mut file = tokio::fs::File::open(video)
        .await
        .map_err(|error| Error::File(error.to_string()))?;
    let mut checksums = Vec::new();
    loop {
        let mut chunk = Vec::with_capacity(CHUNK);
        (&mut file)
            .take(CHUNK as u64)
            .read_to_end(&mut chunk)
            .await
            .map_err(|error| Error::File(error.to_string()))?;
        if chunk.is_empty() {
            break;
        }
        let crc = format!("{:08x}", crc32fast::hash(&chunk));
        let part = checksums.len() + 1;
        let response = http
            .post(format!(
                "https://{host}/{uri}?partNumber={part}&uploadID={upload_id}&phase=transfer"
            ))
            .header("Authorization", &store_auth)
            .header("Content-Type", "application/octet-stream")
            .header("Content-Disposition", "attachment; filename=\"undefined\"")
            .header("Content-Crc32", &crc)
            .body(chunk)
            .send()
            .await
            .map_err(|error| Error::Transport(error.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::Rejected(format!(
                "TikTok storage refused part {part} (HTTP {})",
                response.status()
            )));
        }
        checksums.push(format!("{part}:{crc}"));
    }
    let finished = http
        .post(format!(
            "https://{host}/{uri}?uploadID={upload_id}&phase=finish&uploadmode=part"
        ))
        .header("Authorization", &store_auth)
        .header("Content-Type", "text/plain;charset=UTF-8")
        .body(checksums.join(","))
        .send()
        .await
        .map_err(|error| Error::Transport(error.to_string()))?;
    if !finished.status().is_success() {
        return Err(Error::Rejected(format!(
            "TikTok storage did not finish the upload (HTTP {})",
            finished.status()
        )));
    }

    let commit = json!({"SessionKey": session_key, "Functions": [{"name": "GetMeta"}]}).to_string();
    let (url, headers, body) = vod_call(
        "POST",
        vec![
            ("Action", "CommitUploadInner"),
            ("Version", "2020-11-19"),
            ("SpaceName", "tiktok"),
        ],
        commit,
    );
    expect(
        "upload confirmation",
        page.fetch("POST", &url, &headers, Some(&body)).await?,
    )?;

    let (markup, text_extra) = caption_markup(caption);
    let post = json!({
        "post_common_info": {"creation_id": creation_id, "enter_post_page_from": 1, "post_type": 3},
        "feature_common_info_list": [{
            "geofencing_regions": [],
            "playlist_name": "",
            "playlist_id": "",
            "tcm_params": "{\"commerce_toggle_info\":{}}",
            "sound_exemption": 0,
            "anchors": [],
            "vedit_common_info": {"draft": "", "video_id": video_id},
            "privacy_setting_info": {
                "visibility_type": i32::from(private),
                "allow_duet": 0,
                "allow_stitch": 0,
                "allow_comment": 1,
            },
        }],
        "single_post_req_list": [{
            "batch_index": 0,
            "video_id": video_id,
            "is_long_video": 0,
            "single_post_feature_info": {
                "text": caption,
                "text_extra": text_extra,
                "markup_text": markup,
                "music_info": {},
                "poster_delay": 0,
            },
        }],
    });
    let (status, posted) = page
        .fetch(
            "POST",
            &format!("https://www.tiktok.com/tiktok/web/project/post/v1/?app_name=tiktok_web&channel=tiktok_web&device_platform=web&aid={AID}"),
            &[
                ("content-type", "application/json".to_string()),
                ("accept", "application/json, text/plain, */*".to_string()),
            ],
            Some(&post.to_string()),
        )
        .await?;
    let item = text(&posted["single_post_resp_list"][0]["item_id"]);
    if status != 200 || posted["status_code"] != 0 {
        let reason = text(&posted["status_msg"]);
        return Err(Error::Rejected(format!(
            "TikTok rejected the post: {} (HTTP {status})",
            if reason.is_empty() {
                "no reason given".into()
            } else {
                reason
            }
        )));
    }
    // TikTok accepted the post; the id is empty when it did not say which video it is.
    Ok(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashtags_get_markup_and_utf16_offsets() {
        let (markup, extra) = caption_markup("a😀 #fyp ünï #bir_iki. @user #");
        assert_eq!(
            markup,
            "a😀 <h id=\"0\">#fyp</h> ünï <h id=\"1\">#bir_iki</h>. @user #"
        );
        assert_eq!(extra[0]["start"], 4); // 'a' + emoji (2 units) + space
        assert_eq!(extra[0]["end"], 8);
        assert_eq!(extra[0]["hashtag_name"], "fyp");
        assert_eq!(extra[1]["hashtag_name"], "bir_iki");
        assert_eq!(extra.len(), 2);
    }
    #[test]
    fn plain_captions_pass_through() {
        assert_eq!(caption_markup("Hello"), ("Hello".to_string(), vec![]));
    }
    #[test]
    fn unauthorised_answers_mean_sign_in_again() {
        assert_eq!(
            expect("x", (401, Value::Null)).unwrap_err(),
            Error::LoginRequired
        );
        assert!(matches!(
            expect("x", (429, Value::Null)),
            Err(Error::Rejected(_))
        ));
    }
}
