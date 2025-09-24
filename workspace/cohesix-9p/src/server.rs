// CLASSIFICATION: COMMUNITY
// Filename: server.rs v0.5
// Author: Lukas Bower
// Date Modified: 2028-12-31

use crate::{
    fs::{current_ts, ValidatorHook},
    policy::{Access, SandboxPolicy},
    CohError, FsConfig,
};
use alloc::sync::Arc;
use log::{error, info, warn};
use ninep::{
    fs::{FileMeta, IoUnit, Mode, Perm, Stat, QID_ROOT},
    protocol::{Format9p, RawStat},
    server::{ClientId, ReadOutcome, Serve9p, Server},
    Stream,
};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    net::{SocketAddr, TcpListener},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        RwLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

#[derive(Clone, Debug)]
struct NamespaceConfig {
    root: PathBuf,
    read_only: bool,
}

#[derive(Default, Clone)]
struct PolicyStore {
    default: Option<SandboxPolicy>,
    per_agent: HashMap<String, SandboxPolicy>,
}

impl PolicyStore {
    fn for_agent(&self, agent: &str) -> Option<SandboxPolicy> {
        if let Some(pol) = self.per_agent.get(agent) {
            return Some(pol.clone());
        }
        self.default.clone()
    }
}

struct InnerBackend {
    default_root: PathBuf,
    readonly: bool,
    policy: RwLock<PolicyStore>,
    namespaces: RwLock<HashMap<String, NamespaceConfig>>,
    validator: RwLock<Option<Arc<ValidatorHook>>>,
    path_qids: RwLock<HashMap<String, u64>>,
    qid_paths: RwLock<HashMap<u64, String>>,
    next_qid: AtomicU64,
}

impl InnerBackend {
    fn new(root: PathBuf, readonly: bool) -> io::Result<Self> {
        let canonical = fs::canonicalize(&root).or_else(|_| {
            fs::create_dir_all(&root)?;
            fs::canonicalize(&root)
        })?;
        let mut path_qids = HashMap::new();
        path_qids.insert("/".to_string(), QID_ROOT);
        let mut qid_paths = HashMap::new();
        qid_paths.insert(QID_ROOT, "/".to_string());
        Ok(Self {
            default_root: canonical,
            readonly,
            policy: RwLock::new(PolicyStore::default()),
            namespaces: RwLock::new(HashMap::new()),
            validator: RwLock::new(None),
            path_qids: RwLock::new(path_qids),
            qid_paths: RwLock::new(qid_paths),
            next_qid: AtomicU64::new(QID_ROOT + 1),
        })
    }

    fn set_validator_hook(&self, hook: Option<Arc<ValidatorHook>>) {
        let mut slot = self.validator.write().unwrap();
        *slot = hook;
    }

    fn set_policy(&self, policy: SandboxPolicy) {
        let mut store = self.policy.write().unwrap();
        store.default = Some(policy);
    }

    fn clear_policy(&self) {
        let mut store = self.policy.write().unwrap();
        store.default = None;
    }

    fn set_agent_policy(&self, agent: &str, policy: SandboxPolicy) {
        let mut store = self.policy.write().unwrap();
        store.per_agent.insert(agent.to_string(), policy);
    }

    fn clear_agent_policy(&self, agent: &str) {
        let mut store = self.policy.write().unwrap();
        store.per_agent.remove(agent);
    }

    fn set_namespace(&self, agent: &str, root: PathBuf, read_only: bool) {
        let canonical = fs::canonicalize(&root).unwrap_or(root);
        let mut store = self.namespaces.write().unwrap();
        store.insert(
            agent.to_string(),
            NamespaceConfig {
                root: canonical,
                read_only,
            },
        );
    }

    fn clear_namespace(&self, agent: &str) {
        let mut store = self.namespaces.write().unwrap();
        store.remove(agent);
    }

