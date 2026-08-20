use crate::case::{Case, CaseState};
use crate::config::Config;
use crate::minio_iam::MinioIam;
use crate::s3_dataplane::{build_client, S3DataPlane};
use std::path::{Path, PathBuf};

/// Préfixes au sein du bucket unique de l'affaire.
pub const SITE_PREFIX: &str = "site/";
pub const DATA_PREFIX: &str = "data/";

pub struct CreateResult {
    pub case: Case,
    pub scoped_secret_key: String,
}

fn rand_suffix() -> String {
    let mut b = [0u8; 4];
    getrandom::getrandom(&mut b).expect("rng");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn slug(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect()
}

fn admin_dp(cfg: &Config) -> S3DataPlane {
    S3DataPlane::new(build_client(cfg, &cfg.admin_access_key, &cfg.admin_secret_key, None))
}

pub async fn create_case(cfg: &Config, id: &str) -> anyhow::Result<CreateResult> {
    let s = slug(id);
    let suffix = rand_suffix();
    let bucket = format!("intake-{s}-{suffix}");
    let dp = admin_dp(cfg);

    // Un seul bucket : versioning + CORS (prod virtual-host) + lecture publique du seul
    // préfixe `site/`. Le préfixe `data/` reste privé (données d'incident).
    dp.create_bucket(&bucket).await?;
    dp.enable_versioning(&bucket).await?;
    dp.set_cors(&bucket, "*").await?;
    dp.put_public_read_prefix(&bucket, SITE_PREFIX).await?;

    // config.json non-secret injecté pour le site (l'upload va sous `data/`).
    let config_json = serde_json::json!({
        "endpoint": cfg.endpoint,
        "region": cfg.region,
        "dataBucket": bucket,
        "usePathStyle": true
    })
    .to_string();
    dp.put_object_bytes(
        &bucket,
        &format!("{SITE_PREFIX}config.json"),
        config_json.into_bytes(),
        "application/json",
    )
    .await?;

    // déployer les fichiers statiques du site sous `site/`
    let site_dir = Path::new(&cfg.site_dir);
    if site_dir.exists() {
        dp.deploy_dir(&bucket, site_dir, SITE_PREFIX).await?;
    }

    // clé scopée écriture-seule, verrouillée au préfixe `data/` (ne peut pas altérer
    // le site ni lire les données).
    let iam = MinioIam::from_config(cfg);
    let creds = iam.create_scoped_upload_key(&bucket, DATA_PREFIX).await?;

    let site_url = format!("{}/{}/{SITE_PREFIX}index.html", cfg.endpoint, bucket);
    let case = Case {
        id: id.to_string(),
        bucket,
        scoped_access_key: creds.access_key,
        site_url,
        state: CaseState::Active,
    };
    case.save(&cfg.cases_dir)?;

    Ok(CreateResult {
        case,
        scoped_secret_key: creds.secret_key,
    })
}

pub async fn pull_case(
    cfg: &Config,
    id: &str,
    dest: &Path,
) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let case = Case::load(&cfg.cases_dir, id)?;
    let dp = admin_dp(cfg);
    // ne récupère que les données déposées (`data/`), pas les fichiers du site.
    dp.download_prefix(&case.bucket, DATA_PREFIX, dest).await
}

pub async fn teardown_case(cfg: &Config, id: &str) -> anyhow::Result<()> {
    let case = match Case::load(&cfg.cases_dir, id) {
        Ok(c) => c,
        Err(_) => return Ok(()), // rien à faire
    };
    let iam = MinioIam::from_config(cfg);
    // révoquer la clé (ignore l'erreur si déjà supprimée)
    let _ = iam.delete_scoped_key(&case.scoped_access_key).await;
    let dp = admin_dp(cfg);
    let _ = dp.delete_bucket(&case.bucket).await;

    let torn = Case {
        state: CaseState::TornDown,
        ..case
    };
    torn.save(&cfg.cases_dir)?;
    Ok(())
}
