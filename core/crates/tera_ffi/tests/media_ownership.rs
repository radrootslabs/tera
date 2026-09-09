#![cfg(unix)]

use radroots_blossom::Sha256;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::sync::Arc;
use tera_ffi::{
    FfiAddCommandType, FfiAddDraftInput, FfiBlossomEndpointAuthority, FfiBlossomHostKind,
    FfiMediaFile, FfiPreparedMediaInput, MOBILE_FFI_SCHEMA_VERSION,
    PREPARED_MEDIA_FFI_SCHEMA_VERSION,
};

mod support;

fn photo() -> (tempfile::NamedTempFile, FfiAddDraftInput) {
    let bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x02";
    let mut original = tempfile::NamedTempFile::new().unwrap();
    original.write_all(bytes).unwrap();
    let file =
        Arc::new(FfiMediaFile::new(original.as_raw_fd() as u64, bytes.len() as u64).unwrap());
    let input = FfiAddDraftInput {
        schema_version: MOBILE_FFI_SCHEMA_VERSION,
        command_type: FfiAddCommandType::CreatePhotoUpdate,
        content: "Synthetic owned-file fixture".to_owned(),
        identifier: None,
        title: None,
        summary: None,
        location: None,
        event_timing: None,
        event_start_date: None,
        event_end_date: None,
        event_start_unix_s: None,
        event_end_unix_s: None,
        event_timezone: None,
        price_amount: None,
        currency: None,
        unit: None,
        quantity: None,
        food_published_at_unix_s: None,
        food_status: None,
        media: vec![FfiPreparedMediaInput {
            schema_version: PREPARED_MEDIA_FFI_SCHEMA_VERSION,
            opaque_reference: "media:owned-file".to_owned(),
            file,
            sha256: Sha256::digest(bytes).to_hex(),
            media_type: "image/png".to_owned(),
            byte_size: bytes.len() as u64,
            width: 2,
            height: 2,
            alt: "Synthetic photo".to_owned(),
            prepared_at_unix_s: 1_800_000_000,
        }],
    };
    (original, input)
}

#[tokio::test]
async fn actual_runtime_reads_admitted_bytes_after_close_before_first_poll() {
    let (_root, runtime) = support::runtime().await;
    runtime
        .configure_blossom(
            FfiBlossomHostKind::Simulator,
            FfiBlossomEndpointAuthority::LoopbackDevelopment,
            "http://127.0.0.1:3000".to_owned(),
            vec![],
        )
        .unwrap();
    let (original, input) = photo();
    let digest = input.media[0].sha256.clone();
    let weak = Arc::downgrade(&input.media[0].file);
    let future = runtime.phase1_save_draft(
        "31".repeat(16),
        input,
        1_800_000_000,
        None,
        1_800_000_000_000,
    );
    drop(original);
    assert!(weak.upgrade().is_some());
    let saved = future
        .await
        .expect("owned file survives caller close before first poll");
    assert_eq!(saved.form.unwrap().media[0].sha256, digest);
    assert!(weak.upgrade().is_none());
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_runtime_ignores_reused_descriptor_before_first_poll() {
    let (_root, runtime) = support::runtime().await;
    runtime
        .configure_blossom(
            FfiBlossomHostKind::Simulator,
            FfiBlossomEndpointAuthority::LoopbackDevelopment,
            "http://127.0.0.1:3000".to_owned(),
            vec![],
        )
        .unwrap();
    let (mut original, input) = photo();
    let digest = input.media[0].sha256.clone();
    let future = runtime.phase1_save_draft(
        "32".repeat(16),
        input,
        1_800_000_000,
        None,
        1_800_000_000_000,
    );
    let mut replacement = tempfile::NamedTempFile::new().unwrap();
    replacement.write_all(b"wrong recycled descriptor").unwrap();
    let replacement = std::fs::File::open(replacement.path()).unwrap();
    // SAFETY: this test exclusively owns both live descriptors; original
    // remains the only RAII owner of the atomically replaced target slot.
    assert_eq!(
        unsafe { libc::dup2(replacement.as_raw_fd(), original.as_raw_fd()) },
        original.as_raw_fd()
    );
    let mut observed = String::new();
    original.read_to_string(&mut observed).unwrap();
    assert_eq!(observed, "wrong recycled descriptor");
    let saved = future
        .await
        .expect("reused caller descriptor cannot substitute bytes");
    assert_eq!(saved.form.unwrap().media[0].sha256, digest);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancellation_before_first_poll_releases_only_admitted_owner() {
    let (_root, runtime) = support::runtime().await;
    let (original, input) = photo();
    let weak = Arc::downgrade(&input.media[0].file);
    let future = runtime.phase1_save_draft(
        "33".repeat(16),
        input,
        1_800_000_000,
        None,
        1_800_000_000_000,
    );
    assert!(weak.upgrade().is_some());
    drop(future);
    assert!(weak.upgrade().is_none());
    assert_eq!(original.as_file().metadata().unwrap().len(), 24);
    assert!(runtime.phase1_draft_status("33".repeat(16)).await.is_err());
    runtime.shutdown().await.unwrap();
}