    fn effective_root(&self, agent: &str) -> (PathBuf, bool) {
        let store = self.namespaces.read().unwrap();
        if let Some(ns) = store.get(agent) {
            return (ns.root.clone(), self.readonly || ns.read_only);
        }
        (self.default_root.clone(), self.readonly)
    }

    fn emit_violation(&self, file: &str, agent: &str) {
        if let Some(hook) = self.validator.read().unwrap().as_ref() {
            hook(
                "9p_access",
                file.to_string(),
                agent.to_string(),
                current_ts(),
            );
        }
    }

    fn policy_allows(&self, agent: &str, virt: &str, access: Access) -> bool {
        if let Some(policy) = self.policy.read().unwrap().for_agent(agent) {
            return policy.allows(virt, access);
        }
        match access {
            Access::Read => true,
            Access::Write => Self::default_write_allowed(agent, virt),
        }
    }

    fn check_access(&self, agent: &str, virt: &str, access: Access) -> Result<(), String> {
        if self.policy_allows(agent, virt, access) {
            return Ok(());
        }
        self.emit_violation(virt, agent);
        Err(format!("access denied for {agent} on {virt}"))
    }

    fn default_write_allowed(agent: &str, virt: &str) -> bool {
        if agent == "QueenPrimary" {
            return true;
        }
        if agent == "DroneWorker" {
            return !virt.starts_with("/history");
        }
        !virt.starts_with("/history") && !virt.starts_with("/srv")
    }

    fn normalize_virtual(path: &str) -> String {
        if path.is_empty() || path == "/" {
            return "/".to_string();
        }
        let mut out = String::with_capacity(path.len() + 1);
        if !path.starts_with('/') {
            out.push('/');
        }
        out.push_str(path.trim_end_matches('/'));
        if out.is_empty() {
            "/".to_string()
        } else {
            out
        }
    }

    fn join_virtual(parent: &str, child: &str) -> String {
        if child.is_empty() {
            return Self::normalize_virtual(parent);
        }
        if parent == "/" {
            return Self::normalize_virtual(&format!("/{child}"));
        }
        Self::normalize_virtual(&format!("{parent}/{child}"))
    }

    fn resolve_real_path(root: &Path, virt: &str) -> Result<PathBuf, String> {
        let mut path = root.to_path_buf();
        for comp in Path::new(virt).components() {
            match comp {
                Component::RootDir => {}
                Component::CurDir => {}
                Component::ParentDir => {
                    if !path.pop() {
                        return Err(format!("path escapes namespace: {virt}"));
                    }
                }
                Component::Normal(seg) => path.push(seg),
                Component::Prefix(_) => return Err("invalid path prefix".to_string()),
            }
        }
        if !path.starts_with(root) {
            return Err(format!("path escapes namespace: {virt}"));
        }
        Ok(path)
    }

    fn register_path(&self, virt: &str, is_dir: bool) -> u64 {
        let virt = Self::normalize_virtual(virt);
        if virt == "/" {
            return QID_ROOT;
        }
        if let Some(existing) = self.path_qids.read().unwrap().get(&virt) {
            return *existing;
        }
        let mut pq = self.path_qids.write().unwrap();
        if let Some(existing) = pq.get(&virt) {
            return *existing;
        }
        let qid = self.next_qid.fetch_add(1, Ordering::Relaxed);
        pq.insert(virt.clone(), qid);
        self.qid_paths.write().unwrap().insert(qid, virt.clone());
        if is_dir {
            if let Some(parent) = Path::new(&virt).parent().and_then(|p| p.to_str()) {
                let parent = if parent.is_empty() { "/" } else { parent };
                self.register_path(parent, true);
            }
        }
        qid
    }

    fn path_from_qid(&self, qid: u64) -> Result<String, String> {
        self.qid_paths
            .read()
            .unwrap()
            .get(&qid)
            .cloned()
            .ok_or_else(|| format!("unknown qid {qid}"))
    }

