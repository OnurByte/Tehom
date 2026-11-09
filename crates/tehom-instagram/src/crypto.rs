use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::RngCore;
use rand::rngs::OsRng;
use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::pkcs8::DecodePublicKey;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey};
use serde_json::Value;

use crate::Error;

/// aiograpi `utils.serialization.dumps`: compact JSON, ASCII only (Python `ensure_ascii`).
pub(crate) fn dumps(data: &Value) -> String {
    let json = data.to_string();
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            // serde_json only emits non-ASCII inside strings, where \u escapes are valid.
            for unit in c.encode_utf16(&mut [0; 2]) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

/// aiograpi `utils.auth.generate_signature(dumps(data))`.
pub(crate) fn signed_body(data: &Value) -> String {
    format!("signed_body=SIGNATURE.{}", quote_plus(&dumps(data)))
}

/// Python `urllib.parse.quote_plus` with its default safe set.
fn quote_plus(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// aiograpi `PasswordMixin.password_encrypt`: `#PWD_INSTAGRAM:4:<ts>:<base64>`.
pub(crate) fn encrypt_password(
    password: &str,
    key_id: u8,
    public_key_b64: &str,
    timestamp: u64,
) -> Result<String, Error> {
    let bad_key = || Error::Protocol("invalid password encryption key".into());
    let pem = STANDARD
        .decode(public_key_b64.trim())
        .map_err(|_| bad_key())?;
    let pem = std::str::from_utf8(&pem).map_err(|_| bad_key())?;
    let key = RsaPublicKey::from_public_key_pem(pem)
        .or_else(|_| RsaPublicKey::from_pkcs1_pem(pem))
        .map_err(|_| bad_key())?;

    let mut session_key = [0u8; 32];
    let mut iv = [0u8; 12];
    OsRng.fill_bytes(&mut session_key);
    OsRng.fill_bytes(&mut iv);
    let timestamp = timestamp.to_string();

    let rsa_encrypted = key
        .encrypt(&mut OsRng, Pkcs1v15Encrypt, &session_key)
        .map_err(|_| bad_key())?;
    let sealed = Aes256Gcm::new_from_slice(&session_key)
        .expect("32-byte key")
        .encrypt(
            Nonce::from_slice(&iv),
            Payload {
                msg: password.as_bytes(),
                aad: timestamp.as_bytes(),
            },
        )
        .map_err(|_| Error::Protocol("password encryption failed".into()))?;
    let (aes_encrypted, tag) = sealed.split_at(sealed.len() - 16);

    let mut payload = vec![1u8, key_id];
    payload.extend_from_slice(&iv);
    payload.extend_from_slice(&(rsa_encrypted.len() as u16).to_le_bytes());
    payload.extend_from_slice(&rsa_encrypted);
    payload.extend_from_slice(tag);
    payload.extend_from_slice(aes_encrypted);
    Ok(format!(
        "#PWD_INSTAGRAM:4:{timestamp}:{}",
        STANDARD.encode(payload)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::RsaPrivateKey;
    use rsa::pkcs8::{EncodePublicKey, LineEnding};
    use serde_json::json;

    #[test]
    fn signs_like_aiograpi() {
        let body = signed_body(&json!({"a": "b c", "n": "é&="}));
        // {"a":"b c","n":"é&="} quoted with quote_plus
        assert_eq!(
            body,
            "signed_body=SIGNATURE.%7B%22a%22%3A%22b+c%22%2C%22n%22%3A%22%5Cu00e9%26%3D%22%7D"
        );
        assert_eq!(dumps(&json!("\u{1F600}")), "\"\\ud83d\\ude00\"");
    }

    #[test]
    fn password_envelope_decrypts() {
        let private = RsaPrivateKey::new(&mut OsRng, 1024).unwrap();
        let pem = private
            .to_public_key()
            .to_public_key_pem(LineEnding::LF)
            .unwrap();
        let enc = encrypt_password("hunter2 ü", 41, &STANDARD.encode(pem), 1_700_000_000).unwrap();

        let rest = enc.strip_prefix("#PWD_INSTAGRAM:4:1700000000:").unwrap();
        let raw = STANDARD.decode(rest).unwrap();
        assert_eq!(raw[0], 1);
        assert_eq!(raw[1], 41);
        let iv = &raw[2..14];
        let rsa_len = u16::from_le_bytes([raw[14], raw[15]]) as usize;
        assert_eq!(rsa_len, 128);
        let rsa_part = &raw[16..16 + rsa_len];
        let tag = &raw[16 + rsa_len..32 + rsa_len];
        let ciphertext = &raw[32 + rsa_len..];

        let session_key = private.decrypt(Pkcs1v15Encrypt, rsa_part).unwrap();
        let mut sealed = ciphertext.to_vec();
        sealed.extend_from_slice(tag);
        let plain = Aes256Gcm::new_from_slice(&session_key)
            .unwrap()
            .decrypt(
                Nonce::from_slice(iv),
                Payload {
                    msg: &sealed,
                    aad: b"1700000000",
                },
            )
            .unwrap();
        assert_eq!(plain, "hunter2 ü".as_bytes());
    }
}
