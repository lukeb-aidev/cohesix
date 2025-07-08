// CLASSIFICATION: COMMUNITY
// Filename: devwatcher.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-08

use clap::Parser;
use notify::{RecommendedWatcher, RecursiveMode, Watcher, Event};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::Duration;

#[derive(Parser)]
#[command(about = "Plan9 file change watcher")]
struct Args {
    #[arg(long, default_value = "1s")]
    interval: humantime::Duration,
}

fn main() -> notify::Result<()> {
    let args = Args::parse();
    fs::create_dir_all("/dev/watch")?;
    fs::write("/dev/watch/ctl", b"")?;
    OpenOptions::new().create(true).write(true).truncate(true).open("/dev/watch/events")?;

    let (tx, rx) = channel::<Result<Event, notify::Error>>();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from(args.interval))?;
    let mut watched = HashSet::new();

    loop {
        if let Ok(data) = fs::read_to_string("/dev/watch/ctl") {
            for line in data.lines() {
                let p = line.trim();
                if p.is_empty() { continue; }
                if watched.insert(p.to_string()) {
                    watcher.watch(PathBuf::from(p), RecursiveMode::NonRecursive)?;
                }
            }
        }
        while let Ok(res) = rx.try_recv() {
            if let Ok(event) = res {
                let mut f = OpenOptions::new().append(true).open("/dev/watch/events")?;
                writeln!(f, "{} {:?}", event.paths.get(0).map(|p| p.display()).unwrap_or_default(), event.kind)?;
            }
        }
        std::thread::sleep(Duration::from(args.interval));
    }
}
