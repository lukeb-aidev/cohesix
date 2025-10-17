// Author: Lukas Bower

#[path = "../build_support.rs"]
mod build_support;

use build_support::{classify_linker_script, LinkerScriptKind};
use std::fs;
use std::path::Path;
use tempfile::{NamedTempFile, TempPath};

fn write_temp_script(contents: &str) -> TempPath {
    let file = NamedTempFile::new().expect("failed to create temporary linker script");
    fs::write(file.path(), contents).expect("failed to write temporary linker script");
    file.into_temp_path()
}

#[test]
fn kernel_component_hint_trumps_missing_file() {
    let kind = classify_linker_script(Path::new("kernel/linker.lds")).unwrap();
    assert_eq!(kind, LinkerScriptKind::Kernel);
}

#[test]
fn user_component_hint_treated_as_user() {
    let kind = classify_linker_script(Path::new("build/rootserver/linker.lds")).unwrap();
    assert_eq!(kind, LinkerScriptKind::User);
}

#[test]
fn kernel_marker_in_content_detected() {
    let path = write_temp_script("SECTIONS { /* KERNEL_ELF_PADDR_BASE */ }");
    let kind = classify_linker_script(path.as_ref()).unwrap();
    assert_eq!(kind, LinkerScriptKind::Kernel);
}

#[test]
fn user_marker_in_content_detected() {
    let path = write_temp_script("SECTIONS { /* USER_TOP */ }");
    let kind = classify_linker_script(path.as_ref()).unwrap();
    assert_eq!(kind, LinkerScriptKind::User);
}

#[test]
fn scripts_without_markers_are_reported_unknown() {
    let path = write_temp_script("SECTIONS { /* nothing */ }");
    let kind = classify_linker_script(path.as_ref()).unwrap();
    assert_eq!(kind, LinkerScriptKind::Unknown);
}
