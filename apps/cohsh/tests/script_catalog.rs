// Author: Lukas Bower
// Purpose: Lock cohsh .coh grammar coverage for existing regression scripts.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use cohsh::validate_script;

fn script_paths() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cohsh");
    let mut paths = Vec::new();
    for entry in fs::read_dir(root).expect("read scripts/cohsh") {
        let entry = entry.expect("read_dir entry");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("coh") {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

fn record_features(content: &str, features: &mut BTreeSet<String>) {
    for raw_line in content.lines() {
        let trimmed = raw_line.trim_end();
        let without_comment = trimmed
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(trimmed);
        let line = without_comment.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("EXPECT") {
            let selector = rest.trim();
            if selector.starts_with("OK") {
                features.insert("EXPECT OK".to_owned());
            } else if selector.starts_with("ERR") {
                features.insert("EXPECT ERR".to_owned());
            } else if selector.starts_with("SUBSTR") {
                features.insert("EXPECT SUBSTR".to_owned());
            } else if selector.starts_with("NOT") {
                features.insert("EXPECT NOT".to_owned());
            } else {
                features.insert("EXPECT <invalid>".to_owned());
            }
            continue;
        }
        if line.starts_with("WAIT") {
            features.insert("WAIT".to_owned());
            continue;
        }
        if let Some(cmd) = line.split_whitespace().next() {
            features.insert(format!("CMD:{cmd}"));
        }
    }
}

#[test]
fn parses_all_existing_scripts() {
    for path in script_paths() {
        let file = File::open(&path).expect("open script");
        validate_script(BufReader::new(file)).expect("script should parse");
    }
}

#[test]
fn script_feature_inventory_is_stable() {
    let mut features = BTreeSet::new();
    for path in script_paths() {
        let content = fs::read_to_string(&path).expect("read script");
        record_features(&content, &mut features);
    }
    let expected = BTreeSet::from([
        "CMD:attach".to_owned(),
        "CMD:cat".to_owned(),
        "CMD:echo".to_owned(),
        "CMD:help".to_owned(),
        "CMD:log".to_owned(),
        "CMD:quit".to_owned(),
        "EXPECT ERR".to_owned(),
        "EXPECT OK".to_owned(),
        "EXPECT SUBSTR".to_owned(),
        "WAIT".to_owned(),
    ]);
    assert_eq!(features, expected);
}
