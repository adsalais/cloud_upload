use intake_core::case::{Case, CaseState};

#[test]
fn save_then_load_roundtrip() {
    let dir = std::env::temp_dir().join("intake-cases-test");
    let _ = std::fs::remove_dir_all(&dir);
    let dir_s = dir.to_string_lossy().to_string();

    let c = Case {
        id: "acme-2026".into(),
        bucket: "intake-acme-2026-abcd".into(),
        scoped_access_key: "AKIAEXAMPLE".into(),
        site_url: "http://localhost:9000/intake-acme-2026-abcd/site/index.html".into(),
        state: CaseState::Active,
    };
    c.save(&dir_s).unwrap();

    let loaded = Case::load(&dir_s, "acme-2026").unwrap();
    assert_eq!(loaded.bucket, c.bucket);
    assert_eq!(loaded.scoped_access_key, "AKIAEXAMPLE");
    assert!(matches!(loaded.state, CaseState::Active));
}
