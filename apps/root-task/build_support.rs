// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the build_support module for root-task.
// Author: Lukas Bower
//! Shared helpers for the root-task build script.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

/// Fixed-width placeholder replaced only after the complete Pi image exists.
///
/// The image staging pipeline hashes the final U-Boot image with this field
/// and the two U-Boot CRC fields normalized, writes that digest here, and then
/// repairs the CRCs. A serial marker can therefore identify the complete image
/// without attempting an impossible literal self-hash.
pub const UNSEALED_PI4_IMAGE_ID: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// Git metadata roots relevant to Cargo rerun tracking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitMetadataDirs {
    /// Per-checkout metadata containing HEAD and the worktree index.
    pub worktree: PathBuf,
    /// Shared metadata containing branch refs and packed refs.
    pub common: PathBuf,
}

/// Select the marker Git identity for ordinary or exact-image builds.
///
/// Exact-image builds begin from a clean committed checkout, then temporarily
/// regenerate target-qualified outputs before compiling. The image wrapper
/// records and continuously checks that expected generated-state fingerprint;
/// this helper permits the already-verified clean commit to remain the marker
/// identity while rejecting malformed or mismatched environment claims.
pub fn select_build_git_hash(
    detected_full: &str,
    detected_short: &str,
    working_tree_dirty: bool,
    exact_commit: Option<&str>,
    exact_source_clean: bool,
) -> Result<String, &'static str> {
    let detected_full = detected_full.trim();
    let detected_short = detected_short.trim();
    let Some(exact_commit) = exact_commit else {
        return Ok(format!(
            "{detected_short}{}",
            if working_tree_dirty { "-dirty" } else { "" }
        ));
    };
    if !exact_source_clean {
        return Err("exact Git commit requires a clean-source attestation");
    }
    if !(40..=64).contains(&exact_commit.len())
        || !exact_commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        || exact_commit.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        return Err("exact Git commit must be a full lowercase hexadecimal object ID");
    }
    if exact_commit != detected_full {
        return Err("exact Git commit does not match repository HEAD");
    }
    if detected_short.len() < 7 || !exact_commit.starts_with(detected_short) {
        return Err("short Git identity is not an unambiguous HEAD prefix");
    }
    Ok(detected_short.to_owned())
}

/// Resolve regular-repository and linked-worktree Git metadata roots.
pub fn resolve_git_metadata_dirs(repo_root: &Path, dot_git: &Path) -> Option<GitMetadataDirs> {
    let worktree = if dot_git.is_dir() {
        dot_git.to_path_buf()
    } else {
        let gitdir = fs::read_to_string(dot_git).ok()?;
        let raw_path = gitdir.trim().strip_prefix("gitdir:")?.trim();
        let path = PathBuf::from(raw_path);
        if path.is_absolute() {
            path
        } else {
            repo_root.join(path)
        }
    };
    let common = fs::read_to_string(worktree.join("commondir"))
        .ok()
        .map(|raw_path| PathBuf::from(raw_path.trim()))
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                worktree.join(path)
            }
        })
        .unwrap_or_else(|| worktree.clone());
    Some(GitMetadataDirs { worktree, common })
}

/// Build-profile flags encoded in the serial and image build marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildMarkerFeatures {
    pub kernel: bool,
    pub bootstrap_trace: bool,
    pub serial_console: bool,
    pub net: bool,
    pub net_console: bool,
    pub qemu_driver_task_smoke: bool,
}

/// Format the exact build marker embedded in the root-task image and emitted
/// on serial. Keeping this in build support lets the build script materialise
/// one contiguous string instead of assembling an unverifiable line at run
/// time.
pub fn format_build_marker(
    git_hash: &str,
    build_timestamp: &str,
    features: BuildMarkerFeatures,
) -> String {
    format!(
        "[BUILD] {git_hash} {build_timestamp} image-id={UNSEALED_PI4_IMAGE_ID} features=[kernel:{} bootstrap-trace:{} serial-console:{} net:{} net-console:{} qemu-driver-task-smoke:{}]",
        u8::from(features.kernel),
        u8::from(features.bootstrap_trace),
        u8::from(features.serial_console),
        u8::from(features.net),
        u8::from(features.net_console),
        u8::from(features.qemu_driver_task_smoke),
    )
}