    fn file_meta(&self, virt: &str, is_dir: bool) -> FileMeta {
        let qid = self.register_path(virt, is_dir);
        if virt == "/" {
            return FileMeta::dir("/", qid);
        }
        let name = Path::new(virt)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(virt)
            .to_string();
        if is_dir {
            FileMeta::dir(name, qid)
        } else {
            FileMeta::file(name, qid)
        }
    }

    fn build_stat(&self, virt: &str, meta: &fs::Metadata) -> Result<Stat, String> {
        let is_dir = meta.is_dir();
        let fm = self.file_meta(virt, is_dir);
        let perms = if is_dir {
            Perm::DIR | Perm::OWNER_READ | Perm::OWNER_WRITE | Perm::OWNER_EXEC
        } else {
            Perm::FILE | Perm::OWNER_READ | Perm::OWNER_WRITE | Perm::GROUP_READ | Perm::OTHER_READ
        };
        let n_bytes = if is_dir { 0 } else { meta.len() };
        let atime = meta.accessed().unwrap_or_else(|_| SystemTime::UNIX_EPOCH);
        let mtime = meta.modified().unwrap_or_else(|_| SystemTime::UNIX_EPOCH);
        Ok(Stat {
            fm,
            perms,
            n_bytes,
            last_accesses: atime,
            last_modified: mtime,
            owner: "cohesix".into(),
            group: "cohesix".into(),
            last_modified_by: "cohesix".into(),
        })
    }

    fn walk(&self, parent_qid: u64, child: &str, agent: &str) -> Result<FileMeta, String> {
        let parent = self.path_from_qid(parent_qid)?;
        let target = Self::join_virtual(&parent, child);
        let (root, _) = self.effective_root(agent);
        let real = Self::resolve_real_path(&root, &target)?;
        let meta = fs::metadata(&real).map_err(|e| format!("walk: {e}"))?;
        let fm = self.file_meta(&target, meta.is_dir());
        Ok(fm)
    }

    fn open(&self, qid: u64, agent: &str, mode: Mode) -> Result<IoUnit, String> {
        let virt = self.path_from_qid(qid)?;
        let access = if mode.contains(Mode::DIR) {
            Access::Read
        } else {
            Access::Read
        };
        self.check_access(agent, &virt, access)?;
        Ok(32 * 1024)
    }

    fn create(
        &self,
        parent_qid: u64,
        name: &str,
        perm: Perm,
        agent: &str,
    ) -> Result<(FileMeta, IoUnit), String> {
        let parent = self.path_from_qid(parent_qid)?;
        let target = Self::join_virtual(&parent, name);
        self.check_access(agent, &target, Access::Write)?;
        let (root, read_only) = self.effective_root(agent);
        if read_only {
            return Err("namespace is read-only".into());
        }
        let real = Self::resolve_real_path(&root, &target)?;
        if perm.contains(Perm::DIR) {
            fs::create_dir_all(&real).map_err(|e| format!("create dir: {e}"))?;
            let fm = self.file_meta(&target, true);
            Ok((fm, 32 * 1024))
        } else {
            if let Some(parent_dir) = real.parent() {
                fs::create_dir_all(parent_dir).map_err(|e| format!("create parent: {e}"))?;
            }
            File::create(&real).map_err(|e| format!("create file: {e}"))?;
            let fm = self.file_meta(&target, false);
            Ok((fm, 32 * 1024))
        }
    }

