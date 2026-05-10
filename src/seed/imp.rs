// Implementation of TRYBUILD_INHERIT_CACHE seeding. Compiled only when the
// `inherit-cache` Cargo feature is enabled.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use target_triple::TARGET;

const SEEDED_SUBDIRS: &[&str] = &["deps", ".fingerprint", "incremental", "build"];
const SENTINEL: &str = ".trybuild-inherited-at";

pub(crate) fn enabled() -> bool {
    env::var_os("TRYBUILD_INHERIT_CACHE").is_some_and(|v| !v.is_empty())
}

pub(crate) fn seed_target_dir(parent_target: &Path, fixture_target: &Path) {
    if !enabled() {
        return;
    }

    let parent_debug = pick_parent_debug(parent_target);
    let Some(parent_debug) = parent_debug else {
        eprintln!(
            "trybuild: TRYBUILD_INHERIT_CACHE: no parent target found at {}; skipping seed",
            parent_target.display(),
        );
        return;
    };
    let fixture_debug = if cfg!(trybuild_no_target) {
        fixture_target.join("debug")
    } else {
        fixture_target.join(TARGET).join("debug")
    };
    if let Err(err) = fs::create_dir_all(&fixture_debug) {
        eprintln!(
            "trybuild: TRYBUILD_INHERIT_CACHE: cannot create {}: {}",
            fixture_debug.display(),
            err,
        );
        return;
    }

    // Capture parent_max BEFORE seeding so the sentinel records the timestamp
    // of the parent state we are seeding from, not the time the seeding
    // finished. Storing the value in the sentinel's content (rather than
    // relying on its filesystem mtime) closes the race where a parent file
    // mutates between max_mtime() and the post-seed sentinel write.
    let sentinel = fixture_debug.join(SENTINEL);
    let parent_max = max_mtime(&parent_debug).unwrap_or(SystemTime::UNIX_EPOCH);
    if let Some(stored) = read_sentinel_timestamp(&sentinel) {
        if stored >= parent_max {
            return;
        }
    }

    for sub in SEEDED_SUBDIRS {
        let src = parent_debug.join(sub);
        if !src.is_dir() {
            continue;
        }
        let dst = fixture_debug.join(sub);
        if let Err(err) = seed_dir(&src, &dst) {
            eprintln!(
                "trybuild: TRYBUILD_INHERIT_CACHE: skipped {}: {}",
                src.display(),
                err,
            );
        }
    }

    let _ = write_sentinel(&sentinel, parent_max);
}

fn pick_parent_debug(parent_target: &Path) -> Option<PathBuf> {
    let target_prefixed = parent_target.join(TARGET).join("debug");
    if target_prefixed.is_dir() {
        return Some(target_prefixed);
    }
    let plain = parent_target.join("debug");
    if plain.is_dir() {
        return Some(plain);
    }
    None
}

fn seed_dir(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            if let Err(err) = seed_dir(&src_path, &dst_path) {
                eprintln!(
                    "trybuild: TRYBUILD_INHERIT_CACHE: skipped {}: {}",
                    src_path.display(),
                    err,
                );
            }
        } else if file_type.is_file() {
            if dst_is_fresh(&src_path, &dst_path) {
                continue;
            }
            if let Err(err) = reflink_copy::reflink_or_copy(&src_path, &dst_path) {
                eprintln!(
                    "trybuild: TRYBUILD_INHERIT_CACHE: skipped {}: {}",
                    src_path.display(),
                    err,
                );
            }
        }
    }
    Ok(())
}

fn dst_is_fresh(src: &Path, dst: &Path) -> bool {
    let Ok(src_meta) = fs::metadata(src) else {
        return false;
    };
    let Ok(dst_meta) = fs::metadata(dst) else {
        return false;
    };
    match (src_meta.modified(), dst_meta.modified()) {
        (Ok(s), Ok(d)) => d >= s,
        _ => false,
    }
}

fn max_mtime(root: &Path) -> Option<SystemTime> {
    let mut best: Option<SystemTime> = None;
    let mut stack = vec![root.to_owned()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(mtime) = meta.modified() {
                        if best.is_none_or(|b| mtime > b) {
                            best = Some(mtime);
                        }
                    }
                }
            }
        }
    }
    best
}

fn read_sentinel_timestamp(sentinel: &Path) -> Option<SystemTime> {
    let s = fs::read_to_string(sentinel).ok()?;
    let secs: u64 = s.trim().parse().ok()?;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

fn write_sentinel(sentinel: &Path, ts: SystemTime) -> io::Result<()> {
    let secs = ts
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Atomic via temp+rename so a crash mid-write can't leave a partial
    // sentinel that would parse as zero and force a needless re-seed.
    let tmp = sentinel.with_extension("tmp");
    fs::write(&tmp, secs.to_string())?;
    fs::rename(&tmp, sentinel)
}
