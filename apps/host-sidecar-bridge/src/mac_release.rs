// Author: Lukas Bower
// Purpose: Verify native macOS release results against immutable selected inputs and bounded Apple processing records.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use crate::observations::command;
use anyhow::{anyhow, bail, ensure, Result};
use cohesix_authority::mac_release::{Operation, Target};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MAX_FILES: usize = 8192;
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn native(program: &str, args: &[&str], deadline: Instant) -> Result<Vec<u8>> {
    command(Path::new(program), args, deadline)
}
fn text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| anyhow!("EPERM native-path"))
}
fn token(value: &Value, key: &str) -> Result<String> {
    let s = value[key]
        .as_str()
        .ok_or_else(|| anyhow!("unconfirmed native-field {key}"))?;
    cohesix_authority::validate_id(s).map_err(|_| anyhow!("unconfirmed native-id"))?;
    Ok(s.to_owned())
}

/// Content and executable-mode identity, including only in-tree relative symlinks.
/// No ignored files or platform-dependent traversal order affect this digest.
pub fn artifact_digest(path: &Path) -> Result<String> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        !meta.file_type().is_symlink(),
        "EPERM artifact-root-symlink"
    );
    let mut remaining = MAX_BYTES;
    if meta.is_file() {
        return file_digest(path, &mut remaining);
    }
    ensure!(meta.is_dir(), "EPERM artifact-kind");
    let root = path.canonicalize()?;
    let mut pending = vec![PathBuf::new()];
    let mut rows = Vec::new();
    while let Some(relative) = pending.pop() {
        let entries = fs::read_dir(root.join(&relative))?;
        for entry in entries {
            let entry = entry?;
            let name = relative.join(entry.file_name());
            ensure!(
                rows.len() < MAX_FILES && name.components().count() <= 32,
                "ELIMIT native-tree"
            );
            let name_text = text(&name)?.to_owned();
            let meta = fs::symlink_metadata(entry.path())?;
            let (kind, hash) = if meta.file_type().is_symlink() {
                let target = fs::read_link(entry.path())?;
                ensure!(
                    !target.is_absolute() && entry.path().canonicalize()?.starts_with(&root),
                    "EPERM native-symlink-escape"
                );
                (
                    "symlink",
                    hex::encode(Sha256::digest(text(&target)?.as_bytes())),
                )
            } else if meta.is_dir() {
                pending.push(name);
                ("directory", String::new())
            } else {
                ensure!(meta.is_file(), "EPERM native-tree-special-file");
                ("file", file_digest(&entry.path(), &mut remaining)?)
            };
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                meta.permissions().mode() & 0o111
            };
            #[cfg(not(unix))]
            let executable = 0;
            rows.push((name_text, kind, executable, hash));
        }
    }
    rows.sort();
    let mut digest = Sha256::new();
    digest.update(b"cohesix-native-tree/v1\0");
    digest.update(serde_json::to_vec(&rows)?);
    Ok(hex::encode(digest.finalize()))
}
fn file_digest(path: &Path, remaining: &mut u64) -> Result<String> {
    let mut input = fs::File::open(path)?.take(*remaining + 1);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        *remaining = remaining
            .checked_sub(n as u64)
            .ok_or_else(|| anyhow!("ELIMIT native-artifact"))?;
        digest.update(&buffer[..n]);
    }
    Ok(hex::encode(digest.finalize()))
}

/// Interpret Apple result objects, never a successful command transcript.
pub fn verify_xcode(action: &str, summary: &Value) -> Result<()> {
    let start = summary["startTime"]
        .as_f64()
        .ok_or_else(|| anyhow!("unconfirmed xcode-start"))?;
    let end_key = if action == "mac_release.test" {
        "finishTime"
    } else {
        "endTime"
    };
    let end = summary[end_key]
        .as_f64()
        .ok_or_else(|| anyhow!("unconfirmed xcode-end"))?;
    ensure!(
        start.is_finite() && start > 0.0 && end.is_finite() && end >= start,
        "unconfirmed xcode-time"
    );
    if action == "mac_release.test" {
        let count = summary["totalTestCount"]
            .as_u64()
            .ok_or_else(|| anyhow!("unconfirmed xcode-tests"))?;
        ensure!(
            summary["result"] == "Passed"
                && count > 0
                && summary["passedTests"].as_u64() == Some(count)
                && summary["failedTests"] == 0
                && summary["skippedTests"] == 0
                && summary["expectedFailures"] == 0,
            "unconfirmed xcode-tests"
        );
    } else {
        ensure!(
            matches!(action, "mac_release.build" | "mac_release.archive")
                && summary["status"] == "succeeded"
                && summary["errorCount"] == 0
                && summary["errors"].as_array().is_some_and(Vec::is_empty),
            "unconfirmed xcode-build"
        );
    }
    Ok(())
}

