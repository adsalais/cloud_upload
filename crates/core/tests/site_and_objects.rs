use intake_core::config::Config;
use intake_core::s3_dataplane::{build_client, S3DataPlane};

fn dp(cfg: &Config) -> S3DataPlane {
    S3DataPlane::new(build_client(cfg, &cfg.admin_access_key, &cfg.admin_secret_key, None))
}

#[tokio::test]
async fn site_prefix_public_data_prefix_private() {
    let cfg = Config::from_env();
    let dp = dp(&cfg);
    let bucket = "intake-test-site-0001";
    let _ = dp.delete_bucket(bucket).await;
    dp.create_bucket(bucket).await.unwrap();
    dp.put_public_read_prefix(bucket, "site/").await.unwrap();

    // déployer un dossier temporaire contenant index.html sous site/
    let tmp = std::env::temp_dir().join("intake-site-src");
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("index.html"), b"<h1>ok</h1>").unwrap();
    dp.deploy_dir(bucket, &tmp, "site/").await.unwrap();

    // un objet "données" privé
    dp.put_object_bytes(bucket, "data/a.bin", vec![1, 2, 3], "application/octet-stream")
        .await
        .unwrap();

    // site/ lisible publiquement (200)
    let site_url = format!("{}/{}/site/index.html", cfg.endpoint, bucket);
    let (code, body) = http_get(&site_url).await;
    assert_eq!(code, "200", "site/index.html devrait être public");
    assert!(body.contains("ok"));

    // data/ NON lisible publiquement (403 attendu)
    let data_url = format!("{}/{}/data/a.bin", cfg.endpoint, bucket);
    let (code, _) = http_get(&data_url).await;
    assert_eq!(code, "403", "data/ ne doit PAS être public (obtenu {code})");

    // l'équipe (admin) récupère bien le préfixe data/
    let dest = std::env::temp_dir().join("intake-dl");
    let _ = std::fs::remove_dir_all(&dest);
    let files = dp.download_prefix(bucket, "data/", &dest).await.unwrap();
    assert!(files.iter().any(|(k, _)| k == "data/a.bin"));
    assert_eq!(std::fs::read(dest.join("data/a.bin")).unwrap(), vec![1, 2, 3]);

    dp.delete_bucket(bucket).await.unwrap();
}

// GET HTTP sans dépendance : via `curl`. Retourne (code HTTP, corps).
async fn http_get(url: &str) -> (String, String) {
    let out = tokio::process::Command::new("curl")
        .args(["-s", "-w", "\n%{http_code}", url])
        .output()
        .await
        .expect("curl");
    let s = String::from_utf8_lossy(&out.stdout).to_string();
    let (body, code) = s.rsplit_once('\n').unwrap_or((s.as_str(), ""));
    (code.to_string(), body.to_string())
}
