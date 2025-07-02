// CLASSIFICATION: COMMUNITY
// Filename: server.rs v0.3
// Date Modified: 2025-07-24
// Author: Lukas Bower

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::fs::ValidatorHook;
use crate::ninep_adapter::{read_slice, verify_open};
use crate::policy::{Access, SandboxPolicy};
use cohesix::{CohError, coh_error};
use log::{info, warn};
use ninep::{
    client::TcpClient,
    fs::{FileMeta, IoUnit, Mode, Perm, QID_ROOT, Stat},
    server::{ClientId, ReadOutcome, Serve9p, Server},
};

fn check_perm(path: &str, access: Access) -> Result<(), CohError> {
    crate::enforce_capability(path)?;
    if (path.starts_with("/proc") || path.starts_with("/history")) && access == Access::Write {
        warn!("deny write to restricted path: {}", path);
        return Err(coh_error!("permission denied"));
    }
    if path.starts_with("/mnt") && access == Access::Write {
        warn!("deny write to /mnt: {}", path);
        return Err(coh_error!("permission denied"));
    }
    Ok(())
}

/// Simple in-memory file node.
#[derive(Default)]
struct Node {
    data: Vec<u8>,
    is_dir: bool,
}

/// In-memory 9P filesystem backing the Cohesix server.
#[derive(Default)]
pub struct CohesixFs {
    /// Base path for future persistent storage.
    _root: PathBuf,
    nodes: Mutex<HashMap<String, Node>>, // path -> node
    qmap: Mutex<HashMap<u64, String>>,   // qid -> path
    next_qid: AtomicU64,
    remotes: Mutex<HashMap<String, TcpClient>>, // mountpoint -> client
    policies: Mutex<HashMap<String, SandboxPolicy>>, // uname -> policy
    validator_hook: Option<Arc<ValidatorHook>>,
}

impl CohesixFs {
    /// Create a new in-memory filesystem rooted at the given path.
    pub fn new(root: PathBuf) -> Self {
        let mut qmap = HashMap::new();
        qmap.insert(QID_ROOT, String::from("/"));
        let mut nodes = HashMap::new();
        nodes.insert(
            "/".into(),
            Node {
                data: Vec::new(),
                is_dir: true,
            },
        );
        Self {
            _root: root,
            nodes: Mutex::new(nodes),
            qmap: Mutex::new(qmap),
            next_qid: AtomicU64::new(QID_ROOT + 1),
            remotes: Mutex::new(HashMap::new()),
            policies: Mutex::new(HashMap::new()),
            validator_hook: None,
        }
    }

    fn path_for(&self, qid: u64) -> String {
        match self.qmap.lock() {
            Ok(map) => map.get(&qid).cloned().unwrap_or_else(|| "/".into()),
            Err(poisoned) => poisoned
                .into_inner()
                .get(&qid)
                .cloned()
                .unwrap_or_else(|| "/".into()),
        }
    }

    fn alloc_qid(&self, path: &str) -> u64 {
        let qid = self.next_qid.fetch_add(1, Ordering::SeqCst);
        match self.qmap.lock() {
            Ok(mut map) => {
                map.insert(qid, path.to_string());
            }
            Err(poisoned) => {
                poisoned.into_inner().insert(qid, path.to_string());
            }
        }
        qid
    }

    /// Mount a remote 9P server under the provided mountpoint.
    #[allow(dead_code)]
    // This function will be made public once remote mounts are supported
    // FIXME: expose once remote mount functionality is used by runtime
    pub fn _mount_remote(&self, mountpoint: &str, addr: &str) -> Result<(), CohError> {
        let client = TcpClient::new_tcp("cohesix".to_string(), addr, "/")?;
        match self.remotes.lock() {
            Ok(mut map) => {
                map.insert(mountpoint.to_string(), client);
            }
            Err(poisoned) => {
                poisoned.into_inner().insert(mountpoint.to_string(), client);
            }
        }
        Ok(())
    }

    fn remote_client(&self, path: &str) -> Option<(String, TcpClient)> {
        let map = match self.remotes.lock() {
            Ok(map) => map,
            Err(poisoned) => poisoned.into_inner(),
        };
        for (mnt, client) in map.iter() {
            if path.starts_with(mnt) {
                let sub = path[mnt.len()..].trim_start_matches('/').to_string();
                return Some((sub, client.clone()));
            }
        }
        None
    }

    /// Assign a policy to a user/session name.
    pub fn set_policy(&self, uname: String, policy: SandboxPolicy) {
        match self.policies.lock() {
            Ok(mut map) => {
                map.insert(uname, policy);
            }
            Err(poisoned) => {
                poisoned.into_inner().insert(uname, policy);
            }
        }
    }

    /// Register a validator hook for policy violations.
    pub fn set_validator_hook(&mut self, hook: Arc<ValidatorHook>) {
        self.validator_hook = Some(hook);
    }

