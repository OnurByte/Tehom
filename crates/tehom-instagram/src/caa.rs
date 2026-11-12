//! Instagram's current Android login: the Bloks "CAA" flow. The legacy
//! `accounts/login/` endpoint answers `needs_upgrade` for every app version.
//! Adapted from instagrapi `mixins/bloks.py` and `mixins/attestation.py` (MIT).
//! NOT validated against Instagram: it needs a live account to exercise.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use p256::pkcs8::EncodePublicKey;
use rand::rngs::OsRng;
use reqwest::Method;
use reqwest::header::HeaderMap;
use serde_json::{Value, json};

use crate::crypto::{dumps, encrypt_password};
use crate::error::classify;
use crate::{
    API_ORIGIN, APP_ID, Account, Client, Error, account_from, unix_millis, unix_secs, uuid4,
};

const CAA_ORIGIN: &str = "https://b.i.instagram.com";
const USDID_DOC_ID: &str = "124930351917786857261002920888";
const EXPERIMENT_GROUP: &str = "caa_iteration_v3_perf_ig_4";
const MARKER_ID: u64 = 36707139;

fn b64u(raw: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(raw)
}

/// The local P-256 device identity sent as `X-Meta-Usdid`.
struct Usdid {
    key: SigningKey,
    id: String,
    kid: String,
}

impl Usdid {
    fn new() -> Self {
        let mut kid = [0u8; 32];
        rand::RngCore::fill_bytes(&mut OsRng, &mut kid);
        Self {
            key: SigningKey::random(&mut OsRng),
            id: uuid4(),
            kid: b64u(&kid),
        }
    }
    fn sign(&self, message: &str) -> String {
        let signature: Signature = self.key.sign(message.as_bytes());
        b64u(signature.to_der().as_bytes())
    }
    fn header(&self) -> String {
        let signed = format!("{}.{}", self.id, unix_secs() + 3600);
        format!("{signed}.{}", self.sign(&signed))
    }
    fn registration_token(&self) -> String {
        let now = unix_secs();
        let public = self
            .key
            .verifying_key()
            .to_public_key_der()
            .map(|der| STANDARD.encode(der.as_bytes()))
            .unwrap_or_default();
        let payload = b64u(
            dumps(
                &json!({"sub": self.id, "iat": now, "aud": APP_ID, "exp": now + 3600,
                "pub": public, "alg": "ES256"}),
            )
            .as_bytes(),
        );
        let protected = b64u(
            dumps(
                &json!({"typ": "JWT", "alg": "ES256", "kid": self.kid, "aid": APP_ID, "ver": "1"}),
            )
            .as_bytes(),
        );
        let token = json!({"payload": payload, "signatures": [{
            "protected": protected,
            "signature": self.sign(&format!("{protected}.{payload}")),
        }]});
        b64u(dumps(&token).as_bytes())
    }
}

/// First JSON string literal at or after `start` in a Bloks "lispy" expression.
fn first_json_string(text: &str, start: usize) -> Option<String> {
    let quote = text[start..].find('"')? + start;
    serde_json::Deserializer::from_str(&text[quote..])
        .into_iter::<String>()
        .next()?
        .ok()
}

fn extract_aac(result: &Value) -> Option<String> {
    result["layout"]["bloks_payload"]["data"]
        .as_array()?
        .iter()
        .filter_map(|node| {
            node.get("data")
                .filter(|d| d["key"] == "CAA_ACCOUNT_ACCESS_CONTEXT:aac")
        })
        .find_map(|payload| {
            payload["initial"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    payload["initial_lispy"]
                        .as_str()
                        .and_then(|l| first_json_string(l, 0))
                })
        })
}

