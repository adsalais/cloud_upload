use crate::config::Config;

/// Plan de contrôle IAM spécifique MinIO, via le client `mc`.
pub struct MinioIam {
    pub alias: String,
    pub parent: String,
}

pub struct ScopedCreds {
    pub access_key: String,
    pub secret_key: String,
}

impl MinioIam {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            alias: cfg.mc_alias.clone(),
            parent: cfg.mc_parent.clone(),
        }
    }

    /// Crée une clé de service écriture-seule scopée à `bucket/prefix*`
    /// (ex. `prefix = "data/"` → ne peut écrire que sous `data/`, pas sur le site).
    pub async fn create_scoped_upload_key(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> anyhow::Result<ScopedCreds> {
        let policy = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Action": [
                    "s3:PutObject",
                    "s3:AbortMultipartUpload",
                    "s3:ListMultipartUploadParts"
                ],
                "Resource": [format!("arn:aws:s3:::{bucket}/{prefix}*")]
            }]
        });
        let tmp = std::env::temp_dir().join(format!("intake-pol-{bucket}.json"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(&policy)?)?;

        let out = tokio::process::Command::new("mc")
            .arg("--json")
            .args(["admin", "user", "svcacct", "add", "--policy"])
            .arg(&tmp)
            .arg(&self.alias)
            .arg(&self.parent)
            .output()
            .await?;
        let _ = std::fs::remove_file(&tmp);

        if !out.status.success() {
            anyhow::bail!(
                "mc svcacct add a échoué : {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        #[derive(serde::Deserialize)]
        struct Sa {
            #[serde(rename = "accessKey")]
            access_key: String,
            #[serde(rename = "secretKey")]
            secret_key: String,
        }
        let sa: Sa = serde_json::from_slice(&out.stdout)?;
        Ok(ScopedCreds {
            access_key: sa.access_key,
            secret_key: sa.secret_key,
        })
    }

    pub async fn delete_scoped_key(&self, access_key: &str) -> anyhow::Result<()> {
        let out = tokio::process::Command::new("mc")
            .args(["admin", "user", "svcacct", "rm", &self.alias, access_key])
            .output()
            .await?;
        if !out.status.success() {
            anyhow::bail!(
                "mc svcacct rm a échoué : {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(())
    }
}
