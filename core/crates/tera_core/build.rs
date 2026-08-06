use std::{env, process::Command};

#[path = "src/provenance.rs"]
mod provenance;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUSTC");
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-env-changed=RADROOTS_LIB_REVISION");
    println!("cargo:rerun-if-env-changed=RADROOTS_CONSUMER_REVISION");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    let rustc = env::var("RUSTC").expect("missing required env var RUSTC");
    if let Ok(output) = Command::new(rustc).arg("--version").output()
        && output.status.success()
        && let Ok(version) = String::from_utf8(output.stdout)
    {
        println!("cargo:rustc-env=RUSTC_VERSION={}", version.trim());
    }

    let lib_revision = optional_full_revision("RADROOTS_LIB_REVISION");
    let consumer_revision = optional_full_revision("RADROOTS_CONSUMER_REVISION");
    assert!(
        consumer_revision.is_none() || lib_revision.is_some(),
        "RADROOTS_CONSUMER_REVISION requires RADROOTS_LIB_REVISION"
    );
    if let Some(revision) = lib_revision {
        println!("cargo:rustc-env=RADROOTS_LIB_REVISION={revision}");
    }
    if let Some(revision) = consumer_revision {
        println!("cargo:rustc-env=RADROOTS_CONSUMER_REVISION={revision}");
    }

    let profile = env::var("PROFILE").expect("missing required env var PROFILE");
    println!("cargo:rustc-env=PROFILE={profile}");

    if let Some(epoch) = optional_source_date_epoch() {
        println!("cargo:rustc-env=BUILD_TIME_UNIX={epoch}");
    }
}

fn optional_full_revision(name: &str) -> Option<String> {
    let value = env::var(name).ok()?;
    assert!(
        provenance::is_full_revision(&value),
        "{name} must contain exactly 40 lowercase hexadecimal characters"
    );
    Some(value)
}

fn optional_source_date_epoch() -> Option<u64> {
    let value = env::var("SOURCE_DATE_EPOCH").ok()?;
    Some(
        value
            .parse()
            .expect("SOURCE_DATE_EPOCH must be an unsigned Unix timestamp"),
    )
}