/// Session headers and `logged_in_user` embedded in the Bloks action program.
fn extract_login_response(result: &Value) -> Option<(Value, Value)> {
    let action = result["layout"]["bloks_payload"]["action"].as_str()?;
    let mut index = 0;
    while index < action.len() {
        if action.as_bytes()[index] != b'"' {
            index += 1;
            continue;
        }
        let mut stream = serde_json::Deserializer::from_str(&action[index..]).into_iter::<String>();
        let Some(Ok(text)) = stream.next() else {
            index += 1;
            continue;
        };
        index += stream.byte_offset().max(1);
        if !text.contains("login_response") {
            continue;
        }
        let Ok(raw) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let response = raw["login_response"]
            .as_str()
            .and_then(|s| serde_json::from_str(s).ok());
        let headers = raw["headers"]
            .as_str()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(Value::Null);
        if let Some(response) = response {
            return Some((response, headers));
        }
    }
    None
}

fn network_info() -> Value {
    json!({
        "active_subscriptions_info": null,
        "default_subscription_info": {
            "network_type": null, "is_data_roaming": 1, "is_esim": null, "is_gsm_roaming": 0,
            "is_sim_sms_capable": null, "is_mobile_data_enabled": 1, "sim_carrier_id": 1,
            "sim_carrier_id_name": null, "sim_state": 5, "sim_operator": "310260",
            "sim_operator_name": "T-Mobile", "signal_strength": null,
            "group_id_level_1": null, "network_operator": "310260",
        },
        "is_airplane_mode": 0, "is_active_network_cellular": 0,
        "is_device_sms_capable": 1, "sim_count": 1, "is_wifi": 1,
    })
}

/// Instagram's edge answers `send_login_request` with an empty 429 unless the
/// ClientHello resembles the Android app's: TLS 1.3 only, one cipher suite,
/// X25519/P-256, over HTTP/2 (instagrapi#2852, #2862).
pub(crate) fn caa_http() -> Result<reqwest::Client, Error> {
    use rustls::crypto::ring::{cipher_suite, kx_group};
    let tls = |e: rustls::Error| Error::Protocol(format!("TLS setup failed: {e}"));
    let mut provider = rustls::crypto::ring::default_provider();
    provider.cipher_suites = vec![cipher_suite::TLS13_AES_128_GCM_SHA256];
    provider.kx_groups = vec![kx_group::X25519, kx_group::SECP256R1];
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let mut config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(tls)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec()];
    reqwest::Client::builder()
        .use_preconfigured_tls(config)
        .build()
        .map_err(crate::transport)
}

impl Client {
    fn caa_origin(&self) -> String {
        if self.base == API_ORIGIN {
            CAA_ORIGIN.into()
        } else {
            self.base.clone()
        }
    }

    /// Unsigned form POST used by Bloks, attestation and GraphQL requests.
    async fn caa_post(
        &self,
        path: &str,
        form: &[(&str, String)],
        headers: &[(&str, String)],
        usdid: &Usdid,
    ) -> Result<(u16, HeaderMap, String), Error> {
        let url = format!("{}{path}", self.caa_origin());
        let mut request = self
            .request_with(&self.caa_http, Method::POST, &url)
            .timeout(crate::READ_TIMEOUT)
            .header("X-Meta-Usdid", usdid.header())
            .form(form);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let response = request.send().await.map_err(crate::transport)?;
        self.absorb(response.headers());
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.text().await.map_err(crate::transport)?;
        Ok((status, headers, body))
    }

    async fn bloks_action(
        &self,
        action: &str,
        params: &Value,
        extra: &[(&str, String)],
        usdid: &Usdid,
    ) -> Result<Value, Error> {
        let (uuid, versioning) = {
            let s = self.state();
            (s.uuid.clone(), s.device.bloks_versioning_id.clone())
        };
        let form = [
            ("params", dumps(params)),
            ("_uuid", uuid),
            (
                "bk_client_context",
                dumps(&json!({"bloks_version": versioning, "styles_id": "instagram"})),
            ),
            ("bloks_versioning_id", versioning),
        ];
        let mut headers = vec![(
            "X-FB-Friendly-Name",
            format!("IgApi: bloks/async_action/{action}/"),
        )];
        headers.extend_from_slice(extra);
        let (status, _, body) = self
            .caa_post(
                &format!("/api/v1/bloks/async_action/{action}/"),
                &form,
                &headers,
                usdid,
            )
            .await?;
        classify(status, &body)
    }

