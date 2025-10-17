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
    let user_hint = has_path_hint(path, USER_PATH_HINTS);

    if path_contains_component(path, "kernel") {
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

const USER_PATH_HINTS: &[&str] = &["rootserver", "sel4runtime", "elfloader"];

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
    fn elfloader_hint_is_treated_as_user() {
        assert_eq!(
            classify_linker_script(Path::new("build/elfloader/linker.lds_pp")).unwrap(),
            LinkerScriptKind::User
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
}
