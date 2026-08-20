use intake_core::config::Config;
use intake_core::minio_iam::MinioIam;
use intake_core::s3_dataplane::{build_client, S3DataPlane};

#[tokio::test]
async fn scoped_key_can_only_write_under_data_prefix() {
    let cfg = Config::from_env();
    let admin = S3DataPlane::new(build_client(
        &cfg,
        &cfg.admin_access_key,
        &cfg.admin_secret_key,
        None,
    ));
    let bucket = "intake-test-scoped-0001";
    let _ = admin.delete_bucket(bucket).await;
    admin.create_bucket(bucket).await.unwrap();

    let iam = MinioIam::from_config(&cfg);
    let creds = iam.create_scoped_upload_key(bucket, "data/").await.unwrap();
    let scoped = build_client(&cfg, &creds.access_key, &creds.secret_key, None);

    // POSITIVE: write under data/
    let put_ok = scoped
        .put_object()
        .bucket(bucket)
        .key("data/evidence/a.bin")
        .body(vec![9u8, 9, 9].into())
        .send()
        .await;
    assert!(put_ok.is_ok(), "scoped key should be able to write under data/: {put_ok:?}");

    // NEGATIVE 1: tamper with the site (write under site/) -> denied
    let tamper = scoped
        .put_object()
        .bucket(bucket)
        .key("site/upload.js")
        .body(vec![1u8].into())
        .send()
        .await;
    assert!(tamper.is_err(), "scoped key must NOT be able to write under site/");

    // NEGATIVE 2: write outside data/ (root) -> denied
    let root = scoped
        .put_object()
        .bucket(bucket)
        .key("root.bin")
        .body(vec![1u8].into())
        .send()
        .await;
    assert!(root.is_err(), "scoped key must ONLY write under data/");

    // NEGATIVE 3: read -> denied
    let get = scoped
        .get_object()
        .bucket(bucket)
        .key("data/evidence/a.bin")
        .send()
        .await;
    assert!(get.is_err(), "scoped key must NOT be able to read");

    // NEGATIVE 4: list -> denied
    let list = scoped.list_objects_v2().bucket(bucket).send().await;
    assert!(list.is_err(), "scoped key must NOT be able to list");

    // revoke
    iam.delete_scoped_key(&creds.access_key).await.unwrap();

    // NEGATIVE 5: after revocation, writing fails
    let put_after = scoped
        .put_object()
        .bucket(bucket)
        .key("data/evidence/b.bin")
        .body(vec![0u8].into())
        .send()
        .await;
    assert!(put_after.is_err(), "revoked key must no longer work");

    admin.delete_bucket(bucket).await.unwrap();
}
