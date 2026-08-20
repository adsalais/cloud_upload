use intake_core::config::Config;
use intake_core::s3_dataplane::{build_client, S3DataPlane};

fn dp(cfg: &Config) -> S3DataPlane {
    S3DataPlane::new(build_client(cfg, &cfg.admin_access_key, &cfg.admin_secret_key, None))
}

#[tokio::test]
async fn deploy_public_site_then_download_objects() {
    let cfg = Config::from_env();
    let dp = dp(&cfg);
    let bucket = "intake-test-site-0001";
    let _ = dp.delete_bucket(bucket).await;
    dp.create_bucket(bucket).await.unwrap();
    dp.put_public_read_policy(bucket).await.unwrap();

    // déployer un dossier temporaire contenant index.html
    let tmp = std::env::temp_dir().join("intake-site-src");
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("index.html"), b"<h1>ok</h1>").unwrap();
    dp.deploy_dir(bucket, &tmp, "site/").await.unwrap();

    // lecture publique (anonyme) via HTTP path-style
    let url = format!("{}/{}/site/index.html", cfg.endpoint, bucket);
    let body = http_get(&url).await;
    assert!(body.contains("ok"), "index.html non lisible publiquement : {body}");

    // I/O objets : put + download_all
    dp.put_object_bytes(bucket, "data/a.bin", vec![1, 2, 3], "application/octet-stream")
        .await
        .unwrap();
    let dest = std::env::temp_dir().join("intake-dl");
    let _ = std::fs::remove_dir_all(&dest);
    let files = dp.download_all(bucket, &dest).await.unwrap();
    assert!(files.iter().any(|(k, _)| k == "data/a.bin"));
    let got = std::fs::read(dest.join("data/a.bin")).unwrap();
    assert_eq!(got, vec![1, 2, 3]);

    dp.delete_bucket(bucket).await.unwrap();
}

// GET HTTP sans dépendance : via `curl` (présent en dev).
async fn http_get(url: &str) -> String {
    let out = tokio::process::Command::new("curl")
        .args(["-s", url])
        .output()
        .await
        .expect("curl");
    String::from_utf8_lossy(&out.stdout).to_string()
}
