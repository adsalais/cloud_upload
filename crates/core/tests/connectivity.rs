use intake_core::config::Config;
use intake_core::s3_dataplane::build_client;

#[tokio::test]
async fn connects_and_lists_buckets() {
    let cfg = Config::from_env();
    let client = build_client(&cfg, &cfg.admin_access_key, &cfg.admin_secret_key, None);
    let res = client.list_buckets().send().await;
    assert!(res.is_ok(), "list_buckets failed: {res:?}");
}
