// CLASSIFICATION: COMMUNITY
// Filename: syscalls.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-22

//! Minimal Plan 9 style syscall wrappers for Cohesix.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};

use super::namespace::Namespace;

pub fn open(ns: &Namespace, path: &str) -> io::Result<File> {
    if let Some(real) = ns.resolve(path) {
        OpenOptions::new().read(true).write(true).open(real)
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "path not in namespace"))
    }
}

pub fn create(ns: &Namespace, path: &str) -> io::Result<File> {
    if let Some(real) = ns.resolve(path) {
        OpenOptions::new().create(true).write(true).open(real)
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "path not in namespace"))
    }
}

pub fn read(fd: &mut File, buf: &mut Vec<u8>) -> io::Result<usize> {
    fd.read_to_end(buf)
}

pub fn write(fd: &mut File, data: &[u8]) -> io::Result<usize> {
    fd.write(data)
}

pub fn remove(ns: &Namespace, path: &str) -> io::Result<()> {
    if let Some(real) = ns.resolve(path) {
        fs::remove_file(real)
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "path not in namespace"))
    }
}

pub fn fstat(fd: &File) -> io::Result<fs::Metadata> {
    fd.metadata()
}

pub fn clone(fd: &File) -> io::Result<File> {
    fd.try_clone()
}

pub fn walk(ns: &Namespace, dir: &str) -> io::Result<Vec<String>> {
    if let Some(real) = ns.resolve(dir) {
        let mut v = Vec::new();
        for entry in fs::read_dir(real)? {
            let e = entry?;
            v.push(e.file_name().to_string_lossy().into_owned());
        }
        Ok(v)
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "path not in namespace"))
    }
}