/// Redact test names, device identifiers, paths and diagnostic payloads from public results.
fn project_xcode(summary: &Value) -> Value {
    let mut output = serde_json::Map::new();
    for key in [
        "status",
        "result",
        "startTime",
        "endTime",
        "finishTime",
        "errorCount",
        "warningCount",
        "totalTestCount",
        "passedTests",
        "failedTests",
        "skippedTests",
        "expectedFailures",
    ] {
        if let Some(value) = summary.get(key) {
            output.insert(key.to_owned(), value.clone());
        }
    }
    Value::Object(output)
}

/// Accepted Apple notarization logs bind the submission to the uploaded byte digest.
pub fn verify_notary(log: &Value, id: &str, sha256: &str) -> Result<()> {
    ensure!(
        log["jobId"] == id
            && log["sha256"] == sha256
            && log["status"] == "Accepted"
            && log["statusCode"] == 0,
        "unconfirmed notary-artifact"
    );
    Ok(())
}

/// Select bounded status fields; no names, serial numbers, users or recovery keys.
pub fn compliance(deadline: Instant) -> Result<Value> {
    ensure!(cfg!(target_os = "macos"), "not_supported macos-host");
    let vault = String::from_utf8(native("/usr/bin/fdesetup", &["status"], deadline)?)?;
    let gatekeeper = String::from_utf8(native("/usr/sbin/spctl", &["--status"], deadline)?)?;
    let sip = String::from_utf8(native("/usr/bin/csrutil", &["status"], deadline)?)?;
    let filevault = match vault.trim() {
        "FileVault is On." => true,
        "FileVault is Off." => false,
        _ => bail!("unconfirmed filevault-state"),
    };
    let gatekeeper = match gatekeeper.trim() {
        "assessments enabled" => true,
        "assessments disabled" => false,
        _ => bail!("unconfirmed gatekeeper-state"),
    };
    let sip = match sip.trim() {
        "System Integrity Protection status: enabled." => true,
        "System Integrity Protection status: disabled." => false,
        _ => bail!("unconfirmed sip-state"),
    };
    Ok(
        json!({"schema":"cohesix-endpoint-observation/v1","filevault_enabled":filevault,
        "gatekeeper_enabled":gatekeeper,"sip_enabled":sip,"all_observed_controls_enabled":filevault && gatekeeper && sip,
        "device_attested":false,"certifies_compliance":false}),
    )
}

