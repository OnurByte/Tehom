//! Native Rust client for the Instagram private mobile API. Protocol logic is adapted from aiograpi (MIT, https://github.com/subzeroid/aiograpi, commit 2f82cd3b); see LICENSE-aiograpi.
//!
//! Scope: username/password login, two-factor login, session export/restore and Reel upload.

mod caa;
mod crypto;
mod error;

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::Rng;
use reqwest::cookie::{CookieStore, Jar};
use reqwest::header::HeaderMap;
use reqwest::{Method, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub use error::Error;
use error::classify;

const API_ORIGIN: &str = "https://i.instagram.com";
const APP_ID: &str = "567067343352427";
const LOCALE: &str = "en_US";
const COUNTRY: &str = "US";
const COUNTRY_CODE: u32 = 1;
const TIMEZONE_OFFSET: i32 = -14400;
/// aiograpi `PrivateRequestMixin.read_timeout`.
const READ_TIMEOUT: Duration = Duration::from_secs(25);
/// aiograpi `clip_upload(configure_timeout=10)` and its 50 attempts.
const CONFIGURE_DELAY: Duration = Duration::from_secs(10);
const CONFIGURE_ATTEMPTS: usize = 50;

/// Android device + app profile (aiograpi `config.DEVICE_SETTINGS` / `APP_SETTINGS`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub android_version: u32,
    pub android_release: String,
    pub dpi: String,
    pub resolution: String,
    pub manufacturer: String,
    pub device: String,
    pub model: String,
    pub cpu: String,
    pub app_version: String,
    pub version_code: String,
    pub bloks_versioning_id: String,
}

impl Default for Device {
    fn default() -> Self {
        Self {
            android_version: 34,
            android_release: "14".into(),
            dpi: "480dpi".into(),
            resolution: "1344x2992".into(),
            manufacturer: "Google/google".into(),
            device: "husky".into(),
            model: "Pixel 8 Pro".into(),
            cpu: "husky".into(),
            app_version: "449.0.0.52.84".into(),
            version_code: "1079242191".into(),
            bloks_versioning_id: "799db3e387d7a21a19c72918b8698b6cd28a1516ade363792c100ed45a0f9f0b"
                .into(),
        }
    }
}

impl Device {
    /// aiograpi `config.USER_AGENT_BASE`.
    pub fn user_agent(&self) -> String {
        format!(
            "Instagram {} Android ({}/{}; {}; {}; {}; {}; {}; {}; {}; {})",
            self.app_version,
            self.android_version,
            self.android_release,
            self.dpi,
            self.resolution,
            self.manufacturer,
            self.model,
            self.device,
            self.cpu,
            LOCALE,
            self.version_code
        )
    }
}

/// Everything needed to restore a client. `authorization` is the only secret.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// `Bearer IGT:2:<base64 json>` from the `ig-set-authorization` response header.
    pub authorization: String,
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub device: Device,
    pub uuid: String,
    pub phone_id: String,
    pub advertising_id: String,
    pub android_device_id: String,
    /// From `ig-set-x-mid`, sent back as `X-MID`.
    pub mid: String,
    /// From `ig-set-ig-u-rur`.
    pub ig_u_rur: String,
    /// From `x-ig-set-www-claim`.
    pub ig_www_claim: String,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let authorization = if self.authorization.is_empty() {
            ""
        } else {
            "<redacted>"
        };
        f.debug_struct("Session")
            .field("authorization", &authorization)
            .field("user_id", &self.user_id)
            .field("username", &self.username)
            .field("device", &self.device)
            .field("uuid", &self.uuid)
            .field("phone_id", &self.phone_id)
            .field("advertising_id", &self.advertising_id)
            .field("android_device_id", &self.android_device_id)
            .field("mid", &self.mid)
            .field("ig_u_rur", &self.ig_u_rur)
            .field("ig_www_claim", &self.ig_www_claim)
            .finish()
    }
}

