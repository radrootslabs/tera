use std::{env, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUSTC");
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-env-changed=RADROOTS_SOURCE_SHA");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    let rustc = env::var("RUSTC").expect("missing required env var RUSTC");
    if let Ok(output) = Command::new(rustc).arg("--version").output()
        && output.status.success()
        && let Ok(version) = String::from_utf8(output.stdout)
    {
        println!("cargo:rustc-env=RUSTC_VERSION={}", version.trim());
    }

    if let Some(source_sha) = optional_source_sha() {
        println!("cargo:rustc-env=GIT_HASH={source_sha}");
    }

    let profile = env::var("PROFILE").expect("missing required env var PROFILE");
    println!("cargo:rustc-env=PROFILE={profile}");

    if let Some(epoch) = optional_source_date_epoch() {
        println!("cargo:rustc-env=BUILD_TIME_UNIX={epoch}");
    }
}

fn optional_source_sha() -> Option<String> {
    let value = env::var("RADROOTS_SOURCE_SHA").ok()?;
    assert!(
        (7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "RADROOTS_SOURCE_SHA must contain 7 to 64 hexadecimal characters"
    );
    Some(value.to_ascii_lowercase())
}

fn optional_source_date_epoch() -> Option<u64> {
    let value = env::var("SOURCE_DATE_EPOCH").ok()?;
    Some(
        value
            .parse()
            .expect("SOURCE_DATE_EPOCH must be an unsigned Unix timestamp"),
    )
}
