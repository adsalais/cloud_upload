use intake_core::config::Config;
use intake_core::ops;
use intake_core::s3_dataplane::build_client;

#[tokio::test]
async fn full_case_lifecycle() {
    let cfg = Config::from_env();
    let id = "e2e-0001";
    // au cas où un run précédent a laissé des traces
    let _ = ops::teardown_case(&cfg, id).await;

    let created = ops::create_case(&cfg, id).await.expect("create_case");
    assert!(created.case.data_bucket.starts_with("intake-data-e2e-0001-"));
    assert!(created.case.site_bucket.starts_with("intake-site-e2e-0001-"));

    // la victime (clé scopée) dépose une preuve
    let victim = build_client(
        &cfg,
        &created.case.scoped_access_key,
        &created.scoped_secret_key,
        None,
    );
    victim
        .put_object()
        .bucket(&created.case.data_bucket)
        .key("dump.raw")
        .body(vec![7u8; 1024].into())
        .send()
        .await
        .expect("upload victime");

    // l'équipe récupère
    let dest = std::env::temp_dir().join("intake-e2e-dl");
    let _ = std::fs::remove_dir_all(&dest);
    let files = ops::pull_case(&cfg, id, &dest).await.expect("pull_case");
    assert!(files.iter().any(|(k, _)| k == "dump.raw"));
    assert_eq!(std::fs::read(dest.join("dump.raw")).unwrap().len(), 1024);

    // teardown : buckets supprimés
    ops::teardown_case(&cfg, id).await.expect("teardown_case");
    let admin = intake_core::s3_dataplane::S3DataPlane::new(build_client(
        &cfg,
        &cfg.admin_access_key,
        &cfg.admin_secret_key,
        None,
    ));
    assert!(admin
        .client
        .head_bucket()
        .bucket(&created.case.data_bucket)
        .send()
        .await
        .is_err());
}