    /// Android CAA login. Needs `Ok` only when Instagram handed back a session.
    pub(crate) async fn login_caa(&self, username: &str, password: &str) -> Result<Account, Error> {
        let usdid = Usdid::new();
        let s = self.session();

        // Password key (the same launcher/sync call the legacy login used).
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
        let encrypted = encrypt_password(password, key_id, public_key, unix_secs())?;

        // 1. Register the device identity.
        let variables = json!({"input": {
            "usdid_token": {"sensitive_string_value": usdid.registration_token()},
            "fdid": {"sensitive_string_value": s.phone_id},
        }});
        let form = [
            ("method", "post".to_string()),
            ("pretty", "false".into()),
            ("format", "json".into()),
            ("server_timestamps", "true".into()),
            ("locale", "user".into()),
            (
                "fb_api_req_friendly_name",
                "IGUSDIDRegistrationMutation".into(),
            ),
            ("enable_canonical_naming", "true".into()),
            ("enable_canonical_variable_overrides", "true".into()),
            (
                "enable_canonical_naming_ambiguous_type_prefixing",
                "true".into(),
            ),
            ("variables", variables.to_string()),
            ("purpose", "fetch".into()),
            ("client_doc_id", USDID_DOC_ID.into()),
        ];
        let (status, _, body) = self
            .caa_post(
                "/graphql_www",
                &form,
                &[
                    ("X-FB-Friendly-Name", "IGUSDIDRegistrationMutation".into()),
                    ("X-Client-Doc-Id", USDID_DOC_ID.into()),
                    ("X-Root-Field-Name", "usdid_registration".into()),
                    ("X-Graphql-Client-Library", "pando".into()),
                ],
                &usdid,
            )
            .await?;
        let registered = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| {
                v["data"].as_object().map(|data| {
                    data.iter()
                        .any(|(k, v)| k.contains("usdid_registration") && v["success"] == true)
                })
            })
            .unwrap_or(false);
        if !registered {
            classify(status, &body)?;
            return Err(Error::Protocol(
                "Instagram did not accept the device registration".into(),
            ));
        }

        // 2. Open the CAA login home and keep its account access context.
        let waterfall = uuid4();
        let home = json!({
            "is_from_logged_out": false, "logged_out_user": "", "qpl_join_id": null,
            "family_device_id": s.phone_id, "device_id": s.android_device_id,
            "offline_experiment_group": EXPERIMENT_GROUP, "waterfall_id": waterfall,
            "logout_source": "", "show_internal_settings": false, "last_auto_login_time": 0,
            "disable_auto_login": false, "qe_device_id": s.uuid,
            "use_auto_login_interstitial": true,
            "disable_recursive_auto_login_interstitial": true,
            "auto_login_interstitial_experiment_group_name": "",
            "is_from_logged_in_switcher": false, "switcher_logged_in_uid": "",
            "account_list": [], "blocked_uid": [],
            "INTERNAL_INFRA_THEME": "THREE_NEUTRAL_GRAY",
            "layered_homepage_experiment_group": "Deploy: Not in Experiment",
            "launched_url": "", "sim_phone_numbers": [], "is_from_registration_reminder": false,
        });
        let result = self
            .bloks_action(
                "com.bloks.www.bloks.caa.login.process_client_data_and_redirect",
                &home,
                &[],
                &usdid,
            )
            .await?;
        let aac = extract_aac(&result).ok_or_else(|| {
            Error::Protocol("Instagram did not return the account access context".into())
        })?;