/// One admitted attempt owns a new private directory. Restart never blindly resubmits.
pub fn execute(target: &Target, state: &Path, deadline: Instant) -> Result<Value> {
    ensure!(cfg!(target_os = "macos"), "not_supported macos-host");
    cohesix_authority::mac_release::validate(std::slice::from_ref(target))
        .map_err(anyhow::Error::msg)?;
    if matches!(target.operation, Operation::Compliance) {
        return compliance(deadline);
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(state)
        .map_err(|_| anyhow!("ambiguous macos-attempt-already-exists-or-unavailable"))?;
    let version = String::from_utf8(native(
        "/usr/bin/xcrun",
        &["xcodebuild", "-version"],
        deadline,
    )?)?;
    let mut result = match &target.operation {
        Operation::Build { source }
        | Operation::Test { source }
        | Operation::Archive { source } => {
            ensure!(
                artifact_digest(&source.root)? == source.tree_sha256,
                "EPERM source-digest"
            );
            let frozen = state.join("source");
            native(
                "/usr/bin/ditto",
                &[text(&source.root)?, text(&frozen)?],
                deadline,
            )?;
            ensure!(
                artifact_digest(&frozen)? == source.tree_sha256,
                "EPERM source-copy-digest"
            );
            let project = frozen.join(&source.project);
            let bundle = state.join("result.xcresult");
            let derived = state.join("DerivedData");
            let archive = state.join("Product.xcarchive");
            let verb = target
                .operation
                .action()
                .strip_prefix("mac_release.")
                .ok_or_else(|| anyhow!("EPERM xcode-action"))?;
            let mut args = vec![
                "xcodebuild",
                "-quiet",
                "-project",
                text(&project)?,
                "-scheme",
                &source.scheme,
                "-destination",
                "platform=macOS",
                "-derivedDataPath",
                text(&derived)?,
                "-resultBundlePath",
                text(&bundle)?,
                "-disableAutomaticPackageResolution",
                "-skipPackageUpdates",
                "CODE_SIGNING_ALLOWED=NO",
                verb,
            ];
            if verb == "archive" {
                args.extend(["-archivePath", text(&archive)?]);
            }
            native("/usr/bin/xcrun", &args, deadline)?;
            let summary_args = if verb == "test" {
                vec![
                    "xcresulttool",
                    "get",
                    "test-results",
                    "summary",
                    "--path",
                    text(&bundle)?,
                ]
            } else {
                vec![
                    "xcresulttool",
                    "get",
                    "build-results",
                    "--path",
                    text(&bundle)?,
                ]
            };
            let summary: Value =
                serde_json::from_slice(&native("/usr/bin/xcrun", &summary_args, deadline)?)?;
            verify_xcode(target.operation.action(), &summary)?;
            let output = if verb == "archive" {
                archive
            } else {
                derived.join("Build/Products")
            };
            json!({"source_sha256":source.tree_sha256,"result_sha256":artifact_digest(&bundle)?,
                "output_sha256":artifact_digest(&output)?,"output_path":output,"native_summary":project_xcode(&summary)})
        }
        Operation::Codesign {
            input,
            identity_sha1,
        } => {
            let artifact = copy_input(input, state, deadline)?;
            native(
                "/usr/bin/codesign",
                &[
                    "--force",
                    "--sign",
                    identity_sha1,
                    "--options",
                    "runtime",
                    "--timestamp",
                    text(&artifact)?,
                ],
                deadline,
            )?;
            native(
                "/usr/bin/codesign",
                &[
                    "--verify",
                    "--deep",
                    "--strict",
                    "-R",
                    &format!("certificate leaf = H\"{identity_sha1}\""),
                    text(&artifact)?,
                ],
                deadline,
            )?;
            json!({"input_sha256":input.sha256,"output_sha256":artifact_digest(&artifact)?,"output_path":artifact,
                "identity_sha1":identity_sha1,"signature_verified":true})
        }
        Operation::Notarize {
            input,
            keychain_profile,
        } => {
            let artifact = copy_input(input, state, deadline)?;
            ensure!(
                artifact.is_file(),
                "EPERM notarization-requires-distribution-file"
            );
            let submitted: Value = serde_json::from_slice(&native(
                "/usr/bin/xcrun",
                &[
                    "notarytool",
                    "submit",
                    text(&artifact)?,
                    "--keychain-profile",
                    keychain_profile,
                    "--wait",
                    "--output-format",
                    "json",
                ],
                deadline,
            )?)?;
            let id = token(&submitted, "id")?;
            let log: Value = serde_json::from_slice(&native(
                "/usr/bin/xcrun",
                &[
                    "notarytool",
                    "log",
                    &id,
                    "--keychain-profile",
                    keychain_profile,
                ],
                deadline,
            )?)?;
            verify_notary(&log, &id, &input.sha256)?;
            ensure!(
                artifact_digest(&artifact)? == input.sha256,
                "EPERM changed-notary-input"
            );
            json!({"input_sha256":input.sha256,"submission_id":id,"status":"Accepted","status_code":0})
        }
        Operation::Upload {
            input,
            app_id,
            version,
            api_key_id,
            issuer_id,
            private_key_path_ref,
            jwt_ref,
        } => {
            let artifact = copy_input(input, state, deadline)?;
            ensure!(
                artifact.is_file(),
                "EPERM upload-requires-distribution-file"
            );
            let key_path = cohesix_authority::secret::resolve_reference(private_key_path_ref)?;
            ensure!(
                Path::new(&key_path).is_absolute() && fs::symlink_metadata(&key_path)?.is_file(),
                "EPERM app-store-key-path"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                ensure!(
                    fs::metadata(&key_path)?.permissions().mode() & 0o077 == 0,
                    "EPERM app-store-key-permissions"
                );
            }
            let bearer = cohesix_authority::secret::resolve_reference(jwt_ref)?;
            let client = reqwest::blocking::Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .build()?;
            let submitted: Value = serde_json::from_slice(&native(
                "/usr/bin/xcrun",
                &[
                    "altool",
                    "--upload-package",
                    text(&artifact)?,
                    "--api-key",
                    api_key_id,
                    "--api-issuer",
                    issuer_id,
                    "--p8-file-path",
                    &key_path,
                    "--output-format",
                    "json",
                ],
                deadline,
            )?)?;
            let id = token(&submitted, "delivery-uuid")?;
            let mut count = 0;
            loop {
                let upload = apple_get(
                    &client,
                    &bearer,
                    &format!("buildUploads/{id}?include=build,buildUploadFiles"),
                    deadline,
                )?;
                if upload["data"]["attributes"]["state"]["state"] == "COMPLETE" {
                    verify_upload(&upload, &id, app_id, version, &input.sha256)?;
                    ensure!(
                        artifact_digest(&artifact)? == input.sha256,
                        "EPERM changed-upload-input"
                    );
                    break json!({"input_sha256":input.sha256,"upload_id":id,"build_id":upload["data"]["relationships"]["build"]["data"]["id"],"processing_state":"VALID"});
                }
                ensure!(
                    matches!(
                        upload["data"]["attributes"]["state"]["state"].as_str(),
                        Some("PROCESSING" | "AWAITING_UPLOAD")
                    ),
                    "unconfirmed app-store-state"
                );
                count += 1;
                ensure!(
                    count < 30
                        && deadline.saturating_duration_since(Instant::now())
                            > Duration::from_secs(2),
                    "unconfirmed app-store-timeout"
                );
                std::thread::sleep(Duration::from_secs(2));
            }
        }
        Operation::Compliance => bail!("EPERM unexpected-compliance-branch"),
    };
    result["schema"] = "cohesix-macos-release-operation/v1".into();
    result["target_id"] = target.id.clone().into();
    result["action"] = target.operation.action().into();
    result["xcode_version"] = version.trim().into();
    result["device_attested"] = false.into();
    Ok(result)
}

fn copy_input(
    input: &cohesix_authority::mac_release::Artifact,
    state: &Path,
    deadline: Instant,
) -> Result<PathBuf> {
    ensure!(
        artifact_digest(&input.path)? == input.sha256,
        "EPERM artifact-digest"
    );
    let name = input
        .path
        .file_name()
        .ok_or_else(|| anyhow!("EPERM artifact-name"))?;
    let copy = state.join(name);
    native(
        "/usr/bin/ditto",
        &[text(&input.path)?, text(&copy)?],
        deadline,
    )?;
    ensure!(
        artifact_digest(&copy)? == input.sha256,
        "EPERM copied-artifact-digest"
    );
    Ok(copy)
}
fn apple_get(
    client: &reqwest::blocking::Client,
    bearer: &str,
    path: &str,
    deadline: Instant,
) -> Result<Value> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    ensure!(!remaining.is_zero(), "unconfirmed app-store-timeout");
    let response = client
        .get(format!("https://api.appstoreconnect.apple.com/v1/{path}"))
        .bearer_auth(bearer)
        .timeout(remaining.min(Duration::from_secs(15)))
        .send()
        .map_err(|_| anyhow!("unavailable app-store-transport"))?;
    ensure!(
        response.status().is_success(),
        "unavailable app-store-response"
    );
    let mut bytes = Vec::new();
    response.take(65537).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 65536, "ELIMIT app-store-response");
    Ok(serde_json::from_slice(&bytes)?)
}

