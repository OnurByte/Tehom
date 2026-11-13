//! Request signing for TikTok's upload service (`/top/v1`): the standard AWS
//! Signature V4 with the temporary credentials TikTok hands out. The site's
//! own request signatures (X-Bogus, _signature, ...) are deliberately not
//! implemented; those requests are made by the real browser page.
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
/// JavaScript's `encodeURIComponent`.
pub(crate) fn enc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("any key length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}
/// `YYYYMMDDTHHMMSSZ` for a Unix time.
fn amz_date(unix_seconds: u64) -> String {
    // Civil-from-days (Howard Hinnant), valid for the whole Unix era.
    let days = (unix_seconds / 86_400) as i64 + 719_468;
    let era = days / 146_097;
    let doe = days % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let rest = unix_seconds % 86_400;
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}
pub(crate) struct VodAuth {
    pub access_key: String,
    pub secret_key: String,
    pub session_token: String,
}
/// Signs a request to the upload service (`/top/v1`). Returns the query
/// string and the headers to send.
pub(crate) fn sign_vod(
    method: &str,
    query: &[(&str, &str)],
    body: &str,
    auth: &VodAuth,
    unix_seconds: u64,
) -> (String, Vec<(&'static str, String)>) {
    const REGION: &str = "ap-singapore-1";
    let mut query = query.to_vec();
    query.sort();
    let query = query
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&");
    let date = amz_date(unix_seconds);
    let day = &date[..8];
    let payload_hash = hex(&<Sha256 as sha2::Digest>::digest(body.as_bytes()));
    let mut headers = vec![
        ("x-amz-date", date.clone()),
        ("x-amz-security-token", auth.session_token.clone()),
    ];
    if !body.is_empty() {
        headers.insert(0, ("x-amz-content-sha256", payload_hash.clone()));
    }
    let canonical_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{k}:{}\n", v.trim()))
        .collect();
    let signed = headers
        .iter()
        .map(|(k, _)| *k)
        .collect::<Vec<_>>()
        .join(";");
    let request =
        format!("{method}\n/top/v1\n{query}\n{canonical_headers}\n{signed}\n{payload_hash}");
    let scope = format!("{day}/{REGION}/vod/aws4_request");
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{date}\n{scope}\n{}",
        hex(&<Sha256 as sha2::Digest>::digest(request.as_bytes()))
    );
    let mut key = hmac(format!("AWS4{}", auth.secret_key).as_bytes(), day);
    for part in [REGION, "vod", "aws4_request"] {
        key = hmac(&key, part);
    }
    headers.push((
        "authorization",
        format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={}",
            auth.access_key,
            hex(&hmac(&key, &to_sign))
        ),
    ));
    (query, headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amz_date_formats_known_times() {
        assert_eq!(amz_date(0), "19700101T000000Z");
        assert_eq!(amz_date(1_709_210_096), "20240229T123456Z");
    }
    /// AWS's published Signature Version 4 key-derivation example.
    #[test]
    fn signing_key_matches_the_aws_example() {
        let mut key = hmac(b"AWS4wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY", "20120215");
        for part in ["us-east-1", "iam", "aws4_request"] {
            key = hmac(&key, part);
        }
        assert_eq!(
            hex(&key),
            "f4780e2d9f65fa895f9c67b32ce1baf0b0d8a43505a000a1a9e090d414db404d"
        );
    }
    #[test]
    fn vod_signature_covers_the_body_hash_only_when_there_is_a_body() {
        let auth = VodAuth {
            access_key: "AK".into(),
            secret_key: "SK".into(),
            session_token: "ST".into(),
        };
        let (query, get) = sign_vod(
            "GET",
            &[("Version", "2020-11-19"), ("Action", "X Y")],
            "",
            &auth,
            0,
        );
        assert_eq!(query, "Action=X%20Y&Version=2020-11-19");
        assert!(
            get.last()
                .unwrap()
                .1
                .contains("SignedHeaders=x-amz-date;x-amz-security-token,")
        );
        let (_, post) = sign_vod("POST", &[], "{}", &auth, 0);
        assert!(
            post.last()
                .unwrap()
                .1
                .contains("SignedHeaders=x-amz-content-sha256;x-amz-date;")
        );
    }
}
