// CLASSIFICATION: COMMUNITY
// Filename: init.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-16

//! Minimal Plan 9 style init parser for Cohesix.

use std::fs;
use std::io::{self, BufRead, Write};
use std::time::Instant;
use crate::plan9::namespace::NamespaceLoader;

pub fn run() -> io::Result<()> {
    let start = Instant::now();
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
        const BANNER: &str = r"  _____            _     _
 / ____|          | |   (_)
| |     ___   ___ | |__  _ _ __  ___
| |    / _ \ / _ \| '_ \| | '_ \/ __|
| |___| (_) | (_) | |_) | | | | \__ \
 \_____|___/ \___/|_.__/|_|_| |_|___/";

        println!("{}", BANNER);
        let bee = if std::env::var("LANG").unwrap_or_default().contains("UTF-8") {
            "🐝"
        } else {
            "BEE"
        };
        println!("{}", bee);
    }
    ns.persist("boot")?;
    let elapsed = start.elapsed().as_millis();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/log/boot_time.log")
    {
        let _ = writeln!(f, "init {}ms", elapsed);
    }
    Ok(())
}
