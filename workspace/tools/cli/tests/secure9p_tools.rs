// CLASSIFICATION: COMMUNITY
// Filename: secure9p_tools.rs v0.1
// Author: Lukas Bower
// Date Modified: 2029-09-26

use assert_cmd::Command;
use cohesix_secure9p::manifest::ManifestSignature;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use std::fs;
use std::path::Path;
use tempfile::tempdir;
use x509_parser::extensions::GeneralName;
use x509_parser::parse_x509_certificate;
use x509_parser::pem::parse_x509_pem;

fn write_ca(dir: &Path) -> (String, String) {
    let mut params = CertificateParams::new(vec!["localhost".into()]).expect("ca params");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Secure9P Test CA");
    params.distinguished_name = dn;
    let key = KeyPair::generate().expect("ca key");
    let cert = params.self_signed(&key).expect("ca cert");
    let cert_path = dir.join("ca.pem");
    let key_path = dir.join("ca.key");
    fs::write(&cert_path, cert.pem()).expect("write ca cert");
    fs::write(&key_path, key.serialize_pem()).expect("write ca key");
    (
        cert_path.to_string_lossy().into_owned(),
        key_path.to_string_lossy().into_owned(),
    )
}

#[test]
fn secure9p_sign_writes_digest() {
    let tmp = tempdir().expect("tempdir");
    let manifest_path = tmp.path().join("secure9p.toml");
    fs::write(
        &manifest_path,
        "port = 1\ncert = 'a'\nkey = 'b'\nrequire_client_auth = false\n",
    )
    .expect("write manifest");
    let signature_path = tmp.path().join("secure9p.sig");

    let mut cmd = Command::cargo_bin("secure9p_sign").expect("bin");
    cmd.arg("--manifest")
        .arg(&manifest_path)
        .arg("--output")
        .arg(&signature_path)
        .assert()
        .success();

    let contents = fs::read_to_string(&signature_path).expect("signature");
    assert!(contents.contains("// CLASSIFICATION: COMMUNITY"));
    let expected = ManifestSignature::compute(&fs::read(&manifest_path).unwrap());
    assert!(contents.contains(&expected.digest));
}

#[test]
fn secure9p_onboard_generates_spiffe_certificate() {
    let tmp = tempdir().expect("tempdir");
    let (ca_cert, ca_key) = write_ca(tmp.path());
    let client_cert = tmp.path().join("client.pem");
    let client_key = tmp.path().join("client.key");
    let spiffe = "spiffe://cohesix/test/client";

    let mut cmd = Command::cargo_bin("secure9p_onboard").expect("bin");
    cmd.arg("--ca-cert")
        .arg(&ca_cert)
        .arg("--ca-key")
        .arg(&ca_key)
        .arg("--spiffe-id")
        .arg(spiffe)
        .arg("--out-cert")
        .arg(&client_cert)
        .arg("--out-key")
        .arg(&client_key)
        .arg("--valid-days")
        .arg("30")
        .assert()
        .success();

    let cert_bytes = fs::read(&client_cert).expect("read cert");
    let (_, pem) = parse_x509_pem(&cert_bytes).expect("parse pem");
    let (_, cert) = parse_x509_certificate(&pem.contents).expect("parse cert");
    let san = cert
        .subject_alternative_name()
        .expect("san")
        .expect("san present");
    let mut has_uri = false;
    for name in san.value.general_names.iter() {
        if let GeneralName::URI(uri) = name {
            if *uri == spiffe {
                has_uri = true;
                break;
            }
        }
    }
    assert!(has_uri, "SPIFFE URI missing from SAN");
    assert!(client_key.exists());
}
