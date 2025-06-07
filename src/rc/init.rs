// CLASSIFICATION: COMMUNITY
// Filename: init.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-22

//! Minimal Plan 9 style init parser for Cohesix.

use std::fs;
use std::io::{self, BufRead};
use crate::plan9::namespace::{NamespaceLoader, Namespace};

pub fn run() -> io::Result<()> {
    let mut ns = NamespaceLoader::load()?;
    NamespaceLoader::apply(&mut ns)?;
    if let Ok(file) = fs::File::open("/boot/rc.local") {
        for line in io::BufReader::new(file).lines() {
            let l = line?;
            let tokens: Vec<&str> = l.split_whitespace().collect();
            match tokens.as_slice() {
                ["mount", srv, dst] => ns.add_op(crate::plan9::namespace::NsOp::Mount { srv: srv.to_string(), dst: dst.to_string() }),
                ["bind", src, dst] => ns.add_op(crate::plan9::namespace::NsOp::Bind { src: src.to_string(), dst: dst.to_string(), flags: crate::plan9::namespace::BindFlags::default() }),
                ["srv", path] => ns.add_op(crate::plan9::namespace::NsOp::Srv { path: path.to_string() }),
                ["run", cmd] => println!("[rc] run {cmd}"),
                _ => {}
            }
        }
    } else {
        println!("Welcome to Cohesix rc");
        println!("/dev/console: _");
    }
    ns.persist("boot")?;
    Ok(())
}