impl Session {
    /// aiograpi `set_uuids({})` with the default device.
    fn fresh() -> Self {
        Self {
            authorization: String::new(),
            user_id: None,
            username: None,
            device: Device::default(),
            uuid: uuid4(),
            phone_id: uuid4(),
            advertising_id: uuid4(),
            android_device_id: android_device_id(),
            mid: String::new(),
            ig_u_rur: String::new(),
            ig_www_claim: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub user_id: String,
    pub username: String,
    pub full_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TwoFactorMethod {
    Sms,
    Totp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TwoFactor {
    pub identifier: String,
    pub username: String,
    pub method: TwoFactorMethod,
    /// Obfuscated phone number, when Instagram sends one.
    pub hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Login {
    Done(Account),
    TwoFactor(TwoFactor),
}

pub struct Reel<'a> {
    /// MP4, H.264/AAC.
    pub video: &'a Path,
    /// JPEG cover, sent as-is (aiograpi resizes it with PIL first).
    pub cover: &'a Path,
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
    pub caption: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Media {
    pub pk: String,
    pub code: String,
    /// `https://www.instagram.com/reel/<code>/`
    pub url: String,
}

pub struct Client {
    http: reqwest::Client,
    /// TLS 1.3 / HTTP/2 client shaped like the Android app, for the CAA login.
    caa_http: reqwest::Client,
    jar: Arc<Jar>,
    base: String,
    state: Mutex<Session>,
    pigeon_session_id: String,
    configure_delay: Duration,
}

impl Client {
    /// Fresh device identity (random uuids, aiograpi's default Android device profile).
    pub fn new() -> Result<Self, Error> {
        Self::from_session(Session::fresh())
    }

    /// Restore a previously exported session; no network call.
    pub fn from_session(session: Session) -> Result<Self, Error> {
        let jar = Arc::new(Jar::default());
        let http = reqwest::Client::builder()
            .cookie_provider(jar.clone())
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Transport(e.to_string()))?;
        Ok(Self {
            http,
            caa_http: caa::caa_http()?,
            jar,
            base: API_ORIGIN.into(),
            state: Mutex::new(session),
            pigeon_session_id: format!("UFS-{}-1", uuid4()),
            configure_delay: CONFIGURE_DELAY,
        })
    }

    /// Override the API origin (https://i.instagram.com by default).
    pub fn with_base_url(mut self, base: &str) -> Self {
        self.base = base.trim_end_matches('/').to_string();
        self
    }

    /// Pause between Reel configure attempts (10 s by default, as in aiograpi).
    pub fn with_configure_delay(mut self, delay: Duration) -> Self {
        self.configure_delay = delay;
        self
    }

    /// Everything needed to restore the client later.
    pub fn session(&self) -> Session {
        self.state().clone()
    }

    /// Signs in with Instagram's current CAA flow; only when that cannot even
    /// start (endpoint missing or protocol mismatch) does the legacy flow run.
    pub async fn login(&mut self, username: &str, password: &str) -> Result<Login, Error> {
        let username = username.trim();
        if username.is_empty() || password.is_empty() {
            return Err(Error::BadCredentials);
        }
        match self.login_caa(username, password).await {
            Ok(account) => Ok(Login::Done(account)),
            Err(error @ (Error::Protocol(_) | Error::Rejected(_))) => {
                // Report the current flow's reason unless the old one still works.
                self.login_legacy(username, password)
                    .await
                    .map_err(|_| error)
            }
            Err(error) => Err(error),
        }
    }

    /// aiograpi `login_legacy`: launcher/sync for the password key, then accounts/login/.
    async fn login_legacy(&mut self, username: &str, password: &str) -> Result<Login, Error> {
        let username = username.trim();
        if username.is_empty() || password.is_empty() {
            return Err(Error::BadCredentials);
        }
        {
            // A new login replaces any previous account state.
            let mut s = self.state();
            s.authorization.clear();
            s.user_id = None;
            s.ig_u_rur.clear();
            s.ig_www_claim.clear();
            s.username = Some(username.to_string());
        }
        let s = self.session();

        // pre_login_flow + password_publickeys both post launcher/sync/; one call is enough
        // because only the response headers are needed.
        let (status, headers, body) = self
            .api(
                Method::POST,
                "launcher/sync/",
                Some(&json!({"id": s.uuid, "server_config_retrieval": "1"})),
            )
            .await?;
        let header = |k: &str| headers.get(k).and_then(|v| v.to_str().ok());
        let key = header("ig-set-password-encryption-key-id")
            .and_then(|v| v.trim().parse::<u8>().ok())
            .zip(header("ig-set-password-encryption-pub-key"));
        let Some((key_id, public_key)) = key else {
            classify(status, &body)?;
            return Err(Error::Protocol(
                "Instagram did not return the password encryption key".into(),
            ));
        };
        let enc_password = crypto::encrypt_password(password, key_id, public_key, unix_secs())?;

        let data = json!({
            "jazoest": jazoest(&s.phone_id),
            "country_codes": format!(r#"[{{"country_code":"{COUNTRY_CODE}","source":["default"]}}]"#),
            "phone_id": s.phone_id,
            "enc_password": enc_password,
            "username": username,
            "adid": s.advertising_id,
            "guid": s.uuid,
            "device_id": s.android_device_id,
            "google_tokens": "[]",
            "login_attempt_count": "0",
        });
        let (status, _, body) = self
            .api(Method::POST, "accounts/login/", Some(&data))
            .await?;
        if status == 400
            && let Some(two_factor) = two_factor_from(&body, username)
        {
            return Ok(Login::TwoFactor(two_factor));
        }
        let json = classify(status, &body)?;
        self.finish_login(&json).map(Login::Done)
    }

    /// aiograpi `login_legacy` two-factor branch: accounts/two_factor_login/.
    pub async fn two_factor(
        &mut self,
        challenge: &TwoFactor,
        code: &str,
    ) -> Result<Account, Error> {
        let s = self.session();
        let data = json!({
            "verification_code": code.trim(),
            "phone_id": s.phone_id,
            "_csrftoken": self.csrftoken(),
            "two_factor_identifier": challenge.identifier,
            "username": challenge.username,
            "trust_this_device": "0",
            "guid": s.uuid,
            "device_id": s.android_device_id,
            "waterfall_id": uuid4(),
            "verification_method": "3",
        });
        let (status, _, body) = self
            .api(Method::POST, "accounts/two_factor_login/", Some(&data))
            .await?;
        let json = match classify(status, &body) {
            Err(Error::Rejected(m))
                if status == 400 && !m.eq_ignore_ascii_case("invalid parameters") =>
            {
                return Err(Error::TwoFactorCodeInvalid);
            }
            other => other?,
        };
        self.state().username = Some(challenge.username.clone());
        self.finish_login(&json)
    }

    /// aiograpi `account_info`: accounts/current_user/?edit=true.
    pub async fn account(&self) -> Result<Account, Error> {
        self.require_login()?;
        let json = self
            .call(Method::GET, "accounts/current_user/?edit=true", None)
            .await?;
        account_from(json.get("user").unwrap_or(&Value::Null))
    }

    /// aiograpi `clip_upload` + `clip_configure` without music, location, usertags or cross-posting.
    pub async fn upload_reel(&self, reel: &Reel<'_>) -> Result<Media, Error> {
        let user_id = self.require_login()?;
        let file_err = |e: std::io::Error| Error::File(format!("{}: {e}", reel.video.display()));
        let clip_len = tokio::fs::metadata(reel.video)
            .await
            .map_err(file_err)?
            .len();
        let cover = tokio::fs::read(reel.cover)
            .await
            .map_err(|e| Error::File(format!("{}: {e}", reel.cover.display())))?;

        let upload_id = unix_millis().to_string();
        let (width, height, duration_ms) = (reel.width, reel.height, reel.duration_ms);
        let duration_secs = duration_ms as f64 / 1000.0;
        let composer_session_id = uuid4();
        let asset_id = uuid::Uuid::new_v4().simple().to_string()[..12].to_uppercase();
        let upload_name = format!(
            "{}-0-{clip_len}-{upload_id}-{upload_id}",
            uuid::Uuid::new_v4().simple()
        );

        let mut context = json!({
            "source_attribution": null,
            "enable_video_dimension_upscale": false,
            "source_type": "clips",
            "quality": "",
        });
        if let Ok(target) = user_id.parse::<u64>() {
            context["target_id"] = json!(target);
        }
        let upload_settings = json!({
            "composer_session_id": composer_session_id,
            "upload_setting_properties": {
                "upload_settings_version": "v0.1",
                "codec": {},
                "context": context,
                "video": {
                    "video_height": height,
                    "video_gop_size_sec": 0,
                    "video_rotation_angle": 0,
                    "video_width": width,
                    "source_video_codec": null,
                    "video_partial_frame_size_bytes": 0,
                    "asset_id": asset_id,
                    "video_key_frame_size_bytes": 0,
                    "target_duration": duration_ms / 1000,
                    "video_original_file_size": clip_len,
                    "video_duration_milliseconds": duration_ms,
                    "audio_bit_rate_bps": -1,
                    "video_bit_rate_bps": 0,
                    "audio_codec_type": null,
                    "video_fps": 30,
                },
                "creative_tools": {"transmuxing_eligible": false, "transcoding_required": true},
                "network": {
                    "download_latency_connection_quality": "ig_dummy",
                    "network_connection_name": "ig_dummy",
                    "download_bandwidth_connection_quality": "ig_dummy",
                },
            },
            "preview_spec": {"spec_version": 1, "video_dur_ms": duration_ms, "audio_dur_ms": duration_ms},
        })
        .to_string();
        let response = self
            .request(
                Method::POST,
                &format!("{}/upload_settings/{composer_session_id}", self.base),
            )
            .header("Content-Type", "application/json")
            .header("Offset", "0")
            .header("X-Entity-Length", upload_settings.len().to_string())
            .header("X-Entity-Name", "upload_settings")
            .header("X-Entity-Type", "application/json")
            .header(
                "X_FB_VIDEO_WATERFALL_ID",
                format!("{composer_session_id}_settings"),
            )
            .body(upload_settings)
            .send()
            .await;
        self.expect_200("upload_settings", response).await?;

        let rupload_params = json!({
            "provenance_metadata": r#"{"origin": ["EXTERNAL"]}"#,
            "upload_media_height": height.to_string(),
            "share_type": "reels",
            "debug_segment_id": "0",
            "extract_cover_frame": "1",
            "upload_engine_config_enum": "0",
            "xsharing_user_ids": "[]",
            "upload_media_width": width.to_string(),
            "stella_data": "{}",
            "is_clips_video": "1",
            "is_optimistic_upload": "true",
            "upload_media_duration_ms": duration_ms.to_string(),
            "content_tags": "use_default_cover",
            "upload_id": upload_id,
            "retry_context": r#"{"num_reupload":0,"num_step_auto_retry":0,"num_step_manual_retry":0}"#,
            "session_id": upload_id,
            "media_type": "2",
        })
        .to_string();
        let video_url = format!("{}/rupload_igvideo/{upload_name}", self.base);
        let rupload = |method| {
            self.request(method, &video_url)
                .header("X-Instagram-Rupload-Params", rupload_params.as_str())
                .header(
                    "X_FB_VIDEO_WATERFALL_ID",
                    format!("{composer_session_id}_{asset_id}_Mixed_0"),
                )
                .header("X-Entity-Type", "video/mp4")
                .header("Segment-Start-Offset", "0")
                .header("Segment-Type", "3")
        };
        let response = rupload(Method::GET).send().await;
        self.expect_200("rupload_init", response).await?;

        let file = tokio::fs::File::open(reel.video).await.map_err(file_err)?;
        let response = rupload(Method::POST)
            .header("Offset", "0")
            .header("X-Entity-Name", upload_name.as_str())
            .header("X-Entity-Length", clip_len.to_string())
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", clip_len.to_string())
            .body(reqwest::Body::from(file))
            .send()
            .await;
        self.expect_200("rupload_upload", response).await?;

        let s = self.session();
        let data = json!({
            "_uuid": s.uuid,
            "device_id": s.android_device_id,
            "filter_type": "0",
            "timezone_offset": TIMEZONE_OFFSET.to_string(),
            "location": "{}",
            "source_type": "4",
            "caption": reel.caption,
            "usertags": r#"{"in": []}"#,
            "clips_share_preview_to_feed": "1",
            "upload_id": upload_id,
            "device": {
                "manufacturer": s.device.manufacturer,
                "model": s.device.model,
                "android_version": s.device.android_version,
                "android_release": s.device.android_release,
            },
            "length": duration_secs,
            "clips_segments_metadata": {
                "num_segments": 1,
                "clips_segments": [{
                    "index": 0,
                    "face_effect_id": null,
                    "speed": 100,
                    "source_type": 4,
                    "duration_ms": duration_ms,
                    "audio_type": "original",
                    "from_draft": 0,
                    "camera_position": -1,
                    "media_folder": null,
                    "media_type": "video",
                    "original_media_type": 2,
                    "original_segment_hash": null,
                    "is_remix": false,
                    "has_reusable_template_asset": false,
                }],
            },
            "clips_audio_metadata": {"original": {"volume_level": 1.0}},
            "additional_audio_info": {"has_voiceover_attribution": 0},
            "edits": {"filter_type": 0, "filter_strength": 1.0},
            "capture_type": "clips_v2",
            "clips_creation_entry_point": "clips",
            "camera_entry_point": "71",
            "audience": "default",
            "include_e2ee_mentioned_user_list": "1",
            "hide_from_profile_grid": "false",
            "original_width": width,
            "original_height": height,
            "extra": {"source_width": width, "source_height": height},
            "audio_muted": false,
            "poster_frame_index": 70,
        });

        for _ in 0..CONFIGURE_ATTEMPTS {
            tokio::time::sleep(self.configure_delay).await;
            // aiograpi clip_configure re-uploads the cover on every attempt.
            self.upload_cover(&upload_id, &cover).await?;
            let (status, _, body) = self
                .api(
                    Method::POST,
                    "media/configure_to_clips/?video=1",
                    Some(&data),
                )
                .await?;
            if body.contains("Transcode not finished yet") {
                tokio::time::sleep(self.configure_delay).await;
                continue;
            }
            let json = classify(status, &body)?;
            let media = json.get("media").ok_or_else(|| {
                Error::Protocol("Reel configure succeeded without media payload".into())
            })?;
            let pk = media.get("pk").and_then(id_str);
            let code = media.get("code").and_then(Value::as_str);
            let (Some(pk), Some(code)) = (pk, code) else {
                return Err(Error::Protocol("Reel media without pk or code".into()));
            };
            return Ok(Media {
                url: format!("https://www.instagram.com/reel/{code}/"),
                pk,
                code: code.to_string(),
            });
        }
        Err(Error::Rejected(
            "Instagram did not finish processing the video in time".into(),
        ))
    }

    /// aiograpi `photo_rupload(thumbnail, upload_id, for_story=True)` without the PIL resize.
    async fn upload_cover(&self, upload_id: &str, cover: &[u8]) -> Result<(), Error> {
        let name = format!(
            "{upload_id}_0_{}",
            rand::thread_rng().gen_range(1_000_000_000u64..=9_999_999_999)
        );
        let params = json!({
            "retry_context": r#"{"num_step_auto_retry":0,"num_reupload":0,"num_step_manual_retry":0}"#,
            "media_type": "1",
            "xsharing_user_ids": "[]",
            "upload_id": upload_id,
            "image_compression": r#"{"lib_name": "moz", "lib_version": "3.1.m", "quality": "80"}"#,
        })
        .to_string();
        let response = self
            .request(
                Method::POST,
                &format!("{}/rupload_igphoto/{name}", self.base),
            )
            .header("X-Instagram-Rupload-Params", params)
            .header("X_FB_PHOTO_WATERFALL_ID", uuid4())
            .header("X-Entity-Type", "image/jpeg")
            .header("Offset", "0")
            .header("X-Entity-Name", name.as_str())
            .header("X-Entity-Length", cover.len().to_string())
            .header("Content-Type", "application/octet-stream")
            .body(cover.to_vec())
            .send()
            .await;
        self.expect_200("rupload_igphoto", response).await
    }

    async fn expect_200(
        &self,
        stage: &str,
        response: reqwest::Result<reqwest::Response>,
    ) -> Result<(), Error> {
        let response = response.map_err(transport)?;
        self.absorb(response.headers());
        let status = response.status().as_u16();
        if status == 200 {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        Err(match classify(status, &body) {
            Err(e) => e,
            Ok(_) => Error::Rejected(format!("Reel upload failed during {stage}: HTTP {status}")),
        })
    }

    async fn call(
        &self,
        method: Method,
        endpoint: &str,
        data: Option<&Value>,
    ) -> Result<Value, Error> {
        let (status, _, body) = self.api(method, endpoint, data).await?;
        classify(status, &body)
    }

    /// aiograpi `_send_private_request`: signed form POST or GET under `/api/v1/`.
    async fn api(
        &self,
        method: Method,
        endpoint: &str,
        data: Option<&Value>,
    ) -> Result<(u16, HeaderMap, String), Error> {
        let url = format!("{}/api/v1/{endpoint}", self.base);
        let mut request = self.request(method, &url).timeout(READ_TIMEOUT);
        if let Some(data) = data {
            request = request
                .header(
                    "Content-Type",
                    "application/x-www-form-urlencoded; charset=UTF-8",
                )
                .body(crypto::signed_body(data));
        }
        let response = request.send().await.map_err(transport)?;
        self.absorb(response.headers());
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.text().await.map_err(transport)?;
        Ok((status, headers, body))
    }

    /// aiograpi `base_headers` + `private_headers` (Authorization when present).
    fn request(&self, method: Method, url: &str) -> RequestBuilder {
        self.request_with(&self.http, method, url)
    }

    fn request_with(&self, http: &reqwest::Client, method: Method, url: &str) -> RequestBuilder {
        let s = self.session();
        let now = unix_secs_f64();
        let mut r = http
            .request(method, url)
            .header("X-IG-App-Locale", LOCALE)
            .header("X-IG-Device-Locale", LOCALE)
            .header("X-IG-Mapped-Locale", LOCALE)
            .header("X-Pigeon-Session-Id", self.pigeon_session_id.as_str())
            .header("X-Pigeon-Rawclienttime", format!("{now:.3}"))
            .header("X-IG-Bandwidth-Speed-KBPS", "-1.000")
            .header("X-IG-Bandwidth-TotalBytes-B", "0")
            .header("X-IG-Bandwidth-TotalTime-MS", "0")
            .header("X-IG-App-Startup-Country", COUNTRY)
            .header("X-Bloks-Version-Id", s.device.bloks_versioning_id.as_str())
            .header("X-Bloks-Is-Layout-RTL", "false")
            .header("X-Bloks-Is-Panorama-Enabled", "true")
            .header("X-IG-Device-ID", s.uuid.as_str())
            .header("X-IG-Family-Device-ID", s.phone_id.as_str())
            .header("X-IG-Android-ID", s.android_device_id.as_str())
            .header("X-IG-Timezone-Offset", TIMEZONE_OFFSET.to_string())
            .header("x-fb-connection-type", "WIFI")
            .header("X-IG-Connection-Type", "WIFI")
            .header("X-IG-Capabilities", "3brTv10=")
            .header("X-IG-App-ID", APP_ID)
            .header("Priority", "u=3")
            .header("User-Agent", s.device.user_agent())
            .header("Accept-Language", "en-US")
            .header("X-MID", s.mid.as_str())
            .header("X-FB-HTTP-Engine", "Tigon/MNS/TCP")
            .header("X-Tigon-Is-Retry", "False")
            .header("X-Zero-Balance", "INIT")
            .header("X-Zero-Eh", "")
            .header("X-Zero-State", "unknown")
            .header("Zero-HTTP-Network-Interface", "wifi")
            .header("X-FB-Client-IP", "True")
            .header("X-FB-Server-Cluster", "True")
            .header("IG-INTENDED-USER-ID", s.user_id.as_deref().unwrap_or("0"))
            .header(
                "X-IG-Nav-Chain",
                "9MV:self_profile:2,ProfileMediaTabFragment:self_profile:3,9Xf:self_following:4",
            )
            .header(
                "X-IG-WWW-Claim",
                if s.ig_www_claim.is_empty() {
                    "0"
                } else {
                    &s.ig_www_claim
                },
            );
        if let Some(uid) = &s.user_id {
            let next_year = now + 31_536_000.0;
            r = r
                .header("IG-U-DS-USER-ID", uid.as_str())
                .header(
                    "IG-U-IG-DIRECT-REGION-HINT",
                    format!("LLA,{uid},{next_year}:01f7bae7d8b131877d8e0ae1493252280d72f6d0d554447cb1dc9049b6b2c507c08605b7"),
                )
                .header(
                    "IG-U-SHBID",
                    format!("12695,{uid},{next_year}:01f778d9c9f7546cf3722578fbf9b85143cd6e5132723e5c93f40f55ca0459c8ef8a0d9f"),
                )
                .header(
                    "IG-U-SHBTS",
                    format!("{},{uid},{next_year}:01f7ace11925d0388080078d0282b75b8059844855da27e23c90a362270fddfb3fae7e28", now as u64),
                );
            if s.ig_u_rur.is_empty() {
                r = r.header(
                    "IG-U-RUR",
                    format!("RVA,{uid},{next_year}:01f7f627f9ae4ce2874b2e04463efdb184340968b1b006fa88cb4cc69a942a04201e544c"),
                );
            }
        }
        if !s.ig_u_rur.is_empty() {
            r = r.header("IG-U-RUR", format!("\"{}\"", s.ig_u_rur));
        }
        if !s.authorization.is_empty() {
            r = r.header("Authorization", s.authorization.as_str());
        }
        r
    }

    /// `ig-set-*` response headers (aiograpi private.py and bloks.py).
    fn absorb(&self, headers: &HeaderMap) {
        let get = |k: &str| {
            headers
                .get(k)
                .and_then(|v| v.to_str().ok())
                .filter(|v| !v.is_empty())
        };
        let mut s = self.state();
        if let Some(mid) = get("ig-set-x-mid") {
            s.mid = mid.to_string();
        }
        if let Some(authorization) = get("ig-set-authorization")
            && let Some(data) = parse_authorization(authorization)
        {
            s.authorization = authorization.to_string();
            if let Some(uid) = data.get("ds_user_id").and_then(id_str) {
                s.user_id = Some(uid);
            }
        }
        if let Some(rur) = get("ig-set-ig-u-rur") {
            s.ig_u_rur = rur.to_string();
        }
        if let Some(claim) = get("x-ig-set-www-claim") {
            s.ig_www_claim = claim.to_string();
        }
    }

    fn finish_login(&self, json: &Value) -> Result<Account, Error> {
        let account = account_from(json.get("logged_in_user").unwrap_or(&Value::Null))?;
        let mut s = self.state();
        if s.authorization.is_empty() {
            return Err(Error::Protocol(
                "login succeeded but Instagram returned no session token".into(),
            ));
        }
        s.user_id = Some(account.user_id.clone());
        s.username = Some(account.username.clone());
        Ok(account)
    }

    fn require_login(&self) -> Result<String, Error> {
        let s = self.state();
        match &s.user_id {
            Some(uid) if !s.authorization.is_empty() => Ok(uid.clone()),
            _ => Err(Error::LoginRequired),
        }
    }

    /// aiograpi `token`: csrftoken cookie, else a random 64-char token.
    fn csrftoken(&self) -> String {
        let url = self.base.parse().ok();
        let cookies = url.and_then(|u| self.jar.cookies(&u));
        let from_cookie = cookies
            .as_ref()
            .and_then(|c| c.to_str().ok())
            .and_then(|c| {
                c.split("; ")
                    .find_map(|kv| kv.strip_prefix("csrftoken="))
                    .map(str::to_string)
            });
        from_cookie.unwrap_or_else(|| {
            rand::thread_rng()
                .sample_iter(&rand::distributions::Alphanumeric)
                .take(64)
                .map(char::from)
                .collect()
        })
    }

    fn state(&self) -> MutexGuard<'_, Session> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn two_factor_from(body: &str, username: &str) -> Option<TwoFactor> {
    let json: Value = serde_json::from_str(body).ok()?;
    let info = json.get("two_factor_info")?;
    let identifier = info.get("two_factor_identifier")?.as_str()?.to_string();
    let on = |k: &str| match info.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64() != Some(0),
        Some(Value::String(s)) => matches!(s.trim().to_lowercase().as_str(), "1" | "true" | "yes"),
        _ => false,
    };
    // aiograpi `_infer_bloks_two_factor_challenge`.
    let method = if on("sms_two_factor_on") && !on("totp_two_factor_on") {
        TwoFactorMethod::Sms
    } else {
        TwoFactorMethod::Totp
    };
    Some(TwoFactor {
        identifier,
        username: info
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or(username)
            .to_string(),
        method,
        hint: info
            .get("obfuscated_phone_number")
            .and_then(id_str)
            .filter(|h| !h.is_empty()),
    })
}

fn account_from(user: &Value) -> Result<Account, Error> {
    let user_id = user
        .get("pk")
        .and_then(id_str)
        .ok_or_else(|| Error::Protocol("account without pk".into()))?;
    let text = |k: &str| {
        user.get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    Ok(Account {
        user_id,
        username: text("username"),
        full_name: text("full_name"),
    })
}

/// aiograpi `parse_authorization`: base64 JSON after the last ':'.
fn parse_authorization(authorization: &str) -> Option<serde_json::Map<String, Value>> {
    let b64 = authorization.rsplit(':').next().filter(|p| !p.is_empty())?;
    serde_json::from_slice(&STANDARD.decode(b64).ok()?).ok()
}

fn id_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn transport(e: reqwest::Error) -> Error {
    Error::Transport(e.without_url().to_string())
}

fn uuid4() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// aiograpi `generate_android_device_id`.
fn android_device_id() -> String {
    let digest = Sha256::digest(unix_secs_f64().to_string());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("android-{}", &hex[..16])
}

/// aiograpi `utils.auth.generate_jazoest`.
fn jazoest(symbols: &str) -> String {
    format!("2{}", symbols.chars().map(|c| c as u32).sum::<u32>())
}

fn unix_secs_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn unix_secs() -> u64 {
    unix_secs_f64() as u64
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_round_trips_and_redacts() {
        let mut session = Session::fresh();
        session.authorization = format!(
            "Bearer IGT:2:{}",
            STANDARD.encode(r#"{"ds_user_id":"7","sessionid":"7%3Asecret"}"#)
        );
        session.user_id = Some("7".into());
        let restored: Session =
            serde_json::from_str(&serde_json::to_string(&session).unwrap()).unwrap();
        assert_eq!(restored, session);
        assert_eq!(Client::from_session(restored).unwrap().session(), session);
        let debug = format!("{session:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("IGT:2"));
        assert!(!debug.contains(&session.authorization));
    }

    #[test]
    fn identity_helpers_match_aiograpi() {
        assert_eq!(jazoest("ab"), "2195");
        let id = android_device_id();
        assert!(id.starts_with("android-") && id.len() == 24);
        assert_eq!(
            Device::default().user_agent(),
            "Instagram 449.0.0.52.84 Android (34/14; 480dpi; 1344x2992; Google/google; Pixel 8 Pro; husky; husky; en_US; 1079242191)"
        );
        let auth = format!("Bearer IGT:2:{}", STANDARD.encode(r#"{"ds_user_id":12}"#));
        assert_eq!(
            parse_authorization(&auth)
                .unwrap()
                .get("ds_user_id")
                .and_then(id_str),
            Some("12".into())
        );
        assert!(parse_authorization("Bearer IGT:2:").is_none());
    }

    #[test]
    fn two_factor_info_is_parsed() {
        let body = r#"{"message":"","two_factor_required":true,"two_factor_info":{"username":"bob","two_factor_identifier":"abc","sms_two_factor_on":true,"totp_two_factor_on":false,"obfuscated_phone_number":"1234"},"status":"fail"}"#;
        let tf = two_factor_from(body, "x").unwrap();
        assert_eq!(tf.identifier, "abc");
        assert_eq!(tf.username, "bob");
        assert_eq!(tf.method, TwoFactorMethod::Sms);
        assert_eq!(tf.hint.as_deref(), Some("1234"));
        let totp = r#"{"two_factor_info":{"two_factor_identifier":"z","sms_two_factor_on":true,"totp_two_factor_on":true}}"#;
        assert_eq!(
            two_factor_from(totp, "x").unwrap().method,
            TwoFactorMethod::Totp
        );
    }
}
