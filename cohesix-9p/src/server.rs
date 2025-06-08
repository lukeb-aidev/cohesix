// CLASSIFICATION: COMMUNITY
// Filename: server.rs v0.1
// Date Modified: 2025-07-13
// Author: Lukas Bower

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use log::{error, info, warn};
use ninep::{
    fs::{FileMeta, IoUnit, Mode, Perm, QID_ROOT, Stat},
    sync::{
        client::TcpClient,
        server::{ClientId, ReadOutcome, Serve9p, Server},
    },
};

/// Permission categories for the Cohesix file tree.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Access {
    Read,
    Write,
}

fn check_perm(path: &str, access: Access) -> Result<()> {
    if path.starts_with("/proc") || path.starts_with("/history") {
        if access == Access::Write {
            warn!("deny write to restricted path: {}", path);
            return Err(anyhow!("permission denied"));
        }
    }
    if path.starts_with("/mnt") && access == Access::Write {
        warn!("deny write to /mnt: {}", path);
        return Err(anyhow!("permission denied"));
    }
    Ok(())
}

/// Simple in-memory file node.
#[derive(Default)]
struct Node {
    data: Vec<u8>,
    is_dir: bool,
}

#[derive(Default)]
pub struct CohesixFs {
    root: PathBuf,
    nodes: Mutex<HashMap<String, Node>>, // path -> node
    qmap: Mutex<HashMap<u64, String>>,   // qid -> path
    next_qid: AtomicU64,
    remotes: Mutex<HashMap<String, TcpClient>>, // mountpoint -> client
}

impl CohesixFs {
    pub fn new(root: PathBuf) -> Self {
        let mut qmap = HashMap::new();
        qmap.insert(QID_ROOT, String::from("/"));
        Self {
            root,
            nodes: Mutex::new(HashMap::new()),
            qmap: Mutex::new(qmap),
            next_qid: AtomicU64::new(QID_ROOT + 1),
            remotes: Mutex::new(HashMap::new()),
        }
    }

    fn path_for(&self, qid: u64) -> String {
        self.qmap
            .lock()
            .unwrap()
            .get(&qid)
            .cloned()
            .unwrap_or_else(|| "/".into())
    }

    fn alloc_qid(&self, path: &str) -> u64 {
        let qid = self.next_qid.fetch_add(1, Ordering::SeqCst);
        self.qmap.lock().unwrap().insert(qid, path.to_string());
        qid
    }

    /// Mount a remote 9P server under the provided mountpoint.
    pub fn mount_remote(&self, mountpoint: &str, addr: &str) -> Result<()> {
        let client = TcpClient::new_tcp("cohesix", addr, "/")?;
        self.remotes
            .lock()
            .unwrap()
            .insert(mountpoint.to_string(), client);
        Ok(())
    }

    fn remote_client(&self, path: &str) -> Option<(String, TcpClient)> {
        for (mnt, client) in self.remotes.lock().unwrap().iter() {
            if path.starts_with(mnt) {
                let sub = path[mnt.len()..].trim_start_matches('/').to_string();
                return Some((sub, client.clone()));
            }
        }
        None
    }
}

impl Serve9p for CohesixFs {
    fn walk(&self, _cid: ClientId, parent_qid: u64, child: &str, _uname: &str) -> Result<FileMeta> {
        let base = self.path_for(parent_qid);
        let new_path = if base == "/" {
            format!("/{}", child)
        } else {
            format!("{}/{}", base, child)
        };
        if let Some((sub, mut cli)) = self.remote_client(&new_path) {
            cli.walk(sub)?;
        }
        let is_dir = self
            .nodes
            .lock()
            .unwrap()
            .get(&new_path)
            .map(|n| n.is_dir)
            .unwrap_or(true);
        let qid = self.alloc_qid(&new_path);
        Ok(if is_dir {
            FileMeta::dir(child, qid)
        } else {
            FileMeta::file(child, qid)
        })
    }

    fn open(&self, _cid: ClientId, qid: u64, mode: Mode, _uname: &str) -> Result<IoUnit> {
        let path = self.path_for(qid);
        check_perm(
            &path,
            if mode == Mode::READ {
                Access::Read
            } else {
                Access::Write
            },
        )?;
        if let Some((sub, mut cli)) = self.remote_client(&path) {
            let _ = cli.open(sub, mode)?;
            return Ok(8192);
        }
        let mut nodes = self.nodes.lock().unwrap();
        nodes.entry(path).or_default();
        Ok(8192)
    }

    fn clunk(&self, _cid: ClientId, qid: u64) {
        self.qmap.lock().unwrap().remove(&qid);
    }