/// Apple must bind the upload to the exact file checksum and processed build.
/// Missing attributes or API-version differences are unverified, never success.
pub fn verify_upload(value: &Value, id: &str, app: &str, version: &str, hash: &str) -> Result<()> {
    let data = &value["data"];
    ensure!(
        data["id"] == id
            && data["type"] == "buildUploads"
            && data["attributes"]["state"]["state"] == "COMPLETE"
            && data["attributes"]["cfBundleVersion"] == version,
        "unconfirmed app-store-upload"
    );
    let build = &data["relationships"]["build"]["data"];
    let files = data["relationships"]["buildUploadFiles"]["data"]
        .as_array()
        .ok_or_else(|| anyhow!("unconfirmed app-store-files"))?;
    let included = value["included"]
        .as_array()
        .ok_or_else(|| anyhow!("unconfirmed app-store-build"))?;
    ensure!(
        included
            .iter()
            .filter(|r| r["id"] == build["id"]
                && r["type"] == "builds"
                && r["attributes"]["processingState"] == "VALID"
                && r["attributes"]["version"] == version
                && r["relationships"]["app"]["data"]["id"] == app)
            .count()
            == 1,
        "unconfirmed app-store-build"
    );
    ensure!(
        included
            .iter()
            .filter(|r| r["type"] == "buildUploadFiles"
                && files
                    .iter()
                    .any(|f| f["id"] == r["id"] && f["type"] == "buildUploadFiles")
                && r["attributes"]["assetType"] == "ASSET"
                && r["attributes"]["sourceFileChecksums"]["file"]["algorithm"] == "SHA_256"
                && r["attributes"]["sourceFileChecksums"]["file"]["hash"] == hash)
            .count()
            == 1,
        "unconfirmed app-store-artifact"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_terminal_records_reject_partial_or_wrong_artifact_success() {
        let build =
            json!({"startTime":1.0,"endTime":2.0,"status":"succeeded","errorCount":0,"errors":[]});
        assert!(verify_xcode("mac_release.build", &build).is_ok());
        assert_eq!(
            project_xcode(&json!({"status":"succeeded","destination":{"deviceId":"private"}})),
            json!({"status":"succeeded"})
        );
        assert!(verify_xcode("mac_release.test", &build).is_err());
        let mut test = json!({"startTime":1.0,"finishTime":2.0,"result":"Passed","totalTestCount":1,"passedTests":1,"failedTests":0,"skippedTests":0,"expectedFailures":0});
        assert!(verify_xcode("mac_release.test", &test).is_ok());
        test["totalTestCount"] = 0.into();
        assert!(verify_xcode("mac_release.test", &test).is_err());
        let notary = json!({"jobId":"submission","status":"Accepted","statusCode":0,"sha256":"a".repeat(64)});
        assert!(verify_notary(&notary, "submission", &"a".repeat(64)).is_ok());
        assert!(verify_notary(&notary, "submission", &"b".repeat(64)).is_err());
        let mut upload = json!({"data":{"id":"delivery","type":"buildUploads","attributes":{"cfBundleVersion":"1","state":{"state":"COMPLETE"}},"relationships":{"build":{"data":{"type":"builds","id":"build"}},"buildUploadFiles":{"data":[{"type":"buildUploadFiles","id":"file"}]}}},"included":[{"id":"build","type":"builds","attributes":{"processingState":"VALID","version":"1"},"relationships":{"app":{"data":{"id":"123"}}}},{"id":"file","type":"buildUploadFiles","attributes":{"assetType":"ASSET","sourceFileChecksums":{"file":{"algorithm":"SHA_256","hash":"a".repeat(64)}}}}]});
        assert!(verify_upload(&upload, "delivery", "123", "1", &"a".repeat(64)).is_ok());
        assert!(verify_upload(&upload, "delivery", "999", "1", &"a".repeat(64)).is_err());
        upload["included"][0]["attributes"]["processingState"] = "PROCESSING".into();
        assert!(verify_upload(&upload, "delivery", "123", "1", &"a".repeat(64)).is_err());
    }
    #[test]
    fn input_identity_binds_content_and_refuses_external_symlinks() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("file"), b"first").unwrap();
        let a = artifact_digest(root.path()).unwrap();
        fs::write(root.path().join("file"), b"second").unwrap();
        assert_ne!(a, artifact_digest(root.path()).unwrap());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/passwd", root.path().join("escape")).unwrap();
            assert!(artifact_digest(root.path()).is_err());
        }
    }
}