    fn policy_for(&self, uname: &str) -> Option<SandboxPolicy> {
        let map = match self.policies.lock() {
            Ok(m) => m,
            Err(poisoned) => poisoned.into_inner(),
        };
        map.get(uname).cloned()
    }

    fn check_access(&self, path: &str, access: Access, uname: &str) -> Result<(), CohError> {
        check_perm(path, access)?;
        if self
            .policy_for(uname)
            .is_some_and(|pol| !pol.allows(path, access))
        {
            if let Some(h) = &self.validator_hook {
                h(
                    "9p_policy",
                    path.to_string(),
                    uname.to_string(),
                    current_ts(),
                );
            }
            warn!("deny {:?} to {} by {}", access, path, uname);
            return Err(coh_error!("permission denied"));
        }
        Ok(())
    }
}

impl Serve9p for CohesixFs {
    fn walk(
        &mut self,
        _cid: ClientId,
        parent_qid: u64,
        child: &str,
        _uname: &str,
    ) -> ninep::Result<FileMeta> {
        let base = self.path_for(parent_qid);
        let new_path = if base == "/" {
            format!("/{}", child)
        } else {
            format!("{}/{}", base, child)
        };
        if let Some((sub, mut cli)) = self.remote_client(&new_path) {
            let _ = cli.walk(sub).map_err(|e| e.to_string())?;
        }
        let is_dir = match self.nodes.lock() {
            Ok(map) => map.get(&new_path).map(|n| n.is_dir).unwrap_or(true),
            Err(poisoned) => poisoned
                .into_inner()
                .get(&new_path)
                .map(|n| n.is_dir)
                .unwrap_or(true),
        };
        let qid = self.alloc_qid(&new_path);
        Ok(if is_dir {
            FileMeta::dir(child, qid)
        } else {
            FileMeta::file(child, qid)
        })
    }

    fn open(
        &mut self,
        _cid: ClientId,
        qid: u64,
        mode: Mode,
        _uname: &str,
    ) -> ninep::Result<IoUnit> {
        let path = self.path_for(qid);
        // Opening a file always requires read permissions for now.
        self.check_access(&path, Access::Read, _uname)
            .map_err(|e| e.to_string())?;
        if let Some((sub, mut cli)) = self.remote_client(&path) {
            verify_open(&mut cli, &sub, mode).map_err(|e| e.to_string())?;
            return Ok(8192);
        }
        match self.nodes.lock() {
            Ok(mut nodes) => {
                nodes.entry(path).or_default();
            }
            Err(poisoned) => {
                poisoned.into_inner().entry(path).or_default();
            }
        }
        Ok(8192)
    }

    fn clunk(&mut self, _cid: ClientId, qid: u64) {
        match self.qmap.lock() {
            Ok(mut map) => {
                map.remove(&qid);
            }
            Err(poisoned) => {
                poisoned.into_inner().remove(&qid);
            }
        }
    }

