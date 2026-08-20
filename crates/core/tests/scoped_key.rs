use intake_core::config::Config;
use intake_core::minio_iam::MinioIam;
use intake_core::s3_dataplane::{build_client, S3DataPlane};

#[tokio::test]
async fn scoped_key_can_only_write_to_its_bucket() {
    let cfg = Config::from_env();
    let admin = S3DataPlane::new(build_client(
        &cfg,
        &cfg.admin_access_key,
        &cfg.admin_secret_key,
        None,
    ));
    let data_bucket = "intake-test-scoped-data-0001";
    let other_bucket = "intake-test-scoped-other-0001";
    let _ = admin.delete_bucket(data_bucket).await;
    let _ = admin.delete_bucket(other_bucket).await;
    admin.create_bucket(data_bucket).await.unwrap();
    admin.create_bucket(other_bucket).await.unwrap();

    let iam = MinioIam::from_config(&cfg);
    let creds = iam.create_scoped_upload_key(data_bucket).await.unwrap();

    // client construit avec la clé scopée
    let scoped = build_client(&cfg, &creds.access_key, &creds.secret_key, None);

    // POSITIF : écrire dans le bucket data
    let put_ok = scoped
        .put_object()
        .bucket(data_bucket)
        .key("evidence/a.bin")
        .body(vec![9u8, 9, 9].into())
        .send()
        .await;
    assert!(put_ok.is_ok(), "la clé scopée devrait pouvoir écrire : {put_ok:?}");

    // NÉGATIF 1 : lire l'objet -> refusé
    let get = scoped
        .get_object()
        .bucket(data_bucket)
        .key("evidence/a.bin")
        .send()
        .await;
    assert!(get.is_err(), "la clé scopée ne doit PAS pouvoir lire");

    // NÉGATIF 2 : lister -> refusé
    let list = scoped.list_objects_v2().bucket(data_bucket).send().await;
    assert!(list.is_err(), "la clé scopée ne doit PAS pouvoir lister");

    // NÉGATIF 3 : écrire dans un autre bucket -> refusé
    let put_other = scoped
        .put_object()
        .bucket(other_bucket)
        .key("x")
        .body(vec![1u8].into())
        .send()
        .await;
    assert!(put_other.is_err(), "la clé scopée ne doit écrire QUE dans son bucket");

    // révocation
    iam.delete_scoped_key(&creds.access_key).await.unwrap();

    // NÉGATIF 4 : après révocation, l'écriture échoue
    let put_after = scoped
        .put_object()
        .bucket(data_bucket)
        .key("evidence/b.bin")
        .body(vec![0u8].into())
        .send()
        .await;
    assert!(put_after.is_err(), "la clé révoquée ne doit plus fonctionner");

    admin.delete_bucket(data_bucket).await.unwrap();
    admin.delete_bucket(other_bucket).await.unwrap();
}
