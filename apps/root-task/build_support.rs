// Author: Lukas Bower
//! Shared helpers for the root-task build script.

use std::fs;
use std::path::{Component, Path};

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
    if path_contains_component(path, "kernel") {
        return Ok(LinkerScriptKind::Kernel);
    }

    if has_path_hint(path, USER_PATH_HINTS) {
        return Ok(LinkerScriptKind::User);
    }

    let contents = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {}", path.display(), err))?;

    Ok(classify_linker_script_contents(&contents))
}

fn classify_linker_script_contents(contents: &str) -> LinkerScriptKind {
    let mut lower = contents.to_ascii_lowercase();

    if KERNEL_MARKERS.iter().any(|marker| lower.contains(marker)) {
        return LinkerScriptKind::Kernel;
    }

    if USER_MARKERS.iter().any(|marker| lower.contains(marker)) {
        return LinkerScriptKind::User;
    }

    // Drop the temporary buffer eagerly to avoid holding on to a large
    // allocation when the caller retries classification with additional
    // context.
    lower.clear();

    LinkerScriptKind::Unknown
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
    "kernel_elf_paddr_base",
    "kernel_elf_paddr_offset",
    "kernel_window",
    "kernel_virt_offset",
];

const USER_MARKERS: &[&str] = &[
    "user_top",
    "sel4_usertop",
    "user_window",
    "sel4_userimagebase",
    "_user_image",
    "rootserver_image_base",
    "rootserver_elf_paddr_base",
];

const USER_PATH_HINTS: &[&str] = &["rootserver", "sel4runtime"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
}
