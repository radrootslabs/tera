use super::fixture::*;

#[tokio::test]
async fn pre_refactor_owner_bytes_upgrade_without_changing_any_acknowledged_identity() {
    let fixture = Fixture::copy();
    assert_eq!(fixture.expected["databases"]["runtime"]["user_version"], 13);
    assert_eq!(fixture.expected["draft_count"], 106);
    for _ in 0..2 {
        let runtime = fixture.open(false).await;
        fixture.assert_revisions(&runtime).await;
        fixture.assert_statuses(&runtime).await;
        runtime.shutdown().await.unwrap();
    }
    assert_eq!(
        hash(&std::fs::read(fixture.blob()).unwrap()),
        field(&fixture.host, "media_sha256")
    );
    fixture.copy_for_old_reader_probe();
}

#[tokio::test]
async fn legacy_missing_companion_refuses_upgrade_without_inventing_empty_state() {
    let fixture = Fixture::copy();
    let config = fixture.config(false);
    let private = config.owner_directory().join("private.sqlite");
    let retained = private.with_extension("retained");
    std::fs::rename(&private, &retained).unwrap();
    let runtime = config.owner_directory().join("runtime.sqlite");
    let before = hash(&std::fs::read(&runtime).unwrap());
    let result = crate::runtime::builder::RuntimeBuilder::new(config)
        .build()
        .await;
    match result {
        Err(crate::TeraAppError::Store { report }) => assert_eq!(report.code, "store_incomplete"),
        _ => panic!("Missing historical companion must produce the typed incomplete-store refusal"),
    }
    assert!(!private.exists());
    assert_eq!(hash(&std::fs::read(runtime).unwrap()), before);
    std::fs::rename(retained, private).unwrap();
    let runtime = fixture.open(false).await;
    fixture.assert_revisions(&runtime).await;
    fixture.assert_statuses(&runtime).await;
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn legacy_future_version_and_damaged_catalog_refuse_without_rewriting_acknowledged_bytes() {
    for name in ["unsupported_version", "damaged_catalog"] {
        let fixture = Fixture::copy();
        let config = fixture.config(false);
        let runtime_path = config.owner_directory().join("runtime.sqlite");
        let pristine = std::fs::read(&runtime_path).unwrap();
        let negative = std::fs::read(
            fixture
                .root
                .path()
                .join("refusal_inputs")
                .join(name)
                .join("runtime.sqlite"),
        )
        .unwrap();
        let derivative = &fixture.expected["negative_derivatives"][name];
        assert_eq!(hash(&pristine), field(derivative, "source_sha256"));
        assert_eq!(hash(&negative), field(derivative, "derived_sha256"));
        std::fs::write(&runtime_path, &negative).unwrap();
        let private_path = config.owner_directory().join("private.sqlite");
        let private_before = std::fs::read(&private_path).unwrap();
        let result = crate::runtime::builder::RuntimeBuilder::new(config)
            .build()
            .await;
        let expected_code = if name == "unsupported_version" {
            "schema_too_new"
        } else {
            "storage_integrity_failed"
        };
        match result {
            Err(crate::TeraAppError::Sdk { report }) => assert_eq!(report.code, expected_code),
            _ => {
                panic!("Negative historical input must produce its specific typed storage refusal")
            }
        }
        assert_eq!(std::fs::read(&runtime_path).unwrap(), negative);
        assert_eq!(std::fs::read(private_path).unwrap(), private_before);
        // Recovery replaces only the disposable negative test copy with its
        // original already-hashed bytes. Production never rewinds a schema.
        std::fs::write(runtime_path, pristine).unwrap();
        let runtime = fixture.open(false).await;
        fixture.assert_revisions(&runtime).await;
        fixture.assert_statuses(&runtime).await;
        runtime.shutdown().await.unwrap();
    }
}
