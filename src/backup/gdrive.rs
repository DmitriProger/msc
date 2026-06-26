#![allow(dead_code)]
use anyhow::{Context, Result};
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

const DRIVE_FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
const DRIVE_UPLOAD_URL: &str =
    "https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable";

#[derive(Debug, Deserialize, Serialize)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    pub size: Option<String>,
    #[serde(rename = "createdTime")]
    pub created_time: Option<String>,
    pub parents: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct FileList {
    files: Vec<DriveFile>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

pub struct DriveClient {
    client: reqwest::Client,
    access_token: String,
}

impl DriveClient {
    pub fn new(access_token: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            access_token,
        }
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.access_token)
    }

    pub async fn find_or_create_folder(
        &self,
        name: &str,
        parent_id: Option<&str>,
    ) -> Result<String> {
        let q = if let Some(pid) = parent_id {
            format!("mimeType='application/vnd.google-apps.folder' and name='{}' and '{}' in parents and trashed=false", name, pid)
        } else {
            format!(
                "mimeType='application/vnd.google-apps.folder' and name='{}' and trashed=false",
                name
            )
        };

        let resp: FileList = self
            .client
            .get(DRIVE_FILES_URL)
            .bearer_auth(&self.access_token)
            .query(&[("q", &q), ("fields", &"files(id,name)".to_string())])
            .send()
            .await?
            .json()
            .await?;

        if let Some(file) = resp.files.into_iter().next() {
            return Ok(file.id);
        }

        let mut metadata = serde_json::json!({
            "name": name,
            "mimeType": "application/vnd.google-apps.folder"
        });
        if let Some(pid) = parent_id {
            metadata["parents"] = serde_json::json!([pid]);
        }

        let created: DriveFile = self
            .client
            .post(DRIVE_FILES_URL)
            .bearer_auth(&self.access_token)
            .json(&metadata)
            .send()
            .await?
            .json()
            .await?;

        Ok(created.id)
    }

    pub async fn upload_file_resumable(
        &self,
        file_path: &Path,
        file_name: &str,
        parent_id: &str,
        progress_cb: impl Fn(u64, u64),
    ) -> Result<DriveFile> {
        let metadata = serde_json::json!({
            "name": file_name,
            "parents": [parent_id]
        });
        let metadata_str = serde_json::to_string(&metadata)?;

        let file_size = std::fs::metadata(file_path)
            .with_context(|| format!("Cannot stat file: {}", file_path.display()))?
            .len();

        // Initiate resumable upload session
        let init_resp = self
            .client
            .post(DRIVE_UPLOAD_URL)
            .bearer_auth(&self.access_token)
            .header(CONTENT_TYPE, "application/json; charset=UTF-8")
            .header("X-Upload-Content-Type", "application/zip")
            .header("X-Upload-Content-Length", file_size.to_string())
            .body(metadata_str)
            .send()
            .await
            .context("Failed to initiate resumable upload")?;

        let upload_url = init_resp
            .headers()
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .context("No Location header in upload initiation response")?
            .to_string();

        // Upload the file
        let mut file = File::open(file_path)
            .await
            .with_context(|| format!("Failed to open file: {}", file_path.display()))?;
        let mut data = Vec::with_capacity(file_size as usize);
        file.read_to_end(&mut data).await?;

        progress_cb(0, file_size);

        let result: DriveFile = self
            .client
            .put(&upload_url)
            .header(CONTENT_TYPE, "application/zip")
            .header(CONTENT_LENGTH, file_size.to_string())
            .body(data)
            .send()
            .await
            .context("Failed to upload file")?
            .json()
            .await
            .context("Failed to parse upload response")?;

        progress_cb(file_size, file_size);
        Ok(result)
    }

    pub async fn list_files_in_folder(&self, folder_id: &str) -> Result<Vec<DriveFile>> {
        let q = format!("'{}' in parents and trashed=false", folder_id);
        let mut all_files = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut query = vec![
                ("q", q.clone()),
                (
                    "fields",
                    "nextPageToken,files(id,name,size,createdTime)".to_string(),
                ),
                ("orderBy", "createdTime".to_string()),
            ];
            if let Some(ref token) = page_token {
                query.push(("pageToken", token.clone()));
            }

            let resp: FileList = self
                .client
                .get(DRIVE_FILES_URL)
                .bearer_auth(&self.access_token)
                .query(&query)
                .send()
                .await?
                .json()
                .await?;

            all_files.extend(resp.files);
            page_token = resp.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        Ok(all_files)
    }

    pub async fn delete_file(&self, file_id: &str) -> Result<()> {
        let url = format!("{}/{}", DRIVE_FILES_URL, file_id);
        self.client
            .delete(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .context("Failed to delete file")?;
        Ok(())
    }

    pub async fn download_file(
        &self,
        file_id: &str,
        output_path: &Path,
        progress_cb: impl Fn(u64, u64),
    ) -> Result<()> {
        let url = format!("{}/{}?alt=media", DRIVE_FILES_URL, file_id);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .context("Failed to initiate download")?;

        let total = resp.content_length().unwrap_or(0);
        let bytes = resp
            .bytes()
            .await
            .context("Failed to read download response")?;

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(output_path, &bytes)
            .with_context(|| format!("Failed to write to {}", output_path.display()))?;

        progress_cb(total, total);
        Ok(())
    }
}
