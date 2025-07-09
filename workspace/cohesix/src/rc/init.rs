// CLASSIFICATION: COMMUNITY
// Filename: init.rs v0.5
// Author: Lukas Bower
// Date Modified: 2026-12-30
// Formerly limited to non-UEFI builds; now always compiled for UEFI.

//! Minimal Plan 9 style init parser for Cohesix.
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use crate::plan9::namespace::NamespaceLoader;
use std::fs;
use std::io::{self, BufRead, Write};
use std::time::Instant;
use log::warn;


use serde::{Deserialize, Serialize};
use toml;


#[derive(Debug, Deserialize, Serialize)]
struct InitConf {
    init_mode: Option<String>,
    start_services: Option<Vec<String>>,
}


fn load_init_conf() -> Option<InitConf> {
    std::fs::read_to_string("/etc/init.conf")
        .ok()
        .and_then(|data| toml::from_str(&data).ok())
}

pub fn run() -> io::Result<()> {
    const BANNER: &str = r"  
   ____      _               _      
  / ___|___ | |__   ___  ___(_)_  __
 | |   / _ \| '_ \ / _ \/ __| \ \/ /
 | |__| (_) | | | |  __/\__ \ |>  < 
  \____\___/|_| |_|\___||___/_/_/\_\                              
                                    ";

    println!("{}", BANNER);
    println!("C O H E S I X   R U N T I M E");

    
    match load_init_conf() {
        Some(cfg) => {
            if let Some(mode) = cfg.init_mode {
                println!("[rc] init_mode: {}", mode);
            }
            if let Some(services) = cfg.start_services {
                println!("[rc] would start services: {}", services.join(", "));
            }
        }
        None => warn!("[rc] missing or invalid /etc/init.conf; using defaults"),
    }

    let start = Instant::now();
    let mut ns = NamespaceLoader::load()?;
    if std::path::Path::new("/miniroot").exists() {
        ns.add_op(crate::plan9::namespace::NsOp::Bind {
            src: "/miniroot".to_string(),
            dst: "/".to_string(),
            flags: crate::plan9::namespace::BindFlags { before: true, after: false, create: false },
        });
    }
    NamespaceLoader::apply(&mut ns)?;
    if let Ok(file) = fs::File::open("/boot/rc.local") {
        for line in io::BufReader::new(file).lines() {
            let l = line?;
            let tokens: Vec<&str> = l.split_whitespace().collect();
            match tokens.as_slice() {
                ["mount", srv, dst] => ns.add_op(crate::plan9::namespace::NsOp::Mount {
                    srv: srv.to_string(),
                    dst: dst.to_string(),
                }),
                ["bind", src, dst] => ns.add_op(crate::plan9::namespace::NsOp::Bind {
                    src: src.to_string(),
                    dst: dst.to_string(),
                    flags: crate::plan9::namespace::BindFlags::default(),
                }),
                ["srv", path] => ns.add_op(crate::plan9::namespace::NsOp::Srv {
                    path: path.to_string(),
                }),
                ["run", cmd] => println!("[rc] run {cmd}"),
                _ => {}
            }
        }
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

