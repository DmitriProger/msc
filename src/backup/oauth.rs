#![allow(dead_code)]
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const DEVICE_AUTH_URL: &str = "https://oauth2.googleapis.com/device/code";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub token_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_url: String,
    interval: Option<u64>,
    expires_in: Option<u64>,
}

pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: String,
    pub token_path: String,
}

impl OAuthClient {
    pub fn new(client_id: String, client_secret: String, token_path: String) -> Self {
        Self {
            client_id,
            client_secret,
            token_path,
        }
    }

    pub fn load_token(&self) -> Result<TokenData> {
        let path = Path::new(&self.token_path);
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read token from {}", self.token_path))?;
        serde_json::from_str(&content).context("Failed to parse token file")
    }

    pub fn save_token(&self, token: &TokenData) -> Result<()> {
        let path = Path::new(&self.token_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(token).context("Failed to serialize token")?;
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write token to {}", self.token_path))?;
        set_file_permissions(path, 0o600);
        Ok(())
    }

    pub fn is_authorized(&self) -> bool {
        self.load_token().is_ok()
    }

    pub async fn authorize(&self) -> Result<TokenData> {
        let client = reqwest::Client::new();

        let params = [("client_id", self.client_id.as_str()), ("scope", SCOPE)];
        let resp: DeviceCodeResponse = client
            .post(DEVICE_AUTH_URL)
            .form(&params)
            .send()
            .await
            .context("Failed to start device flow")?
            .json()
            .await
            .context("Failed to parse device code response")?;

        println!();
        println!("  1. Open this URL in your browser:");
        println!();
        println!("     {}", resp.verification_url);
        println!();
        println!("  2. Enter this code:");
        println!();
        println!("     {}", resp.user_code);
        println!();

        let interval = resp.interval.unwrap_or(5);
        let token = self
            .poll_for_token(&client, &resp.device_code, interval)
            .await?;
        self.save_token(&token)?;
        Ok(token)
    }

    async fn poll_for_token(
        &self,
        client: &reqwest::Client,
        device_code: &str,
        interval: u64,
    ) -> Result<TokenData> {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;

            let params = [
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("device_code", device_code),
                (
                    "grant_type",
                    "urn:ietf:params:oauth2:grant-type:device_code",
                ),
            ];
            let resp = client.post(TOKEN_URL).form(&params).send().await?;
            let body: serde_json::Value = resp.json().await?;

            if let Some(access_token) = body.get("access_token").and_then(|v| v.as_str()) {
                return Ok(TokenData {
                    access_token: access_token.to_string(),
                    refresh_token: body
                        .get("refresh_token")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    expires_in: body.get("expires_in").and_then(|v| v.as_u64()),
                    token_type: body
                        .get("token_type")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                });
            }

            if let Some(error) = body.get("error").and_then(|v| v.as_str()) {
                if error == "authorization_pending" || error == "slow_down" {
                    continue;
                }
                anyhow::bail!("OAuth error: {}", error);
            }
        }
    }

    pub async fn refresh_token(&self) -> Result<TokenData> {
        let mut token = self.load_token()?;
        let refresh_token = token
            .refresh_token
            .clone()
            .context("No refresh token available")?;

        let client = reqwest::Client::new();
        let params = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];
        let resp: serde_json::Value = client
            .post(TOKEN_URL)
            .form(&params)
            .send()
            .await?
            .json()
            .await?;

        if let Some(access_token) = resp.get("access_token").and_then(|v| v.as_str()) {
            token.access_token = access_token.to_string();
            if let Some(ei) = resp.get("expires_in").and_then(|v| v.as_u64()) {
                token.expires_in = Some(ei);
            }
            self.save_token(&token)?;
            Ok(token)
        } else {
            anyhow::bail!("Failed to refresh token: {:?}", resp.get("error"))
        }
    }

    pub async fn get_valid_token(&self) -> Result<String> {
        match self.load_token() {
            Ok(token) => Ok(token.access_token),
            Err(_) => anyhow::bail!("Not authorized. Run `msc backup auth` first"),
        }
    }
}

fn set_file_permissions(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = std::fs::metadata(path) {
        let mut perms = metadata.permissions();
        perms.set_mode(mode);
        let _ = std::fs::set_permissions(path, perms);
    }
}
