use crate::config::Config;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{
    BucketVersioningStatus, CorsConfiguration, CorsRule, VersioningConfiguration,
};
use aws_sdk_s3::Client;
use std::path::{Path, PathBuf};

/// Construit un client S3 (compatible MinIO/Scaleway/OVH) en path-style.
pub fn build_client(cfg: &Config, access: &str, secret: &str, session: Option<&str>) -> Client {
    let creds = Credentials::new(access, secret, session.map(|s| s.to_string()), None, "static");
    let s3conf = aws_sdk_s3::config::Builder::new()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new(cfg.region.clone()))
        .endpoint_url(cfg.endpoint.clone())
        .credentials_provider(creds)
        .force_path_style(true)
        .build();
    Client::from_conf(s3conf)
}

/// Plan de données S3, portable entre MinIO et les providers compatibles S3.
pub struct S3DataPlane {
    pub client: Client,
}

impl S3DataPlane {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn create_bucket(&self, name: &str) -> anyhow::Result<()> {
        // région us-east-1 : pas de CreateBucketConfiguration
        self.client.create_bucket().bucket(name).send().await?;
        Ok(())
    }

    pub async fn enable_versioning(&self, bucket: &str) -> anyhow::Result<()> {
        let vc = VersioningConfiguration::builder()
            .status(BucketVersioningStatus::Enabled)
            .build();
        self.client
            .put_bucket_versioning()
            .bucket(bucket)
            .versioning_configuration(vc)
            .send()
            .await?;
        Ok(())
    }

    /// Configure le CORS du bucket. Non bloquant si le backend ne l'implémente pas
    /* (MinIO renvoie `NotImplemented`) : en proto on est en path-style/même origine,
       le CORS n'est requis qu'en prod virtual-host (Scaleway/OVH le supportent). */
    pub async fn set_cors(&self, bucket: &str, origin: &str) -> anyhow::Result<()> {
        let rule = CorsRule::builder()
            .allowed_methods("PUT")
            .allowed_methods("POST")
            .allowed_methods("GET")
            .allowed_methods("HEAD")
            .allowed_origins(origin)
            .allowed_headers("*")
            .expose_headers("ETag")
            .max_age_seconds(3000)
            .build()?;
        let cc = CorsConfiguration::builder().cors_rules(rule).build()?;
        match self
            .client
            .put_bucket_cors()
            .bucket(bucket)
            .cors_configuration(cc)
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.code() == Some("NotImplemented") {
                    Ok(())
                } else {
                    Err(e.into())
                }
            }
        }
    }

    pub async fn delete_bucket(&self, bucket: &str) -> anyhow::Result<()> {
        let mut key_marker: Option<String> = None;
        let mut ver_marker: Option<String> = None;
        loop {
            let mut req = self.client.list_object_versions().bucket(bucket);
            if let Some(k) = &key_marker {
                req = req.key_marker(k);
            }
            if let Some(v) = &ver_marker {
                req = req.version_id_marker(v);
            }
            let resp = match req.send().await {
                Ok(r) => r,
                Err(_) => break, // bucket inexistant : rien à purger
            };
            for v in resp.versions() {
                if let (Some(k), Some(id)) = (v.key(), v.version_id()) {
                    self.client
                        .delete_object()
                        .bucket(bucket)
                        .key(k)
                        .version_id(id)
                        .send()
                        .await?;
                }
            }
            for d in resp.delete_markers() {
                if let (Some(k), Some(id)) = (d.key(), d.version_id()) {
                    self.client
                        .delete_object()
                        .bucket(bucket)
                        .key(k)
                        .version_id(id)
                        .send()
                        .await?;
                }
            }
            if resp.is_truncated().unwrap_or(false) {
                key_marker = resp.next_key_marker().map(str::to_string);
                ver_marker = resp.next_version_id_marker().map(str::to_string);
            } else {
                break;
            }
        }
        self.client.delete_bucket().bucket(bucket).send().await?;
        Ok(())
    }
}

impl S3DataPlane {
    /// Rend lisible publiquement **uniquement** les objets sous `prefix`
    /// (ex. `site/`). Les autres préfixes (ex. `data/`) restent privés.
    pub async fn put_public_read_prefix(&self, bucket: &str, prefix: &str) -> anyhow::Result<()> {
        let policy = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": {"AWS": ["*"]},
                "Action": ["s3:GetObject"],
                "Resource": [format!("arn:aws:s3:::{bucket}/{prefix}*")]
            }]
        })
        .to_string();
        self.client
            .put_bucket_policy()
            .bucket(bucket)
            .policy(policy)
            .send()
            .await?;
        Ok(())
    }

    pub async fn put_object_bytes(
        &self,
        bucket: &str,
        key: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> anyhow::Result<()> {
        self.client
            .put_object()
            .bucket(bucket)
            .key(key)
            .content_type(content_type)
            .body(ByteStream::from(bytes))
            .send()
            .await?;
        Ok(())
    }

    /// Téléverse récursivement un dossier local sous `key_prefix`.
    pub async fn deploy_dir(
        &self,
        bucket: &str,
        dir: &Path,
        key_prefix: &str,
    ) -> anyhow::Result<()> {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d)? {
                let path = entry?.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let rel = path.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
                let key = format!("{key_prefix}{rel}");
                let ct = content_type_for(&path);
                let body = ByteStream::from_path(&path).await?;
                self.client
                    .put_object()
                    .bucket(bucket)
                    .key(&key)
                    .content_type(ct)
                    .body(body)
                    .send()
                    .await?;
            }
        }
        Ok(())
    }

    /// Télécharge tous les objets d'un bucket dans `dest`.
    pub async fn download_all(
        &self,
        bucket: &str,
        dest: &Path,
    ) -> anyhow::Result<Vec<(String, PathBuf)>> {
        self.download_prefix(bucket, "", dest).await
    }

    /// Télécharge les objets d'un bucket sous `prefix` dans `dest`.
    pub async fn download_prefix(
        &self,
        bucket: &str,
        prefix: &str,
        dest: &Path,
    ) -> anyhow::Result<Vec<(String, PathBuf)>> {
        let mut out = Vec::new();
        let mut cont: Option<String> = None;
        loop {
            let mut req = self.client.list_objects_v2().bucket(bucket).prefix(prefix);
            if let Some(t) = &cont {
                req = req.continuation_token(t);
            }
            let resp = req.send().await?;
            for obj in resp.contents() {
                let key = match obj.key() {
                    Some(k) => k.to_string(),
                    None => continue,
                };
                let go = self.client.get_object().bucket(bucket).key(&key).send().await?;
                let data = go.body.collect().await?.into_bytes();
                let path = dest.join(&key);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, &data)?;
                out.push((key, path));
            }
            if resp.is_truncated().unwrap_or(false) {
                cont = resp.next_continuation_token().map(str::to_string);
            } else {
                break;
            }
        }
        Ok(out)
    }
}

fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}