    fn read(
        &self,
        qid: u64,
        offset: usize,
        count: usize,
        agent: &str,
    ) -> Result<ReadOutcome, String> {
        let virt = self.path_from_qid(qid)?;
        self.check_access(agent, &virt, Access::Read)?;
        let (root, _) = self.effective_root(agent);
        let real = Self::resolve_real_path(&root, &virt)?;
        let meta = fs::metadata(&real).map_err(|e| format!("read meta: {e}"))?;
        if meta.is_dir() {
            let stats = self.read_dir(qid, agent)?;
            let mut buf = Vec::new();
            for stat in stats {
                let raw: RawStat = stat.into();
                raw.write_to(&mut buf).map_err(|e| e.to_string())?;
            }
            let end = buf.len().min(offset.saturating_add(count));
            let start = buf.len().min(offset);
            let slice = buf[start..end].to_vec();
            return Ok(ReadOutcome::Immediate(slice));
        }
        let mut file = File::open(&real).map_err(|e| format!("open: {e}"))?;
        file.seek(SeekFrom::Start(offset as u64))
            .map_err(|e| format!("seek: {e}"))?;
        let mut buf = vec![0u8; count];
        let n = file.read(&mut buf).map_err(|e| format!("read: {e}"))?;
        buf.truncate(n);
        Ok(ReadOutcome::Immediate(buf))
    }

    fn read_dir(&self, qid: u64, agent: &str) -> Result<Vec<Stat>, String> {
        let virt = self.path_from_qid(qid)?;
        self.check_access(agent, &virt, Access::Read)?;
        let (root, _) = self.effective_root(agent);
        let real = Self::resolve_real_path(&root, &virt)?;
        let mut stats = Vec::new();
        let entries = fs::read_dir(&real).map_err(|e| format!("read_dir: {e}"))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("read_dir entry: {e}"))?;
            let name = entry.file_name();
            let name = match name.to_str() {
                Some(n) => n,
                None => {
                    warn!("skipping non-utf8 path in {:?}", real);
                    continue;
                }
            };
            let child = Self::join_virtual(&virt, name);
            let meta = entry
                .metadata()
                .map_err(|e| format!("read_dir meta: {e}"))?;
            stats.push(self.build_stat(&child, &meta)?);
        }
        Ok(stats)
    }

    fn write(&self, qid: u64, offset: usize, data: &[u8], agent: &str) -> Result<usize, String> {
        let virt = self.path_from_qid(qid)?;
        self.check_access(agent, &virt, Access::Write)?;
        let (root, read_only) = self.effective_root(agent);
        if read_only {
            return Err("namespace is read-only".into());
        }
        let real = Self::resolve_real_path(&root, &virt)?;
        if let Some(parent) = real.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create parent: {e}"))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&real)
            .map_err(|e| format!("open write: {e}"))?;
        file.seek(SeekFrom::Start(offset as u64))
            .map_err(|e| format!("seek write: {e}"))?;
        file.write_all(data).map_err(|e| format!("write: {e}"))?;
        Ok(data.len())
    }

    fn remove(&self, qid: u64, agent: &str) -> Result<(), String> {
        let virt = self.path_from_qid(qid)?;
        self.check_access(agent, &virt, Access::Write)?;
        let (root, read_only) = self.effective_root(agent);
        if read_only {
            return Err("namespace is read-only".into());
        }
        let real = Self::resolve_real_path(&root, &virt)?;
        let meta = fs::metadata(&real).map_err(|e| format!("remove meta: {e}"))?;
        if meta.is_dir() {
            fs::remove_dir_all(&real).map_err(|e| format!("remove dir: {e}"))?;
        } else {
            fs::remove_file(&real).map_err(|e| format!("remove file: {e}"))?;
        }
        self.path_qids.write().unwrap().remove(&virt);
        self.qid_paths.write().unwrap().retain(|_, v| v != &virt);
        Ok(())
    }
}

#[derive(Clone)]
pub struct NinepBackend {
    inner: Arc<InnerBackend>,
}