    fn create(
        &mut self,
        _cid: ClientId,
        parent: u64,
        name: &str,
        _perm: Perm,
        _mode: Mode,
        _uname: &str,
    ) -> ninep::Result<(FileMeta, IoUnit)> {
        let base = self.path_for(parent);
        let path = if base == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", base, name)
        };
        self.check_access(&path, Access::Write, _uname)
            .map_err(|e| e.to_string())?;
        let qid = self.alloc_qid(&path);
        match self.nodes.lock() {
            Ok(mut nodes) => {
                nodes.insert(
                    path.clone(),
                    Node {
                        data: Vec::new(),
                        is_dir: false,
                    },
                );
            }
            Err(poisoned) => {
                poisoned.into_inner().insert(
                    path.clone(),
                    Node {
                        data: Vec::new(),
                        is_dir: false,
                    },
                );
            }
        }
        Ok((FileMeta::file(name, qid), 8192))
    }

    fn read(
        &mut self,
        _cid: ClientId,
        qid: u64,
        offset: usize,
        count: usize,
        _uname: &str,
    ) -> ninep::Result<ReadOutcome> {
        let path = self.path_for(qid);
        self.check_access(&path, Access::Read, _uname)
            .map_err(|e| e.to_string())?;
        if let Some((sub, mut cli)) = self.remote_client(&path) {
            let slice = read_slice(&mut cli, &sub, offset, count).map_err(|e| e.to_string())?;
            return Ok(ReadOutcome::Immediate(slice));
        }
        let nodes = match self.nodes.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(node) = nodes.get(&path) {
            let slice = node.data.iter().skip(offset).take(count).copied().collect();
            return Ok(ReadOutcome::Immediate(slice));
        }
        Err("not found".to_string())
    }

    fn read_dir(&mut self, _cid: ClientId, qid: u64, _uname: &str) -> ninep::Result<Vec<Stat>> {
        let path = self.path_for(qid);
        let mut stats = Vec::new();
        let map = match self.nodes.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        for (p, node) in map.iter() {
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
        &mut self,
        _cid: ClientId,
        qid: u64,
        offset: usize,
        data: Vec<u8>,
        _uname: &str,
    ) -> ninep::Result<usize> {
        let path = self.path_for(qid);
        self.check_access(&path, Access::Write, _uname)
            .map_err(|e| e.to_string())?;
        if let Some((sub, mut cli)) = self.remote_client(&path) {
            let written = cli
                .write(sub, offset as u64, &data)
                .map_err(|e| e.to_string())?;
            return Ok(written);
        }
        let mut nodes = match self.nodes.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let node = nodes.entry(path).or_default();
        if node.data.len() < offset + data.len() {
            node.data.resize(offset + data.len(), 0);
        }
        node.data[offset..offset + data.len()].copy_from_slice(&data);
        Ok(data.len())
    }

    fn remove(&mut self, _cid: ClientId, qid: u64, _uname: &str) -> ninep::Result<()> {
        let path = self.path_for(qid);
        self.check_access(&path, Access::Write, _uname)
            .map_err(|e| e.to_string())?;
        match self.nodes.lock() {
            Ok(mut nodes) => {
                nodes.remove(&path);
            }
            Err(poisoned) => {
                poisoned.into_inner().remove(&path);
            }
        }
        Ok(())
    }

    fn stat(&mut self, _cid: ClientId, qid: u64, _uname: &str) -> ninep::Result<Stat> {
        let path = self.path_for(qid);
        if let Some((sub, mut cli)) = self.remote_client(&path) {
            return cli.stat(sub).map_err(|e| e.to_string());
        }
        let nodes = match self.nodes.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let node = nodes.get(&path).ok_or_else(|| "not found".to_string())?;
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

    fn write_stat(
        &mut self,
        _cid: ClientId,
        _qid: u64,
        _stat: Stat,
        _uname: &str,
    ) -> ninep::Result<()> {
        Err("write_stat not supported".to_string())
    }
}

/// Top-level 9P server wrapper.
pub struct FsServer {
    cfg: super::FsConfig,
    policies: Vec<(String, SandboxPolicy)>,
    validator_hook: Option<Arc<ValidatorHook>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl FsServer {
    /// Create a server with the provided configuration.
    pub fn new(cfg: super::FsConfig) -> Self {
        Self {
            cfg,
            policies: Vec::new(),
            validator_hook: None,
            handle: None,
        }
    }

    /// Configure a sandbox policy for a session/username before `start`.
    pub fn set_policy(&mut self, uname: String, policy: SandboxPolicy) {
        self.policies.push((uname, policy));
    }

    /// Register a validator hook applied to the inner filesystem.
    pub fn set_validator_hook(&mut self, hook: Arc<ValidatorHook>) {
        self.validator_hook = Some(hook);
    }

    /// Start serving over TCP.
    pub fn start(&mut self) -> Result<(), CohError> {
        let mut fs = CohesixFs::new(self.cfg.root.clone());
        for (u, p) in &self.policies {
            fs.set_policy(u.clone(), p.clone());
        }
        if let Some(h) = &self.validator_hook {
            fs.set_validator_hook(Arc::clone(h));
        }
        let server = Server::new(fs);
        info!("Starting 9P server on {}", self.cfg.port);
        let handle = server.serve_tcp(self.cfg.port);
        self.handle = Some(handle);
        Ok(())
    }

    /// Start serving over a Unix domain socket path.
    pub fn start_socket(&mut self, socket: impl Into<String>) -> Result<(), CohError> {
        let path = socket.into();
        let mut fs = CohesixFs::new(self.cfg.root.clone());
        for (u, p) in &self.policies {
            fs.set_policy(u.clone(), p.clone());
        }
        if let Some(h) = &self.validator_hook {
            fs.set_validator_hook(Arc::clone(h));
        }
        let server = Server::new(fs);
        info!("Starting 9P server on {}", &path);
        let handle = server.serve_socket(path);
        self.handle = Some(handle);
        Ok(())
    }

    /// Start serving over an arbitrary in-process stream.
    pub fn start_on_stream<U: ninep::Stream + Send + 'static>(
        &mut self,
        stream: U,
    ) -> Result<U, CohError> {
        let mut fs = CohesixFs::new(self.cfg.root.clone());
        for (u, p) in &self.policies {
            fs.set_policy(u.clone(), p.clone());
        }
        if let Some(h) = &self.validator_hook {
            fs.set_validator_hook(Arc::clone(h));
        }
        let server = Server::new(fs);
        info!("[Plan9] cohesix-9p serving /usr, /etc, /srv");
        let handle = server.serve_stream(stream.try_clone().map_err(|e| coh_error!(e))?);
        self.handle = Some(handle);
        Ok(stream)
    }
}

fn current_ts() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
