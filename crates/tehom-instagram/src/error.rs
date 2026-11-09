use std::fmt;

use serde_json::Value;

/// Failures surfaced to the app. Messages are written for the end user and never contain secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    BadCredentials,
    TwoFactorCodeInvalid,
    /// Challenge/checkpoint required: the user must approve in the Instagram app.
    Checkpoint(String),
    /// Session expired or was revoked.
    LoginRequired,
    RateLimited,
    /// feedback_required and other server refusals; the text is meant for the user.
    Rejected(String),
    /// Instagram answered with a 5xx status.
    Server(u16),
    Transport(String),
    Protocol(String),
    /// A local media file could not be read.
    File(String),
}

impl Error {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Error::Transport(_) | Error::RateLimited | Error::Server(_)
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadCredentials => f.write_str(
                "Instagram rejected the username or password. This can also happen when Instagram \
                 distrusts the network or device, even if the password is correct.",
            ),
            Error::TwoFactorCodeInvalid => f.write_str("The two-factor code was not accepted."),
            Error::Checkpoint(m) => write!(
                f,
                "Instagram wants you to confirm this sign-in in the Instagram app ({m})."
            ),
            Error::LoginRequired => {
                f.write_str("The Instagram session has expired. Please sign in again.")
            }
            Error::RateLimited => f.write_str(
                "Instagram is limiting requests right now. Wait a few minutes and try again.",
            ),
            Error::Rejected(m) => write!(f, "Instagram refused the request: {m}"),
            Error::Server(code) => write!(
                f,
                "Instagram had a server problem (HTTP {code}). Try again later."
            ),
            Error::Transport(m) => write!(f, "Could not reach Instagram: {m}"),
            Error::Protocol(m) => write!(f, "Unexpected response from Instagram: {m}"),
            Error::File(m) => write!(f, "Could not read the media file: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Port of aiograpi `PrivateRequestMixin._send_private_request` error handling.
/// Returns the parsed JSON body when the response is a success.
pub(crate) fn classify(status: u16, body: &str) -> Result<Value, Error> {
    let json: Option<Value> = serde_json::from_str(body).ok();
    let field = |k: &str| {
        json.as_ref()
            .and_then(|j| j.get(k))
            .and_then(Value::as_str)
            .unwrap_or("")
    };
    let message = field("message");
    let error_type = field("error_type");

    if (200..300).contains(&status) {
        let Some(parsed) = json.as_ref() else {
            return Err(Error::Protocol("the response was not JSON".into()));
        };
        if field("status") == "fail" {
            return Err(Error::Rejected(or(message, "the request failed")));
        }
        if let Some(title) = parsed.get("error_title") {
            return Err(Error::Rejected(
                title.as_str().unwrap_or(message).to_string(),
            ));
        }
        return Ok(parsed.clone());
    }

    if message.contains("Please wait a few minutes") {
        return Err(Error::RateLimited);
    }
    Err(match status {
        403 if message == "login_required" => Error::LoginRequired,
        403 => Error::Rejected(or(message, &short(body))),
        400 => {
            let has_two_factor = json.as_ref().is_some_and(|j| {
                j.get("two_factor_info")
                    .is_some_and(|v| !v.is_null() && v != &Value::Bool(false))
            });
            if has_two_factor || error_type == "two_factor_required" {
                Error::Rejected("two-factor authentication required".into())
            } else if message == "challenge_required" {
                let url = json
                    .as_ref()
                    .and_then(|j| j.pointer("/challenge/url"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if url.contains("/suspended/") {
                    Error::Checkpoint("account suspended".into())
                } else {
                    Error::Checkpoint("challenge_required".into())
                }
            } else if message == "feedback_required" {
                Error::Rejected(format!("feedback_required: {}", field("feedback_message")))
            } else if matches!(
                message,
                "consent_required" | "geoblock_required" | "checkpoint_required"
            ) {
                Error::Checkpoint(message.into())
            } else if error_type == "sentry_block" {
                Error::Rejected(or(message, "sentry_block"))
            } else if error_type == "rate_limit_error" {
                Error::RateLimited
            } else if error_type == "bad_password" || message.contains("The username you entered") {
                Error::BadCredentials
            } else if error_type == "needs_upgrade" {
                Error::Rejected("Instagram requires a newer app version (needs_upgrade)".into())
            } else if message.contains("VideoTooLongException") {
                Error::Rejected("the video is too long".into())
            } else {
                Error::Rejected(or(message, or(error_type, "bad request").as_str()))
            }
        }
        401 => Error::LoginRequired,
        429 => Error::RateLimited,
        404 if body.trim() == "Not Found" => Error::Checkpoint("challenge_required".into()),
        408 => Error::Transport("request timed out (HTTP 408)".into()),
        500..=599 => Error::Server(status),
        _ => Error::Protocol(format!("HTTP {status}")),
    })
}

fn or(s: &str, fallback: &str) -> String {
    if s.is_empty() { fallback } else { s }.to_string()
}

fn short(body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return "forbidden".into();
    }
    body.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_aiograpi_error_bodies() {
        let c = |s, b: &str| classify(s, b).unwrap_err();
        assert_eq!(
            c(
                400,
                r#"{"message":"The password you entered is incorrect.","error_type":"bad_password","status":"fail"}"#
            ),
            Error::BadCredentials
        );
        assert_eq!(
            c(
                403,
                r#"{"message":"login_required","logout_reason":2,"status":"fail"}"#
            ),
            Error::LoginRequired
        );
        assert_eq!(
            c(
                400,
                r#"{"message":"challenge_required","challenge":{"url":"https://i.instagram.com/challenge/1/abc/","api_path":"/challenge/1/abc/"},"status":"fail"}"#
            ),
            Error::Checkpoint("challenge_required".into())
        );
        assert_eq!(
            c(
                400,
                r#"{"message":"checkpoint_required","checkpoint_url":"https://x","status":"fail"}"#
            ),
            Error::Checkpoint("checkpoint_required".into())
        );
        assert_eq!(
            c(
                400,
                r#"{"message":"feedback_required","feedback_message":"This action was blocked.","status":"fail"}"#
            ),
            Error::Rejected("feedback_required: This action was blocked.".into())
        );
        assert_eq!(
            c(
                400,
                r#"{"message":"Please wait a few minutes before you try again.","status":"fail"}"#
            ),
            Error::RateLimited
        );
        assert_eq!(c(429, "{}"), Error::RateLimited);
        assert_eq!(
            c(
                400,
                r#"{"message":"","error_type":"rate_limit_error","status":"fail"}"#
            ),
            Error::RateLimited
        );
        assert_eq!(c(502, "<html>Bad gateway</html>"), Error::Server(502));
        assert_eq!(
            c(404, "Not Found"),
            Error::Checkpoint("challenge_required".into())
        );
        assert_eq!(
            c(
                200,
                r#"{"message":"Transcode not finished yet.","status":"fail"}"#
            ),
            Error::Rejected("Transcode not finished yet.".into())
        );
        assert_eq!(
            c(
                200,
                r#"{"error_title":"bad image input extra:{}","message":"media_needs_reupload","status":"ok"}"#
            ),
            Error::Rejected("bad image input extra:{}".into())
        );
        assert!(matches!(c(200, "not json"), Error::Protocol(_)));
        assert!(
            classify(200, r#"{"status":"ok","user":{}}"#)
                .unwrap()
                .get("user")
                .is_some()
        );
    }

    #[test]
    fn retryable_variants() {
        assert!(Error::Transport("x".into()).is_retryable());
        assert!(Error::RateLimited.is_retryable());
        assert!(Error::Server(503).is_retryable());
        assert!(!Error::BadCredentials.is_retryable());
        assert!(!Error::Rejected("x".into()).is_retryable());
    }
}
