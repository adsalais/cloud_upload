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
    assert!(created.case.bucket.starts_with("intake-e2e-0001-"));

    // la victime (clé scopée) dépose une preuve sous `data/`
    let victim = build_client(
        &cfg,
        &created.case.scoped_access_key,
        &created.scoped_secret_key,
        None,
    );
    victim
        .put_object()
        .bucket(&created.case.bucket)
        .key("data/dump.raw")
        .body(vec![7u8; 1024].into())
        .send()
        .await
        .expect("upload victime");

    // l'équipe récupère (préfixe data/ uniquement)
    let dest = std::env::temp_dir().join("intake-e2e-dl");
    let _ = std::fs::remove_dir_all(&dest);
    let files = ops::pull_case(&cfg, id, &dest).await.expect("pull_case");
    assert!(files.iter().any(|(k, _)| k == "data/dump.raw"));
    assert_eq!(std::fs::read(dest.join("data/dump.raw")).unwrap().len(), 1024);

    // teardown : le bucket unique est supprimé
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
        .bucket(&created.case.bucket)
        .send()
        .await
        .is_err());
}