        // 3. Attestation nonce (keystore reported unavailable, as the app does without hardware).
        let (status, _, body) = self
            .caa_post(
                "/api/v1/attestation/create_android_keystore/",
                &[
                    ("app_scoped_device_id", s.uuid.clone()),
                    ("key_hash", String::new()),
                ],
                &[(
                    "X-FB-Friendly-Name",
                    "IgApi: attestation/create_android_keystore/".into(),
                )],
                &usdid,
            )
            .await?;
        let keystore = classify(status, &body)?;
        let nonce = keystore["challenge_nonce"]
            .as_str()
            .filter(|n| !n.is_empty())
            .ok_or_else(|| Error::Protocol("Instagram did not return an attestation nonce".into()))?
            .to_owned();
        let attest = dumps(&json!({"attestation": [{
            "version": 2, "type": "keystore", "errors": [-1013],
            "challenge_nonce": nonce, "signed_nonce": "", "key_hash": "",
        }]}));

        // 4. OAuth token preflight.
        let preflight = json!({
            "client_input_params": {
                "username_input": username, "si_device_param_network_info": network_info(),
                "aac": aac, "lois_settings": {"lois_token": ""}, "cloud_trust_token": null,
                "zero_balance_state": "", "network_bssid": null,
            },
            "server_params": {
                "is_from_logged_out": 0,
                "layered_homepage_experiment_group": "Deploy: Not in Experiment",
                "device_id": s.android_device_id, "login_surface": "login_home",
                "waterfall_id": waterfall,
                "INTERNAL__latency_qpl_instance_id": unix_millis() as u64,
                "is_platform_login": 0, "login_entry_point": "logged_out",
                "INTERNAL__latency_qpl_marker_id": MARKER_ID, "family_device_id": s.phone_id,
                "offline_experiment_group": EXPERIMENT_GROUP,
                "access_flow_version": "pre_mt_behavior", "is_from_logged_in_switcher": 0,
                "qe_device_id": s.uuid,
            },
        });
        self.bloks_action(
            "com.bloks.www.caa.login.oauth.token.fetch.async",
            &preflight,
            &[],
            &usdid,
        )
        .await?;

        // 5. The login itself.
        let input_id = &uuid4().replace('-', "")[..4];
        let login = json!({
            "client_input_params": {
                "blocked_uids": [], "aac": aac, "sim_phones": [], "aymh_accounts": [],
                "network_bssid": null, "secure_family_device_id": "",
                "has_granted_read_contacts_permissions": 0, "auth_secure_device_id": "",
                "has_whatsapp_installed": 0, "si_device_param_network_info": network_info(),
                "password": encrypted, "sso_token_map_json_string": "",
                "block_store_machine_id": "", "ig_vetted_device_nonces": null,
                "cloud_trust_token": null, "event_flow": "login_manual",
                "password_contains_non_ascii": (!password.is_ascii()).to_string(),
                "client_known_key_hash": "", "sso_accounts_auth_data": [],
                "encrypted_msisdn": "", "has_granted_read_phone_permissions": 0,
                "app_manager_id": "", "should_show_nested_nta_from_aymh": 0,
                "device_id": s.android_device_id, "zero_balance_state": "",
                "login_attempt_count": 1, "machine_id": s.mid,
                "flash_call_permission_status": {"READ_PHONE_STATE": "DENIED",
                    "READ_CALL_LOG": "DENIED", "ANSWER_PHONE_CALLS": "DENIED"},
                "accounts_list": [], "gms_incoming_call_retriever_eligibility": "eligible",
                "family_device_id": s.phone_id, "fb_ig_device_id": [], "device_emails": [],
                "try_num": 1, "lois_settings": {"lois_token": ""}, "event_step": "home_page",
                "headers_infra_flow_id": "", "openid_tokens": {}, "contact_point": username,
            },
            "server_params": {
                "should_trigger_override_login_2fa_action": 0, "is_from_logged_out": 0,
                "should_trigger_override_login_success_action": 0,
                "login_credential_type": "none", "server_login_source": "login",
                "waterfall_id": waterfall, "two_step_login_type": "one_step_login",
                "login_source": "Login", "is_platform_login": 0,
                "login_entry_point": "logged_out",
                "INTERNAL__latency_qpl_marker_id": MARKER_ID, "is_from_aymh": 0,
                "offline_experiment_group": EXPERIMENT_GROUP, "is_from_landing_page": 0,
                "left_nav_button_action": "NONE",
                "password_text_input_id": format!("{input_id}ig:82"),
                "is_from_empty_password": 0, "is_from_msplit_fallback": 0,
                "ar_event_source": "login_home_page", "qe_device_id": s.uuid,
                "username_text_input_id": format!("{input_id}ig:81"),
                "layered_homepage_experiment_group": "Deploy: Not in Experiment",
                "device_id": s.android_device_id, "login_surface": "login_home",
                "INTERNAL__latency_qpl_instance_id": unix_millis() as u64,
                "reg_flow_source": "login_home_native_integration_point",
                "is_caa_perf_enabled": 1, "credential_type": "password",
                "is_from_password_entry_page": 0, "caller": "gslr",
                "family_device_id": s.phone_id, "is_from_assistive_id": 0,
                "access_flow_version": "pre_mt_behavior", "is_from_logged_in_switcher": 0,
            },
        });
        let result = self
            .bloks_action(
                "com.bloks.www.bloks.caa.login.async.send_login_request",
                &login,
                &[("X-IG-Attest-Params", attest)],
                &usdid,
            )
            .await?;

