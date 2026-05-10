// Seed trybuild's isolated target subdir from the parent workspace's build
// artifacts so the spawned cargo can reuse them. Only active under the
// `inherit-cache` Cargo feature; otherwise this module is a no-op shim and
// the reflink-copy dependency is not pulled in.

#[cfg(feature = "inherit-cache")]
mod imp;

pub(crate) fn enabled() -> bool {
    #[cfg(feature = "inherit-cache")]
    {
        imp::enabled()
    }
    #[cfg(not(feature = "inherit-cache"))]
    {
        false
    }
}

#[cfg(feature = "inherit-cache")]
pub(crate) fn seed_target_dir(parent_target: &std::path::Path, fixture_target: &std::path::Path) {
    imp::seed_target_dir(parent_target, fixture_target);
}

#[cfg(not(feature = "inherit-cache"))]
pub(crate) fn seed_target_dir(_parent_target: &std::path::Path, _fixture_target: &std::path::Path) {
    // No-op when the inherit-cache feature is disabled.
}
