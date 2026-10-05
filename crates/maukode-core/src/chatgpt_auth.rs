//! Sign in with ChatGPT for locally hosted open-source clients.
//! Credentials are separate from API-key provider configuration.

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::Url;
use ring::signature::{RsaPublicKeyComponents, RSA_PKCS1_2048_8192_SHA256};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use uuid::Uuid;

const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPE: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
static REFRESH_LOCK: Mutex<()> = Mutex::const_new(());

#[derive(Clone)]
pub struct AuthEndpoints {
    pub authorize: String,
    pub token: String,
    pub discovery: String,
    pub resource: String,
}

impl Default for AuthEndpoints {
    fn default() -> Self {
        Self {
            authorize: "https://auth.openai.com/api/accounts/authorize".into(),
            token: "https://auth.openai.com/api/accounts/oauth/token".into(),
            discovery: "https://auth.openai.com/.well-known/openid-configuration".into(),
            resource: RESOURCE.into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Profile {
    email: String,
    subject: String,
    client_id: String,
    id_token: String,
    access_token: String,
    refresh_token: String,
    scope: String,
    expires_at: u64,
}

#[derive(Serialize, Deserialize)]
struct CredentialStore {
    host_id: String,
    #[serde(default)]
    active_client_id: Option<String>,
    #[serde(default)]
    pending_client_id: Option<String>,
    #[serde(default)]
    pending_subject: Option<String>,
    #[serde(default)]
    profiles: Vec<Profile>,
}

impl CredentialStore {
    fn active(&self) -> Result<&Profile> {
        let id = self
            .active_client_id
            .as_deref()
            .ok_or_else(|| anyhow!("Sign in with ChatGPT first: maukode auth login"))?;
        self.profiles
            .iter()
            .find(|profile| profile.client_id == id)
            .ok_or_else(|| anyhow!("ChatGPT account not found; run maukode auth login"))
    }
}

pub struct LoginAttempt {
    pub url: String,
    listener: TcpListener,
    state: String,
    nonce: String,
    verifier: String,
    redirect_uri: String,
    existing_client_id: Option<String>,
    existing_subject: Option<String>,
}

#[derive(Clone)]
pub struct ChatGptAuth {
    path: PathBuf,
    endpoints: AuthEndpoints,
    http: reqwest::Client,
}

impl ChatGptAuth {
    pub fn new() -> Result<Self> {
        let path = crate::paths::config_dir()
            .ok_or_else(|| anyhow!("Cannot locate config directory"))?
            .join("maukode/chatgpt_auth.json");
        Ok(Self::at_path(path))
    }

    pub fn at_path(path: PathBuf) -> Self {
        Self {
            path,
            endpoints: AuthEndpoints::default(),
            http: reqwest::Client::new(),
        }
    }

    pub fn with_endpoints(mut self, endpoints: AuthEndpoints) -> Self {
        self.endpoints = endpoints;
        self
    }

    fn load(&self) -> Result<CredentialStore> {
        if !self.path.exists() {
            return Ok(CredentialStore {
                host_id: format!("urn:uuid:{}", Uuid::new_v4()),
                active_client_id: None,
                pending_client_id: None,
                pending_subject: None,
                profiles: Vec::new(),
            });
        }
        let bytes = std::fs::read(&self.path).context("Cannot read ChatGPT credentials")?;
        serde_json::from_slice(&bytes).context("Invalid ChatGPT credential file")
    }

    fn save(&self, credentials: &CredentialStore) -> Result<()> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| anyhow!("Invalid auth path"))?;
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
        let temporary = parent.join(format!(".chatgpt-auth-{}", Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            serde_json::to_writer(&mut file, credentials)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    fn lock_store_blocking(&self) -> Result<std::fs::File> {
        let path = self.path.with_extension("lock");
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("Invalid auth lock path"))?;
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        file.lock()?;
        Ok(file)
    }

    async fn lock_store(&self) -> Result<std::fs::File> {
        let auth = self.clone();
        tokio::task::spawn_blocking(move || auth.lock_store_blocking()).await?
    }

    pub fn status(&self) -> Result<Option<String>> {
        let credentials = self.load()?;
        Ok(credentials
            .active()
            .ok()
            .map(|profile| profile.email.clone()))
    }

    pub fn accounts(&self) -> Result<Vec<(String, String, bool)>> {
        let credentials = self.load()?;
        Ok(credentials
            .profiles
            .iter()
            .map(|profile| {
                (
                    profile.client_id.clone(),
                    profile.email.clone(),
                    credentials.active_client_id.as_deref() == Some(&profile.client_id),
                )
            })
            .collect())
    }

    pub fn use_account(&self, client_id: &str) -> Result<()> {
        let _lock = self.lock_store_blocking()?;
        let mut credentials = self.load()?;
        if !credentials
            .profiles
            .iter()
            .any(|profile| profile.client_id == client_id)
        {
            bail!("ChatGPT account not found");
        }
        credentials.active_client_id = Some(client_id.into());
        self.save(&credentials)
    }

    pub async fn begin_login(&self, new_account: bool) -> Result<LoginAttempt> {
        let credentials = {
            let _lock = self.lock_store().await?;
            let credentials = self.load()?;
            // Keep the host ID stable even if sign-in is cancelled.
            self.save(&credentials)?;
            credentials
        };
        let existing = if new_account {
            None
        } else {
            credentials.active().ok()
        };
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let redirect_uri = format!("http://127.0.0.1:{}/auth/callback", address.port());
        let state = random_value();
        let nonce = random_value();
        let verifier = random_value();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let saved_client_id =
            existing
                .map(|profile| profile.client_id.as_str())
                .or(if new_account {
                    None
                } else {
                    credentials.pending_client_id.as_deref()
                });
        let client_id = saved_client_id.unwrap_or("dynamic_agent_client");
        let mut url = Url::parse(&self.endpoints.authorize)?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("client_id", client_id)
                .append_pair("response_type", "code")
                .append_pair("redirect_uri", &redirect_uri)
                .append_pair("scope", SCOPE)
                .append_pair("resource", &self.endpoints.resource)
                .append_pair("state", &state)
                .append_pair("nonce", &nonce)
                .append_pair("code_challenge", &challenge)
                .append_pair("code_challenge_method", "S256")
                .append_pair("ext_agent_host_id", &credentials.host_id);
            if client_id == "dynamic_agent_client" {
                query.append_pair("agent_name_hint", "MauKode");
            }
        }
        Ok(LoginAttempt {
            url: url.to_string(),
            listener,
            state,
            nonce,
            verifier,
            redirect_uri,
            existing_client_id: saved_client_id.map(str::to_owned),
            existing_subject: existing
                .map(|profile| profile.subject.clone())
                .or(if new_account {
                    None
                } else {
                    credentials.pending_subject
                }),
        })
    }

    pub async fn finish_login(&self, attempt: LoginAttempt) -> Result<String> {
        let callback = tokio::time::timeout(Duration::from_secs(180), async {
            loop {
                let (mut socket, _) = attempt.listener.accept().await?;
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0_u8; 1024];
                    let count = socket.read(&mut chunk).await?;
                    if count == 0 || bytes.len() + count > 8192 {
                        bail!("Invalid OAuth callback");
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                    if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = std::str::from_utf8(&bytes)?;
                let target = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .ok_or_else(|| anyhow!("Invalid OAuth callback"))?;
                let url = Url::parse(&format!("http://127.0.0.1{target}"))?;
                if url.path() != "/auth/callback" {
                    socket.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await?;
                    continue;
                }
                let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
                let valid = params.get("state") == Some(&attempt.state);
                let message = if valid { "MauKode sign-in received. Return to the terminal." } else { "Invalid sign-in state." };
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{message}", message.len());
                socket.write_all(response.as_bytes()).await?;
                if !valid {
                    bail!("OAuth state mismatch");
                }
                break Ok::<_, anyhow::Error>(params);
            }
        }).await.context("ChatGPT sign-in timed out")??;
        if let Some(error) = callback.get("error") {
            bail!("ChatGPT authorization failed: {error}");
        }
        let code = callback
            .get("code")
            .ok_or_else(|| anyhow!("OAuth code missing"))?;
        let client_id = if let Some(existing) = &attempt.existing_client_id {
            if callback.get("client_id").is_some_and(|id| id != existing) {
                bail!("OAuth client ID mismatch");
            }
            existing.clone()
        } else {
            callback
                .get("client_id")
                .filter(|id| id.starts_with("oaiapp_"))
                .cloned()
                .ok_or_else(|| anyhow!("OAuth registration did not return a client ID"))?
        };
        {
            let _lock = self.lock_store().await?;
            let mut credentials = self.load()?;
            credentials.pending_client_id = Some(client_id.clone());
            self.save(&credentials)?;
        }
        let response = self
            .http
            .post(&self.endpoints.token)
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", client_id.as_str()),
                ("code", code.as_str()),
                ("code_verifier", attempt.verifier.as_str()),
                ("redirect_uri", attempt.redirect_uri.as_str()),
                ("resource", self.endpoints.resource.as_str()),
            ])
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("ChatGPT token exchange failed ({})", response.status());
        }
        let token: TokenResponse = response.json().await?;
        require_plan_scope(&token.scope)?;
        let identity = self
            .verify_id_token(&token.id_token, &client_id, &attempt.nonce)
            .await?;
        if attempt
            .existing_subject
            .as_deref()
            .is_some_and(|subject| subject != identity.subject)
        {
            bail!("Different ChatGPT account returned during reauthorization");
        }
        let _lock = self.lock_store().await?;
        let mut credentials = self.load()?;
        let profile = Profile {
            email: identity.email,
            subject: identity.subject,
            client_id: client_id.clone(),
            id_token: token.id_token,
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            scope: token.scope,
            expires_at: now_secs() + token.expires_in,
        };
        let email = profile.email.clone();
        credentials
            .profiles
            .retain(|saved| saved.client_id != client_id);
        credentials.profiles.push(profile);
        credentials.active_client_id = Some(client_id);
        credentials.pending_client_id = None;
        credentials.pending_subject = None;
        self.save(&credentials)?;
        Ok(email)
    }

    async fn verify_id_token(&self, token: &str, client_id: &str, nonce: &str) -> Result<Identity> {
        let segments: Vec<_> = token.split('.').collect();
        if segments.len() != 3 {
            bail!("Invalid ID token");
        }
        let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(segments[0])?)?;
        if header["alg"] != "RS256" {
            bail!("Unsupported ID token signature");
        }
        let kid = header["kid"]
            .as_str()
            .ok_or_else(|| anyhow!("ID token key ID missing"))?;
        let discovery: Value = self
            .http
            .get(&self.endpoints.discovery)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let issuer = discovery["issuer"]
            .as_str()
            .ok_or_else(|| anyhow!("OIDC issuer missing"))?;
        let jwks_uri = discovery["jwks_uri"]
            .as_str()
            .ok_or_else(|| anyhow!("OIDC JWKS URI missing"))?;
        let issuer_url = Url::parse(issuer)?;
        let jwks_url = Url::parse(jwks_uri)?;
        if issuer_url.origin() != jwks_url.origin() {
            bail!("OIDC JWKS origin mismatch");
        }
        let jwks: Value = self
            .http
            .get(jwks_uri)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let key = jwks["keys"]
            .as_array()
            .and_then(|keys| {
                keys.iter()
                    .find(|key| key["kid"] == kid && key["kty"] == "RSA")
            })
            .ok_or_else(|| anyhow!("ID token signing key not found"))?;
        let n = URL_SAFE_NO_PAD.decode(
            key["n"]
                .as_str()
                .ok_or_else(|| anyhow!("RSA modulus missing"))?,
        )?;
        let e = URL_SAFE_NO_PAD.decode(
            key["e"]
                .as_str()
                .ok_or_else(|| anyhow!("RSA exponent missing"))?,
        )?;
        let signature = URL_SAFE_NO_PAD.decode(segments[2])?;
        let signed = format!("{}.{}", segments[0], segments[1]);
        RsaPublicKeyComponents { n: &n, e: &e }
            .verify(&RSA_PKCS1_2048_8192_SHA256, signed.as_bytes(), &signature)
            .map_err(|_| anyhow!("ID token signature invalid"))?;
        let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(segments[1])?)?;
        if claims["iss"] != issuer
            || claims["nonce"] != nonce
            || !audience_matches(&claims["aud"], client_id)
        {
            bail!("ID token claims invalid");
        }
        if claims["exp"]
            .as_u64()
            .is_none_or(|expiry| expiry <= now_secs())
        {
            bail!("ID token expired");
        }
        if claims["iat"]
            .as_u64()
            .is_none_or(|issued| issued > now_secs() + 60)
        {
            bail!("ID token issuance time invalid");
        }
        Ok(Identity {
            subject: claims["sub"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("ID token subject missing"))?
                .into(),
            email: claims["email"].as_str().unwrap_or("ChatGPT account").into(),
        })
    }

    pub async fn access_token(&self) -> Result<String> {
        let _guard = REFRESH_LOCK.lock().await;
        let _lock = self.lock_store().await?;
        let mut credentials = self.load()?;
        let profile = credentials.active()?;
        require_plan_scope(&profile.scope)?;
        if profile.expires_at > now_secs() + 120 {
            return Ok(profile.access_token.clone());
        }
        let client_id = profile.client_id.clone();
        let refresh_token = profile.refresh_token.clone();
        let response = self
            .http
            .post(&self.endpoints.token)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", client_id.as_str()),
                ("refresh_token", refresh_token.as_str()),
                ("resource", self.endpoints.resource.as_str()),
            ])
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("ChatGPT session expired; run maukode auth login");
        }
        let token: TokenResponse = response.json().await?;
        require_plan_scope(&token.scope)?;
        let profile = credentials
            .profiles
            .iter_mut()
            .find(|profile| profile.client_id == client_id)
            .ok_or_else(|| anyhow!("ChatGPT profile missing"))?;
        profile.access_token = token.access_token;
        profile.refresh_token = token.refresh_token;
        if !token.id_token.is_empty() {
            profile.id_token = token.id_token;
        }
        profile.scope = token.scope;
        profile.expires_at = now_secs() + token.expires_in;
        let access_token = profile.access_token.clone();
        self.save(&credentials)?;
        Ok(access_token)
    }

    pub async fn logout(&self) -> Result<()> {
        let _lock = self.lock_store().await?;
        let mut credentials = self.load()?;
        let profile = credentials.active()?.clone();
        let discovery: Value = self
            .http
            .get(&self.endpoints.discovery)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let url = discovery["revocation_endpoint"]
            .as_str()
            .ok_or_else(|| anyhow!("OIDC revocation endpoint missing"))?;
        let response = self
            .http
            .post(url)
            .form(&[
                ("token", profile.refresh_token.as_str()),
                ("token_type_hint", "refresh_token"),
                ("client_id", profile.client_id.as_str()),
            ])
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("Could not revoke ChatGPT session ({})", response.status());
        }
        credentials
            .profiles
            .retain(|saved| saved.client_id != profile.client_id);
        credentials.active_client_id = None;
        credentials.pending_client_id = Some(profile.client_id);
        credentials.pending_subject = Some(profile.subject);
        self.save(&credentials)
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    id_token: String,
    scope: String,
    expires_in: u64,
}

