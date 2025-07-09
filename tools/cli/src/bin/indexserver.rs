// CLASSIFICATION: COMMUNITY
// Filename: indexserver.rs v0.2
// Author: Lukas Bower
// Date Modified: 2027-11-05

use clap::Parser;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(about = "Plan9 file index server")]
struct Args {
    #[arg(long, default_value = "/")]
    root: PathBuf,
    #[arg(long, default_value = "1s")]
    interval: humantime::Duration,
}

type Index = Arc<RwLock<HashMap<String, Vec<String>>>>;

fn build_index(index: &Index, root: &Path) {
    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path().display().to_string();
        let mut idx = index.write().unwrap();
        idx.entry(name).or_default().push(path);
    }
}

fn search(index: &Index, q: &str) -> Vec<String> {
    let idx = index.read().unwrap();
    idx.get(q).cloned().unwrap_or_default()
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let index: Index = Arc::new(RwLock::new(HashMap::new()));
    build_index(&index, &args.root);
    std::fs::create_dir_all("/srv/index")?;
    std::fs::write("/srv/index/query", b"")?;
    std::fs::write("/srv/index/results", b"")?;
    let mut last = String::new();
    loop {
        let query = std::fs::read_to_string("/srv/index/query")?.trim().to_string();
        if !query.is_empty() && query != last {
            let res = search(&index, &query).join("\n");
            std::fs::write("/srv/index/results", res)?;
            last = query;
        }
        std::thread::sleep(Duration::from(args.interval));
    }
}
