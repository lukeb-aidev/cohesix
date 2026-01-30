use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const VALID_STATUSES: [&str; 4] = ["Implemented", "Inherited", "Planned", "NA"];
const VALID_EVIDENCE_TYPES: [&str; 5] = ["doc", "code", "test", "script", "log_fixture"];

#[derive(Debug, Deserialize)]
struct ControlsFile {
    control: Vec<Control>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Control {
    id: String,
    title: String,
    family: String,
    status: String,
    rationale: String,
    #[serde(default)]
    evidence: Vec<Evidence>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Evidence {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "ref")]
    reference: String,
    note: Option<String>,
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest_dir)
}

fn load_controls(root: &Path) -> Result<ControlsFile, String> {
    let controls_path = root.join("docs/nist/controls.toml");
    let content = fs::read_to_string(&controls_path)
        .map_err(|err| format!("failed to read {}: {}", controls_path.display(), err))?;
    toml::from_str(&content).map_err(|err| format!("failed to parse controls.toml: {}", err))
}

fn check_controls(root: &Path, controls: &ControlsFile) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    for control in &controls.control {
        if !VALID_STATUSES.contains(&control.status.as_str()) {
            errors.push(format!(
                "{}: invalid status '{}'",
                control.id, control.status
            ));
        }

        for evidence in &control.evidence {
            if !VALID_EVIDENCE_TYPES.contains(&evidence.kind.as_str()) {
                errors.push(format!(
                    "{}: invalid evidence type '{}'",
                    control.id, evidence.kind
                ));
            }

            if evidence.reference.contains("://") {
                errors.push(format!(
                    "{}: evidence ref '{}' must be repo-relative (no URLs)",
                    control.id, evidence.reference
                ));
            }

            let ref_path = Path::new(&evidence.reference);
            if ref_path.is_absolute() {
                errors.push(format!(
                    "{}: evidence ref '{}' must be repo-relative",
                    control.id, evidence.reference
                ));
            }

            let full_path = root.join(ref_path);
            if !full_path.exists() {
                errors.push(format!(
                    "{}: evidence ref '{}' does not exist",
                    control.id, evidence.reference
                ));
            }
        }

        if control.status == "Implemented" {
            if control.evidence.len() < 2 {
                errors.push(format!(
                    "{}: Implemented controls require at least 2 evidence items",
                    control.id
                ));
            }
            let has_required = control.evidence.iter().any(|evidence| {
                matches!(
                    evidence.kind.as_str(),
                    "test" | "script" | "log_fixture"
                )
            });
            if !has_required {
                errors.push(format!(
                    "{}: Implemented controls require evidence of type test/script/log_fixture",
                    control.id
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn report_md(root: &Path, controls: &ControlsFile) -> Result<(), String> {
    let report_path = root.join("docs/nist/REPORT.md");
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {}", parent.display(), err))?;
    }

    let mut output = String::new();
    output.push_str("# NIST 800-53 LOW Control Registry Report (Generated)\n\n");
    output.push_str("Source: docs/nist/controls.toml\n\n");
    output.push_str("| ID | Family | Status | Title | Evidence refs |\n");
    output.push_str("|---|---|---|---|---|\n");

    for control in &controls.control {
        let evidence_refs = if control.evidence.is_empty() {
            "-".to_string()
        } else {
            control
                .evidence
                .iter()
                .map(|evidence| evidence.reference.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            control.id, control.family, control.status, control.title, evidence_refs
        ));
    }

    fs::write(&report_path, output)
        .map_err(|err| format!("failed to write {}: {}", report_path.display(), err))?;
    Ok(())
}

fn print_usage() {
    eprintln!("usage: security-nist <check|report-md>");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(2);
    }

    let root = repo_root();
    let controls = match load_controls(&root) {
        Ok(controls) => controls,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
    };

    match args[1].as_str() {
        "check" => match check_controls(&root, &controls) {
            Ok(()) => {
                println!("security-nist check ok");
            }
            Err(errors) => {
                for error in errors {
                    eprintln!("error: {error}");
                }
                std::process::exit(1);
            }
        },
        "report-md" => match report_md(&root, &controls) {
            Ok(()) => {
                println!("wrote docs/nist/REPORT.md");
            }
            Err(err) => {
                eprintln!("error: {err}");
                std::process::exit(1);
            }
        },
        _ => {
            print_usage();
            std::process::exit(2);
        }
    }
}