/// Return whether a generated artifact is stale after the source manifest was
/// changed in the current checkout.
///
/// Fresh checkouts do not preserve commit-time ordering between tracked files,
/// so timestamps are meaningful only when Git reports a local manifest change.
pub fn generated_artifact_is_stale(
    manifest_has_tracked_changes: bool,
    manifest_mtime: SystemTime,
    artifact_mtime: SystemTime,
) -> bool {
    manifest_has_tracked_changes && artifact_mtime < manifest_mtime
}

/// Classification of seL4 linker scripts discovered in the SDK tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkerScriptKind {
    /// Kernel linker script, unsuitable for the root task.
    Kernel,
    /// Userland linker script intended for the root task image.
    User,
    /// Script did not contain recognisable markers.
    Unknown,
}

/// Attempt to classify a linker script located at `path`.
///
/// The classifier uses both path hints and textual markers to avoid
/// accidentally linking the root task with the seL4 kernel script. Using the
/// kernel script inflates the PT_LOAD segment span and causes the ELF-loader to
/// overlap with the staged root task image, preventing the VM from booting.
pub fn classify_linker_script(path: &Path) -> Result<LinkerScriptKind, String> {
    let user_hint = has_path_hint(path, USER_PATH_HINTS);

    if path_contains_component(path, "kernel") {
        return Ok(LinkerScriptKind::Kernel);
    }
    if path_contains_component(path, "elfloader") {
        // The elfloader script links the boot image, not the root-task ELF.
        return Ok(LinkerScriptKind::Kernel);
    }

    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) => {
            if user_hint {
                return Ok(LinkerScriptKind::User);
            }

            return Err(format!("failed to read {}: {}", path.display(), err));
        }
    };

    let classification = classify_linker_script_contents(&contents);
    if classification == LinkerScriptKind::Unknown && user_hint {
        Ok(LinkerScriptKind::User)
    } else {
        Ok(classification)
    }
}

/// Parse the seL4 platform timer frequency from a generated platform header.
///
/// seL4 emits platform timer clocks in forms such as
/// `#define TIMER_CLOCK_HZ ULL_CONST(54000000)`. The root-task build uses this
/// generated value as the single source of truth for converting architectural
/// counter ticks into milliseconds.
pub fn parse_timer_clock_hz(contents: &str) -> Option<u64> {
    contents.lines().find_map(parse_timer_clock_hz_line)
}

fn parse_timer_clock_hz_line(raw_line: &str) -> Option<u64> {
    let line = raw_line.trim();
    let value = line.strip_prefix("#define TIMER_CLOCK_HZ")?.trim();
    parse_first_decimal_u64(value)
}

fn parse_first_decimal_u64(value: &str) -> Option<u64> {
    let mut digits = String::new();
    let mut started = false;
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            started = true;
            digits.push(ch);
            continue;
        }
        if started {
            break;
        }
    }
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

fn classify_linker_script_contents(contents: &str) -> LinkerScriptKind {
    let mut lower = contents.to_ascii_lowercase();

    let has_kernel_marker = KERNEL_MARKERS.iter().any(|marker| lower.contains(marker));
    let has_rootserver_marker = ROOTSERVER_MARKERS
        .iter()
        .any(|marker| lower.contains(marker));
    let has_user_marker = USER_MARKERS.iter().any(|marker| lower.contains(marker));

    // Drop the temporary buffer eagerly to avoid holding on to a large
    // allocation when the caller retries classification with additional
    // context.
    lower.clear();

    match (has_kernel_marker, has_rootserver_marker, has_user_marker) {
        (true, true, _) => LinkerScriptKind::Unknown,
        (true, false, _) => LinkerScriptKind::Kernel,
        (false, true, _) => LinkerScriptKind::User,
        (false, false, true) => LinkerScriptKind::User,
        (false, false, false) => LinkerScriptKind::Unknown,
    }
}