impl NinepBackend {
    pub fn new(root: PathBuf, readonly: bool) -> io::Result<Self> {
        let inner = InnerBackend::new(root, readonly)?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    pub fn set_validator_hook<F>(&self, hook: F)
    where
        F: Fn(&'static str, String, String, u64) + Send + Sync + 'static,
    {
        self.inner.set_validator_hook(Some(Arc::new(hook)));
    }

    pub fn clear_validator_hook(&self) {
        self.inner.set_validator_hook(None);
    }

    pub fn set_policy(&self, policy: SandboxPolicy) {
        self.inner.set_policy(policy);
    }

    pub fn clear_policy(&self) {
        self.inner.clear_policy();
    }

    pub fn set_agent_policy(&self, agent: &str, policy: SandboxPolicy) {
        self.inner.set_agent_policy(agent, policy);
    }

    pub fn clear_agent_policy(&self, agent: &str) {
        self.inner.clear_agent_policy(agent);
    }

    pub fn set_namespace(&self, agent: &str, root: PathBuf, read_only: bool) {
        self.inner.set_namespace(agent, root, read_only);
    }

    pub fn clear_namespace(&self, agent: &str) {
        self.inner.clear_namespace(agent);
    }

    pub fn serve_stream<S: Stream>(&self, stream: S) -> JoinHandle<()> {
        Server::new(self.clone()).serve_stream(stream)
    }
}

impl Serve9p for NinepBackend {
    fn walk(
        &mut self,
        _cid: ClientId,
        parent_qid: u64,
        child: &str,
        uname: &str,
    ) -> ninep::Result<FileMeta> {
        self.inner.walk(parent_qid, child, uname).map_err(|e| {
            warn!("walk failed for {}:{} -> {}", uname, parent_qid, e);
            e
        })
    }

    fn open(&mut self, _cid: ClientId, qid: u64, mode: Mode, uname: &str) -> ninep::Result<IoUnit> {
        self.inner.open(qid, uname, mode).map_err(|e| {
            warn!("open failed for {} on {}: {}", uname, qid, e);
            e
        })
    }

    fn create(
        &mut self,
        _cid: ClientId,
        parent: u64,
        name: &str,
        perm: Perm,
        _mode: Mode,
        uname: &str,
    ) -> ninep::Result<(FileMeta, IoUnit)> {
        self.inner.create(parent, name, perm, uname).map_err(|e| {
            warn!("create failed for {} -> {name}: {}", uname, e);
            e
        })
    }

    fn read(
        &mut self,
        _cid: ClientId,
        qid: u64,
        offset: usize,
        count: usize,
        uname: &str,
    ) -> ninep::Result<ReadOutcome> {
        self.inner.read(qid, offset, count, uname).map_err(|e| {
            warn!("read failed for {} on {}: {}", uname, qid, e);
            e
        })
    }

    fn read_dir(&mut self, _cid: ClientId, qid: u64, uname: &str) -> ninep::Result<Vec<Stat>> {
        self.inner.read_dir(qid, uname).map_err(|e| {
            warn!("read_dir failed for {} on {}: {}", uname, qid, e);
            e
        })
    }

    fn write(
        &mut self,
        _cid: ClientId,
        qid: u64,
        offset: usize,
        data: Vec<u8>,
        uname: &str,
    ) -> ninep::Result<usize> {
        self.inner.write(qid, offset, &data, uname).map_err(|e| {
            warn!("write failed for {} on {}: {}", uname, qid, e);
            e
        })
    }

    fn remove(&mut self, _cid: ClientId, qid: u64, uname: &str) -> ninep::Result<()> {
        self.inner.remove(qid, uname).map_err(|e| {
            warn!("remove failed for {} on {}: {}", uname, qid, e);
            e
        })
    }

    fn clunk(&mut self, _cid: ClientId, _qid: u64) {}

    fn stat(&mut self, _cid: ClientId, qid: u64, uname: &str) -> ninep::Result<Stat> {
        let virt = self.inner.path_from_qid(qid).map_err(|e| {
            warn!("stat path failed: {}", e);
            e.clone()
        })?;
        self.inner
            .check_access(uname, &virt, Access::Read)
            .map_err(|e| {
                warn!("stat denied for {} on {}: {}", uname, virt, e);
                e.clone()
            })?;
        let (root, _) = self.inner.effective_root(uname);
        let real = InnerBackend::resolve_real_path(&root, &virt).map_err(|e| {
            warn!("stat resolve failed for {} on {}: {}", uname, virt, e);
            e
        })?;
        let meta = fs::metadata(&real).map_err(|e| format!("stat meta: {e}"))?;
        self.inner.build_stat(&virt, &meta).map_err(|e| {
            warn!("stat build failed for {} on {}: {}", uname, virt, e);
            e
        })
    }

    fn write_stat(
        &mut self,
        _cid: ClientId,
        _qid: u64,
        _stat: Stat,
        _uname: &str,
    ) -> ninep::Result<()> {
        Err("wstat not supported".to_string())
    }
}

struct ServerThread {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for ServerThread {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            if let Err(e) = handle.join() {
                error!("9P server thread join error: {:?}", e);
            }
        }
    }
}

pub struct FsServer {
    cfg: FsConfig,
    backend: NinepBackend,
    thread: Option<ServerThread>,
}

impl FsServer {
    pub fn new(cfg: FsConfig) -> Self {
        let backend =
            NinepBackend::new(PathBuf::from(&cfg.root), cfg.readonly).unwrap_or_else(|e| {
                warn!("failed to initialize backend root {}: {}", cfg.root, e);
                NinepBackend::new(PathBuf::from("/"), cfg.readonly).expect("backend init fallback")
            });
        Self {
            cfg,
            backend,
            thread: None,
        }
    }