struct Identity {
    subject: String,
    email: String,
}

fn require_plan_scope(scope: &str) -> Result<()> {
    if !scope
        .split_whitespace()
        .any(|part| part == "chatgpt.tokens.use.direct")
    {
        bail!("ChatGPT plan use was not approved during sign-in");
    }
    Ok(())
}

fn audience_matches(aud: &Value, client_id: &str) -> bool {
    aud.as_str() == Some(client_id)
        || aud
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value == client_id))
}

fn random_value() -> String {
    let mut bytes = [0_u8; 32];
    // UUID v4 uses the platform cryptographic random source; concatenate two values.
    bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    URL_SAFE_NO_PAD.encode(bytes)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};
    use serde_json::json;

    #[tokio::test]
    async fn login_url_has_pkce_and_callback_rejects_wrong_state() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("auth.json");
        let auth = ChatGptAuth::at_path(path.clone());
        let attempt = auth.begin_login(false).await.unwrap();
        let url = Url::parse(&attempt.url).unwrap();
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(params["client_id"], "dynamic_agent_client");
        assert_eq!(params["code_challenge_method"], "S256");
        assert!(params["scope"].contains("chatgpt.tokens.use.direct"));
        assert!(params["ext_agent_host_id"].starts_with("urn:uuid:"));
        let callback = format!("{}?state=wrong&code=forged", params["redirect_uri"]);
        let pending = tokio::spawn(async move { auth.finish_login(attempt).await });
        let response = reqwest::get(callback).await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(pending
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("state mismatch"));
        let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(saved["profiles"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn refresh_rotates_credentials_before_reuse() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("auth.json");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let auth = ChatGptAuth::at_path(path.clone()).with_endpoints(AuthEndpoints {
            token: format!("http://{address}/token"),
            ..AuthEndpoints::default()
        });
        auth.save(&CredentialStore {
            host_id: "urn:uuid:test".into(),
            active_client_id: Some("oaiapp_test".into()),
            pending_client_id: None,
            pending_subject: None,
            profiles: vec![Profile {
                email: "test@example.com".into(),
                subject: "subject".into(),
                client_id: "oaiapp_test".into(),
                id_token: "saved".into(),
                access_token: "old".into(),
                refresh_token: "old-refresh".into(),
                scope: SCOPE.into(),
                expires_at: 0,
            }],
        })
        .unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let header_end = loop {
                let mut chunk = [0; 1024];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
                if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse().ok())
                })
                .unwrap();
            while request.len() - header_end < length {
                let mut chunk = [0; 1024];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
            }
            let body = String::from_utf8_lossy(&request[header_end..header_end + length]);
            assert!(body.contains("grant_type=refresh_token"));
            assert!(body.contains("refresh_token=old-refresh"));
            let payload = json!({
                "access_token":"new", "refresh_token":"new-refresh",
                "scope":SCOPE, "expires_in":3600,
            })
            .to_string();
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        assert_eq!(auth.access_token().await.unwrap(), "new");
        server.await.unwrap();
        assert_eq!(auth.access_token().await.unwrap(), "new");
        let saved = auth.load().unwrap();
        assert_eq!(saved.active().unwrap().refresh_token, "new-refresh");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn missing_plan_scope_and_unverified_id_token_are_rejected() {
        assert!(require_plan_scope("openid profile").is_err());
        assert!(!audience_matches(&json!("someone_else"), "oaiapp_test"));
    }

    #[test]
    fn account_selection_keeps_registrations_separate() {
        let temp = tempfile::tempdir().unwrap();
        let auth = ChatGptAuth::at_path(temp.path().join("auth.json"));
        let profiles = ["first", "second"]
            .into_iter()
            .map(|id| Profile {
                email: format!("{id}@example.com"),
                subject: id.into(),
                client_id: format!("oaiapp_{id}"),
                id_token: String::new(),
                access_token: id.into(),
                refresh_token: id.into(),
                scope: SCOPE.into(),
                expires_at: u64::MAX,
            })
            .collect();
        auth.save(&CredentialStore {
            host_id: "urn:uuid:test".into(),
            active_client_id: Some("oaiapp_first".into()),
            pending_client_id: None,
            pending_subject: None,
            profiles,
        })
        .unwrap();
        auth.use_account("oaiapp_second").unwrap();
        assert_eq!(
            auth.status().unwrap().as_deref(),
            Some("second@example.com")
        );
        assert_eq!(auth.accounts().unwrap().len(), 2);
        assert!(auth.use_account("missing").is_err());
    }

    #[tokio::test]
    async fn credential_lock_serializes_independent_instances() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("auth.json");
        let first = ChatGptAuth::at_path(path.clone());
        let second = ChatGptAuth::at_path(path);
        let guard = first.lock_store().await.unwrap();
        let waiting = tokio::spawn(async move { second.lock_store().await.unwrap() });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiting.is_finished());
        drop(guard);
        let next_guard = tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        drop(next_guard);
    }

    #[tokio::test]
    async fn logout_revokes_token_and_keeps_registration_for_reauthorization() {
        let temp = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let auth =
            ChatGptAuth::at_path(temp.path().join("auth.json")).with_endpoints(AuthEndpoints {
                discovery: format!("{issuer}/discovery"),
                ..AuthEndpoints::default()
            });
        auth.save(&CredentialStore {
            host_id: "urn:uuid:test".into(),
            active_client_id: Some("oaiapp_test".into()),
            pending_client_id: None,
            pending_subject: None,
            profiles: vec![Profile {
                email: "test@example.com".into(),
                subject: "person".into(),
                client_id: "oaiapp_test".into(),
                id_token: "id".into(),
                access_token: "access".into(),
                refresh_token: "refresh".into(),
                scope: SCOPE.into(),
                expires_at: u64::MAX,
            }],
        })
        .unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 2048];
                let count = socket.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..count]);
                let payload = if request.starts_with("GET /discovery ") {
                    json!({"revocation_endpoint": format!("{issuer}/revoke")}).to_string()
                } else {
                    assert!(request.starts_with("POST /revoke "));
                    String::new()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        auth.logout().await.unwrap();
        server.await.unwrap();
        assert!(auth.status().unwrap().is_none());
        let saved = auth.load().unwrap();
        assert_eq!(saved.pending_client_id.as_deref(), Some("oaiapp_test"));
        assert_eq!(saved.pending_subject.as_deref(), Some("person"));
        assert!(saved.profiles.is_empty());
        let attempt = auth.begin_login(false).await.unwrap();
        let url = Url::parse(&attempt.url).unwrap();
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(params["client_id"], "oaiapp_test");
        assert_eq!(attempt.existing_subject.as_deref(), Some("person"));
        let fresh = auth.begin_login(true).await.unwrap();
        assert!(fresh.url.contains("client_id=dynamic_agent_client"));
        assert!(fresh.existing_subject.is_none());
    }

    #[tokio::test]
    async fn id_token_requires_valid_signature_audience_expiry_and_nonce() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let issuer = format!("http://{address}");
        let discovery = json!({
            "issuer": issuer,
            "jwks_uri": format!("{issuer}/jwks"),
        })
        .to_string();
        let jwks = include_str!("../tests/fixtures/id_token_jwk.json").to_string();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 1024];
                let count = socket.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..count]);
                let payload = if request.starts_with("GET /jwks ") {
                    &jwks
                } else {
                    &discovery
                };
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let auth = ChatGptAuth::at_path(tempfile::tempdir().unwrap().path().join("auth.json"))
            .with_endpoints(AuthEndpoints {
                discovery: format!("{issuer}/discovery"),
                ..AuthEndpoints::default()
            });
        let key =
            RsaKeyPair::from_pkcs8(include_bytes!("../tests/fixtures/id_token_key.der")).unwrap();
        let header = URL_SAFE_NO_PAD.encode(json!({"alg":"RS256","kid":"test-key"}).to_string());
        let make_token = |audience: &str, nonce: &str, exp: u64| {
            let claims = json!({
                "iss": issuer, "aud": audience, "nonce": nonce,
                "exp": exp, "iat": now_secs(), "sub": "person", "email": "test@example.com",
            });
            let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
            let signed = format!("{header}.{payload}");
            let mut signature = vec![0; key.public().modulus_len()];
            key.sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                signed.as_bytes(),
                &mut signature,
            )
            .unwrap();
            format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature))
        };
        let valid = make_token("oaiapp_test", "nonce", now_secs() + 60);
        assert_eq!(
            auth.verify_id_token(&valid, "oaiapp_test", "nonce")
                .await
                .unwrap()
                .subject,
            "person"
        );
        assert!(auth
            .verify_id_token(&valid, "oaiapp_test", "wrong")
            .await
            .is_err());
        assert!(auth
            .verify_id_token(
                &make_token("other", "nonce", now_secs() + 60),
                "oaiapp_test",
                "nonce"
            )
            .await
            .is_err());
        assert!(auth
            .verify_id_token(
                &make_token("oaiapp_test", "nonce", now_secs() - 1),
                "oaiapp_test",
                "nonce"
            )
            .await
            .is_err());
        let mut invalid = valid.into_bytes();
        *invalid.last_mut().unwrap() = if *invalid.last().unwrap() == b'A' {
            b'B'
        } else {
            b'A'
        };
        assert!(auth
            .verify_id_token(&String::from_utf8(invalid).unwrap(), "oaiapp_test", "nonce")
            .await
            .is_err());
        server.abort();
    }

    #[tokio::test]
    async fn login_exchanges_issued_client_and_persists_verified_profile() {
        let temp = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let issuer = format!("http://{address}");
        let auth =
            ChatGptAuth::at_path(temp.path().join("auth.json")).with_endpoints(AuthEndpoints {
                token: format!("{issuer}/token"),
                discovery: format!("{issuer}/discovery"),
                ..AuthEndpoints::default()
            });
        let attempt = auth.begin_login(false).await.unwrap();
        let url = Url::parse(&attempt.url).unwrap();
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        let key =
            RsaKeyPair::from_pkcs8(include_bytes!("../tests/fixtures/id_token_key.der")).unwrap();
        let header = URL_SAFE_NO_PAD.encode(json!({"alg":"RS256","kid":"test-key"}).to_string());
        let claims = json!({
            "iss": issuer, "aud": "oaiapp_test", "nonce": params["nonce"],
            "exp": now_secs() + 3600, "iat": now_secs(),
            "sub": "person", "email": "test@example.com",
        });
        let signed = format!("{header}.{}", URL_SAFE_NO_PAD.encode(claims.to_string()));
        let mut signature = vec![0; key.public().modulus_len()];
        key.sign(
            &RSA_PKCS1_SHA256,
            &SystemRandom::new(),
            signed.as_bytes(),
            &mut signature,
        )
        .unwrap();
        let id_token = format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature));
        let token = json!({
            "access_token": "access", "refresh_token": "refresh", "id_token": id_token,
            "scope": SCOPE, "expires_in": 3600,
        })
        .to_string();
        let discovery = json!({"issuer": issuer, "jwks_uri": format!("{issuer}/jwks")}).to_string();
        let jwks = include_str!("../tests/fixtures/id_token_jwk.json").to_string();
        let server = tokio::spawn(async move {
            for _ in 0..3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let count = socket.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                    if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let (payload, content_type) = if request.starts_with("POST /token ") {
                    (&token, "application/json")
                } else if request.starts_with("GET /discovery ") {
                    (&discovery, "application/json")
                } else {
                    assert!(request.starts_with("GET /jwks "));
                    (&jwks, "application/json")
                };
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let callback = format!(
            "{}?state={}&code=code&client_id=oaiapp_test",
            params["redirect_uri"], params["state"]
        );
        let callback_request = tokio::spawn(async move { reqwest::get(callback).await.unwrap() });
        assert_eq!(
            auth.finish_login(attempt).await.unwrap(),
            "test@example.com"
        );
        assert_eq!(
            callback_request.await.unwrap().status(),
            reqwest::StatusCode::OK
        );
        server.await.unwrap();
        assert_eq!(auth.access_token().await.unwrap(), "access");
        let saved = auth.load().unwrap();
        assert_eq!(saved.active().unwrap().client_id, "oaiapp_test");
        assert!(saved.pending_client_id.is_none());
    }
}