fn path_contains_component(path: &Path, needle: &str) -> bool {
    path.components().any(|component| match component {
        Component::Normal(part) => part
            .to_str()
            .map(|value| value.eq_ignore_ascii_case(needle))
            .unwrap_or(false),
        _ => false,
    })
}

fn has_path_hint(path: &Path, hints: &[&str]) -> bool {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    hints.iter().any(|hint| lower.contains(hint))
}

const KERNEL_MARKERS: &[&str] = &[
    "kernel_elf_base",
    "kernel_elf_base_raw",
    "kernel_elf_paddr_base",
    "kernel_elf_paddr_base_raw",
    "kernel_elf_paddr_offset",
    "kernel_window",
    "kernel_virt_offset",
    "kload_paddr",
    "kload_vaddr",
    "kernel_offset",
    "ki_boot_end",
    "ki_end",
];

const ROOTSERVER_MARKERS: &[&str] = &[
    "rootserver",
    "sel4runtime",
    "rootserver_stack",
    "rootserver_objects",
    "rootserver_extra_bi",
];

const USER_MARKERS: &[&str] = &[
    "user_top",
    "sel4_usertop",
    "user_window",
    "sel4_userimagebase",
    "_user_image",
    "rootserver_image_base",
    "rootserver_elf_paddr_base",
    "rootserver_stack_bottom",
    "rootserver_stack_top",
    "rootserver_objects_start",
    "rootserver_objects_end",
];