    fn create(
        &self,
        _cid: ClientId,
        parent: u64,
        name: &str,
        _perm: Perm,
        _mode: Mode,
        _uname: &str,
    ) -> Result<(FileMeta, IoUnit)> {
        let base = self.path_for(parent);
        let path = if base == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", base, name)
        };
        check_perm(&path, Access::Write)?;
        let qid = self.alloc_qid(&path);
        self.nodes.lock().unwrap().insert(
            path.clone(),
            Node {
                data: Vec::new(),
                is_dir: false,
            },
        );
        Ok((FileMeta::file(name, qid), 8192))
    }

    fn read(
        &self,
        _cid: ClientId,
        qid: u64,
        offset: usize,
        count: usize,
        _uname: &str,
    ) -> Result<ReadOutcome> {
        let path = self.path_for(qid);
        if let Some((sub, mut cli)) = self.remote_client(&path) {
            let data = cli.read(sub)?;
            let slice = data.into_iter().skip(offset).take(count).collect();
            return Ok(ReadOutcome::Immediate(slice));
        }
        let nodes = self.nodes.lock().unwrap();
        if let Some(node) = nodes.get(&path) {
            let slice = node.data.iter().skip(offset).take(count).copied().collect();
            return Ok(ReadOutcome::Immediate(slice));
        }
        Err(anyhow!("not found"))
    }

    fn read_dir(&self, _cid: ClientId, qid: u64, _uname: &str) -> Result<Vec<Stat>> {
        let path = self.path_for(qid);
        let mut stats = Vec::new();
        for (p, node) in self.nodes.lock().unwrap().iter() {
            if let Some(rel) = p.strip_prefix(&path) {
                if rel.starts_with('/') && rel.split('/').count() == 2 {
                    let name = rel.trim_start_matches('/');
                    let qid = self.alloc_qid(p);
                    let fm = if node.is_dir {
                        FileMeta::dir(name, qid)
                    } else {
                        FileMeta::file(name, qid)
                    };
                    stats.push(Stat {
                        fm,
                        perms: Perm::OWNER_READ,
                        n_bytes: node.data.len() as u64,
                        last_accesses: std::time::SystemTime::now(),
                        last_modified: std::time::SystemTime::now(),
                        owner: String::new(),
                        group: String::new(),
                        last_modified_by: String::new(),
                    });
                }
            }
        }
        Ok(stats)
    }

    fn write(
        &self,
        _cid: ClientId,
        qid: u64,
        offset: usize,
        data: Vec<u8>,
        _uname: &str,
    ) -> Result<usize> {
        let path = self.path_for(qid);
        check_perm(&path, Access::Write)?;
        if let Some((sub, mut cli)) = self.remote_client(&path) {
            let written = cli.write(sub, offset as u64, &data)?;
            return Ok(written);
        }
        let mut nodes = self.nodes.lock().unwrap();
        let node = nodes.entry(path).or_default();
        if node.data.len() < offset + data.len() {
            node.data.resize(offset + data.len(), 0);
        }
        node.data[offset..offset + data.len()].copy_from_slice(&data);
        Ok(data.len())
    }

    fn remove(&self, _cid: ClientId, qid: u64, _uname: &str) -> Result<()> {
        let path = self.path_for(qid);
        check_perm(&path, Access::Write)?;
        self.nodes.lock().unwrap().remove(&path);
        Ok(())
    }

    fn stat(&self, _cid: ClientId, qid: u64, _uname: &str) -> Result<Stat> {
        let path = self.path_for(qid);
        if let Some((sub, mut cli)) = self.remote_client(&path) {
            return cli.stat(sub);
        }
        let nodes = self.nodes.lock().unwrap();
        let node = nodes.get(&path).ok_or_else(|| anyhow!("not found"))?;
        let fm = if node.is_dir {
            FileMeta::dir("", qid)
        } else {
            FileMeta::file("", qid)
        };
        Ok(Stat {
            fm,
            perms: Perm::OWNER_READ,
            n_bytes: node.data.len() as u64,
            last_accesses: std::time::SystemTime::now(),
            last_modified: std::time::SystemTime::now(),
            owner: String::new(),
            group: String::new(),
            last_modified_by: String::new(),
        })
    }

    fn write_stat(&self, _cid: ClientId, _qid: u64, _stat: Stat, _uname: &str) -> Result<()> {
        Err("write_stat not supported".into())
    }
}

pub struct FsServer {
    _handle: std::thread::JoinHandle<()>,
}

impl FsServer {
    pub fn start(cfg: super::FsConfig) -> Result<Self> {
        let fs = CohesixFs::new(cfg.root);
        let server = Server::new(fs);
        info!("Starting 9P server on {}", cfg.port);
        let handle = server.serve_tcp(cfg.port);
        Ok(FsServer { _handle: handle })
    }
}
