use intake_core::integrity::{build_manifest, verify};

#[test]
fn manifest_then_verify_detects_tampering() {
    let src = std::env::temp_dir().join("intake-integrity-src");
    let _ = std::fs::remove_dir_all(&src);
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.bin"), b"hello").unwrap();
    std::fs::write(src.join("b.bin"), b"world").unwrap();

    let manifest = build_manifest(&[src.join("a.bin"), src.join("b.bin")]).unwrap();
    assert_eq!(manifest.len(), 2);

    // "downloaded" objects: same bytes, keys carry a data/ prefix + timestamp
    let dl = std::env::temp_dir().join("intake-integrity-dl");
    let _ = std::fs::remove_dir_all(&dl);
    std::fs::create_dir_all(&dl).unwrap();
    let a = dl.join("a"); std::fs::write(&a, b"hello").unwrap();
    let b = dl.join("b"); std::fs::write(&b, b"world").unwrap();
    let downloaded = vec![
        ("data/1699999999-a.bin".to_string(), a),
        ("data/1699999999-b.bin".to_string(), b.clone()),
    ];

    // everything matches (by content, despite the timestamped keys)
    let ok = verify(&downloaded, &manifest).unwrap();
    assert!(ok.is_ok(), "expected OK: {ok:?}");
    assert_eq!(ok.matched.len(), 2);

    // tamper a received object -> MISSING (expected not received) + UNEXPECTED (unknown received)
    std::fs::write(&b, b"tampered").unwrap();
    let bad = verify(&downloaded, &manifest).unwrap();
    assert!(!bad.is_ok());
    assert_eq!(bad.unexpected.len(), 1);
    assert_eq!(bad.missing.len(), 1);
}
