//! Shared upper bounds for settings and all inbound-cache admission paths.
#[cfg(feature = "mobile-social")]
pub(super) const MEDIA_CACHE_MIN_BYTES: u64 = 16 * 1024 * 1024;
pub(super) const MEDIA_CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub(super) const MEDIA_CACHE_MAX_ARTIFACTS: u32 = 10_000;