const USER_PATH_HINTS: &[&str] = &["rootserver", "sel4runtime"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn build_marker_is_one_exact_serial_and_image_identity() {
        assert_eq!(
            format_build_marker(
                "abc123-dirty",
                "2026-07-16T00:00:00Z",
                BuildMarkerFeatures {
                    kernel: true,
                    bootstrap_trace: true,
                    serial_console: true,
                    net: true,
                    net_console: true,
                    qemu_driver_task_smoke: false,
                },
            ),
            "[BUILD] abc123-dirty 2026-07-16T00:00:00Z image-id=0000000000000000000000000000000000000000000000000000000000000000 features=[kernel:1 bootstrap-trace:1 serial-console:1 net:1 net-console:1 qemu-driver-task-smoke:0]"
        );
    }

    #[test]
    fn linked_worktree_resolves_shared_branch_metadata() {
        let repo = TempDir::new().expect("failed to create repository fixture");
        let common = repo.path().join("repo.git");
        let worktree = common.join("worktrees/fixture");
        fs::create_dir_all(&worktree).expect("failed to create worktree metadata");
        fs::write(
            repo.path().join(".git"),
            "gitdir: repo.git/worktrees/fixture\n",
        )
        .expect("failed to write .git pointer");
        fs::write(worktree.join("commondir"), "../..\n")
            .expect("failed to write commondir pointer");

        let resolved = resolve_git_metadata_dirs(repo.path(), &repo.path().join(".git"))
            .expect("failed to resolve linked worktree metadata");

        assert_eq!(resolved.worktree, worktree);
        assert_eq!(resolved.common, worktree.join("../.."));
    }

    #[test]
    fn ordinary_git_identity_records_untracked_or_tracked_dirtiness() {
        let clean = select_build_git_hash(
            "abcdef0123456789abcdef0123456789abcdef01",
            "abcdef012345",
            false,
            None,
            false,
        )
        .expect("clean identity must be accepted");
        let dirty = select_build_git_hash(
            "abcdef0123456789abcdef0123456789abcdef01",
            "abcdef012345",
            true,
            None,
            false,
        )
        .expect("dirty identity must be accepted");

        assert_eq!(clean, "abcdef012345");
        assert_eq!(dirty, "abcdef012345-dirty");
    }

    #[test]
    fn exact_git_identity_requires_clean_matching_full_commit() {
        let full = "abcdef0123456789abcdef0123456789abcdef01";

        assert_eq!(
            select_build_git_hash(full, "abcdef012345", true, Some(full), true),
            Ok("abcdef012345".to_owned())
        );
        assert_eq!(
            select_build_git_hash(full, "abcdef012345", true, Some(full), false),
            Err("exact Git commit requires a clean-source attestation")
        );
        assert_eq!(
            select_build_git_hash(full, "abcdef012345", true, Some(&"0".repeat(40)), true),
            Err("exact Git commit does not match repository HEAD")
        );
        assert_eq!(
            select_build_git_hash(full, "abcdef012345", true, Some("ABCDEF0"), true),
            Err("exact Git commit must be a full lowercase hexadecimal object ID")
        );
    }

    #[test]
    fn kernel_component_in_path_short_circuits() {
        assert_eq!(
            classify_linker_script(Path::new("kernel/linker.lds")).unwrap(),
            LinkerScriptKind::Kernel
        );
    }

    #[test]
    fn user_hint_in_path_short_circuits() {
        assert_eq!(
            classify_linker_script(Path::new("build/rootserver/linker.lds")).unwrap(),
            LinkerScriptKind::User
        );
    }

    #[test]
    fn user_hint_beats_kernel_component_when_both_present() {
        assert_eq!(
            classify_linker_script(Path::new("kernel/gen_config/rootserver/linker.lds")).unwrap(),
            LinkerScriptKind::Kernel
        );
    }

    #[test]
    fn kernel_path_detection_is_case_insensitive() {
        assert!(path_contains_component(
            Path::new("KERNEL/sel4.ld"),
            "kernel"
        ));
    }

    #[test]
    fn user_hint_detection_is_case_insensitive() {
        assert!(has_path_hint(
            Path::new("Build/SeL4Runtime/linker.lds"),
            USER_PATH_HINTS
        ));
    }

    #[test]
    fn elfloader_script_is_rejected_for_root_task() {
        assert_eq!(
            classify_linker_script(Path::new("build/elfloader/linker.lds_pp")).unwrap(),
            LinkerScriptKind::Kernel
        );
    }

    #[test]
    fn detects_kernel_marker() {
        assert_eq!(
            classify_linker_script_contents("/* KERNEL_ELF_BASE */"),
            LinkerScriptKind::Kernel
        );
    }

    #[test]
    fn detects_user_marker() {
        assert_eq!(
            classify_linker_script_contents("/* USER_TOP */"),
            LinkerScriptKind::User
        );
    }

    #[test]
    fn unknown_without_markers() {
        assert_eq!(
            classify_linker_script_contents("/* no hints */"),
            LinkerScriptKind::Unknown
        );
    }

    #[test]
    fn parses_platform_timer_clock_hz_from_ull_const() {
        assert_eq!(
            parse_timer_clock_hz("#define TIMER_CLOCK_HZ ULL_CONST(54000000)\n"),
            Some(54_000_000)
        );
    }

    #[test]
    fn parses_platform_timer_clock_hz_from_plain_integer() {
        assert_eq!(
            parse_timer_clock_hz("#define TIMER_CLOCK_HZ 62500000\n"),
            Some(62_500_000)
        );
    }

    #[test]
    fn clean_checkout_ignores_artifact_write_order() {
        assert!(!generated_artifact_is_stale(
            false,
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(2),
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1),
        ));
    }

    #[test]
    fn changed_manifest_rejects_older_artifact() {
        assert!(generated_artifact_is_stale(
            true,
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(2),
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1),
        ));
    }

    #[test]
    fn changed_manifest_accepts_regenerated_artifact() {
        assert!(!generated_artifact_is_stale(
            true,
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1),
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(2),
        ));
    }
}
