use crate::case::{Case, CaseState};
use crate::config::Config;
use crate::minio_iam::MinioIam;
use crate::s3_dataplane::{build_client, S3DataPlane};
use std::path::{Path, PathBuf};

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
    let data_bucket = format!("intake-data-{s}-{suffix}");
    let site_bucket = format!("intake-site-{s}-{suffix}");
    let dp = admin_dp(cfg);

    // bucket data : privé + versioning + CORS (belt-and-suspenders pour la prod)
    dp.create_bucket(&data_bucket).await?;
    dp.enable_versioning(&data_bucket).await?;
    dp.set_cors(&data_bucket, "*").await?;

    // bucket site : public read + déploiement
    dp.create_bucket(&site_bucket).await?;
    dp.put_public_read_policy(&site_bucket).await?;

    // config.json non-secret injecté pour le site
    let config_json = serde_json::json!({
        "endpoint": cfg.endpoint,
        "region": cfg.region,
        "dataBucket": data_bucket,
        "usePathStyle": true
    })
    .to_string();
    dp.put_object_bytes(
        &site_bucket,
        "site/config.json",
        config_json.into_bytes(),
        "application/json",
    )
    .await?;

    // déployer les fichiers statiques du site (index.html, sigv4.js, upload.js…)
    let site_dir = Path::new(&cfg.site_dir);
    if site_dir.exists() {
        dp.deploy_dir(&site_bucket, site_dir, "site/").await?;
    }

    // clé scopée écriture-seule pour la victime
    let iam = MinioIam::from_config(cfg);
    let creds = iam.create_scoped_upload_key(&data_bucket).await?;

    let site_url = format!("{}/{}/site/index.html", cfg.endpoint, site_bucket);
    let case = Case {
        id: id.to_string(),
        data_bucket,
        site_bucket,
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
    dp.download_all(&case.data_bucket, dest).await
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
    let _ = dp.delete_bucket(&case.data_bucket).await;
    let _ = dp.delete_bucket(&case.site_bucket).await;

    let torn = Case {
        state: CaseState::TornDown,
        ..case
    };
    torn.save(&cfg.cases_dir)?;
    Ok(())
}
