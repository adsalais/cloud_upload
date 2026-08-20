use crate::config::Config;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::Client;

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
