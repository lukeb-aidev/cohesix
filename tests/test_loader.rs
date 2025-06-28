// CLASSIFICATION: COMMUNITY
// Filename: test_loader.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-09-26

use cohesix::runtime::loader::load_and_run;
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

#[test]
fn load_valid_binary() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ok.out");
    let mut f = File::create(&path).unwrap();
    f.write_all(b"COHB").unwrap();
    f.write_all(&[1]).unwrap();
    f.write_all(b"#!/bin/sh\nexit 0\n").unwrap();
    assert!(load_and_run(path.to_str().unwrap()).is_ok());
}

#[test]
fn reject_invalid_magic() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("bad.out");
    let mut f = File::create(&path).unwrap();
    f.write_all(b"BADC").unwrap();
    f.write_all(&[1, 0x01]).unwrap();
    let err = load_and_run(path.to_str().unwrap()).unwrap_err();
    assert!(err.to_string().contains("invalid"));
}
