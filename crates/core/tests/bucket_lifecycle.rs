use intake_core::config::Config;
use intake_core::s3_dataplane::{build_client, S3DataPlane};

fn dp(cfg: &Config) -> S3DataPlane {
    S3DataPlane::new(build_client(cfg, &cfg.admin_access_key, &cfg.admin_secret_key, None))
}

#[tokio::test]
async fn create_versioned_bucket_then_delete_purges_versions() {
    let cfg = Config::from_env();
    let dp = dp(&cfg);
    let bucket = "intake-test-lifecycle-0001";

    // nettoyage préventif si un run précédent a échoué
    let _ = dp.delete_bucket(bucket).await;

    dp.create_bucket(bucket).await.expect("create_bucket");
    dp.enable_versioning(bucket).await.expect("enable_versioning");
    dp.set_cors(bucket, "*").await.expect("set_cors");

    // écrire deux versions du même objet
    for body in ["v1", "v2"] {
        dp.client
            .put_object()
            .bucket(bucket)
            .key("obj.txt")
            .body(body.as_bytes().to_vec().into())
            .send()
            .await
            .expect("put_object");
    }

    // versioning actif => au moins deux versions listées
    let versions = dp
        .client
        .list_object_versions()
        .bucket(bucket)
        .send()
        .await
        .expect("list_object_versions");
    assert!(versions.versions().len() >= 2, "versioning inactif ?");

    // delete_bucket doit purger versions + supprimer le bucket
    dp.delete_bucket(bucket).await.expect("delete_bucket");
    let head = dp.client.head_bucket().bucket(bucket).send().await;
    assert!(head.is_err(), "le bucket existe encore après delete_bucket");
}