        let Some((response, headers)) = extract_login_response(&result) else {
            let text = result.to_string().to_ascii_lowercase();
            return Err(if text.contains("two_step_verification") {
                Error::Checkpoint("verification code required; approve the login in the Instagram app, then try again".into())
            } else if text.contains("find an account")
                || text.contains("password you entered")
                || (text.contains("password") && text.contains("incorrect"))
            {
                Error::BadCredentials
            } else {
                Error::Protocol("Instagram did not return a session".into())
            });
        };
        let get = |name: &str| {
            headers
                .as_object()
                .and_then(|m| m.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)))
                .and_then(|(_, v)| v.as_str())
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        {
            let mut state = self.state();
            if let Some(authorization) = get("ig-set-authorization") {
                state.authorization = authorization;
            }
            if let Some(rur) = get("ig-set-ig-u-rur") {
                state.ig_u_rur = rur;
            }
            if let Some(claim) = get("x-ig-set-www-claim") {
                state.ig_www_claim = claim;
            }
        }
        let account = account_from(&response["logged_in_user"])?;
        let mut state = self.state();
        if state.authorization.is_empty() {
            return Err(Error::Protocol(
                "login succeeded but Instagram returned no session token".into(),
            ));
        }
        state.user_id = Some(account.user_id.clone());
        state.username = Some(account.username.clone());
        Ok(account)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aac_is_found_in_the_bloks_payload() {
        let result = json!({"layout": {"bloks_payload": {"data": [
            {"data": {"key": "CAA_ACCOUNT_ACCESS_CONTEXT:aac", "initial": "abc"}}]}}});
        assert_eq!(extract_aac(&result).as_deref(), Some("abc"));
        let lispy = json!({"layout": {"bloks_payload": {"data": [
            {"data": {"key": "CAA_ACCOUNT_ACCESS_CONTEXT:aac", "initial_lispy": "(bk.x \"xyz\")"}}]}}});
        assert_eq!(extract_aac(&lispy).as_deref(), Some("xyz"));
    }

    #[test]
    fn login_response_is_read_from_the_action_program() {
        let inner = json!({
            "login_response": json!({"logged_in_user": {"pk": 1}}).to_string(),
            "headers": json!({"IG-Set-Authorization": "Bearer IGT:2:x"}).to_string(),
        })
        .to_string();
        let action = format!(
            "(bk.action.map.Make \"junk\" {})",
            serde_json::to_string(&inner).unwrap()
        );
        let result = json!({"layout": {"bloks_payload": {"action": action}}});
        let (response, headers) = extract_login_response(&result).unwrap();
        assert_eq!(response["logged_in_user"]["pk"], 1);
        assert_eq!(headers["IG-Set-Authorization"], "Bearer IGT:2:x");
    }

    #[test]
    fn usdid_header_has_three_parts() {
        assert_eq!(Usdid::new().header().split('.').count(), 3);
    }
}