    pub fn backend(&self) -> NinepBackend {
        self.backend.clone()
    }

    pub fn set_validator_hook<F>(&self, hook: F)
    where
        F: Fn(&'static str, String, String, u64) + Send + Sync + 'static,
    {
        self.backend.set_validator_hook(hook);
    }

    pub fn clear_validator_hook(&self) {
        self.backend.clear_validator_hook();
    }

    pub fn set_policy(&self, policy: SandboxPolicy) {
        self.backend.set_policy(policy);
    }

    pub fn clear_policy(&self) {
        self.backend.clear_policy();
    }

    pub fn set_agent_policy(&self, agent: &str, policy: SandboxPolicy) {
        self.backend.set_agent_policy(agent, policy);
    }

    pub fn set_namespace(&self, agent: &str, root: PathBuf, read_only: bool) {
        self.backend.set_namespace(agent, root, read_only);
    }

    pub fn start(&mut self) -> Result<(), CohError> {
        if self.thread.is_some() {
            return Ok(());
        }
        fs::create_dir_all(&self.cfg.root)?;
        let listener = TcpListener::bind(("0.0.0.0", self.cfg.port))?;
        listener.set_nonblocking(true)?;
        let backend = self.backend.clone();
        let running = Arc::new(AtomicBool::new(true));
        let running_loop = running.clone();
        let port = listener
            .local_addr()
            .map(|a| a.port())
            .unwrap_or(self.cfg.port);
        let handle = thread::Builder::new()
            .name(format!("cohesix-9p-{port}"))
            .spawn(move || accept_loop(listener, backend, running_loop))?;
        info!("9P server listening on 0.0.0.0:{port}");
        self.thread = Some(ServerThread {
            running,
            handle: Some(handle),
        });
        Ok(())
    }

    pub fn serve_stream<S: Stream>(&self, stream: S) -> JoinHandle<()> {
        self.backend.serve_stream(stream)
    }
}

fn accept_loop(listener: TcpListener, backend: NinepBackend, running: Arc<AtomicBool>) {
    while running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, addr)) => {
                spawn_session(stream, addr, backend.clone());
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                error!("9P accept error: {e}");
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn spawn_session(stream: std::net::TcpStream, addr: SocketAddr, backend: NinepBackend) {
    if let Err(e) = stream.set_nodelay(true) {
        warn!("failed to set TCP_NODELAY for {}: {}", addr, e);
    }
    info!("new 9P client from {}", addr);
    let _ = backend.serve_stream(stream);
}
