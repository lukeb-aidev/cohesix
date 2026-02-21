// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide Secure9P-backed mount helpers for coh.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
#[cfg(any(feature = "fuse", target_os = "linux"))]
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(any(feature = "fuse", target_os = "linux"))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(any(feature = "fuse", target_os = "linux"))]
use std::sync::Mutex;
#[cfg(any(feature = "fuse", target_os = "linux"))]
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context, Result};
use cohsh::client::CohClient;
use cohsh_core::Secure9pTransport;
use fs2::FileExt;
#[cfg(any(feature = "fuse", target_os = "linux"))]
use secure9p_codec::OpenMode;
use sha2::{Digest, Sha256};

use crate::console::ConsoleSession;
use crate::policy::CohPolicy;
use crate::rest::RestSession;
#[cfg(any(feature = "fuse", target_os = "linux"))]
use crate::CohAccess;
use crate::MAX_PATH_COMPONENTS;
#[cfg(any(feature = "fuse", target_os = "linux"))]
use crate::{list_dir, MAX_DIR_LIST_BYTES};

#[cfg(any(feature = "fuse", target_os = "linux"))]
const ROOT_INODE: u64 = 1;
#[cfg(any(feature = "fuse", target_os = "linux"))]
const TTL: Duration = Duration::from_secs(1);

/// Append-only offset tracker for mount writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendOnlyTracker {
    cursor: u64,
}

impl AppendOnlyTracker {
    /// Create a new append-only tracker starting at offset 0.
    #[must_use]
    pub fn new() -> Self {
        Self { cursor: 0 }
    }

    /// Validate the next write offset and advance the cursor.
    pub fn check_and_advance(&mut self, offset: i64, len: usize) -> Result<()> {
        if offset < 0 {
            return Err(anyhow!("append-only offset must be >= 0"));
        }
        let offset = offset as u64;
        if offset != self.cursor {
            return Err(anyhow!(
                "append-only offset mismatch: expected {} got {}",
                self.cursor,
                offset
            ));
        }
        self.cursor = self
            .cursor
            .checked_add(len as u64)
            .ok_or_else(|| anyhow!("append-only offset overflow"))?;
        Ok(())
    }
}

impl Default for AppendOnlyTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Exclusive lock for REST-backed mounts (one mount per gateway URL).
#[derive(Debug)]
pub struct RestMountLock {
    path: PathBuf,
    file: std::fs::File,
}

impl RestMountLock {
    /// Acquire the REST mount lock keyed by the gateway URL.
    pub fn acquire(rest_url: &str) -> Result<Self> {
        let mut hasher = Sha256::new();
        hasher.update(rest_url.as_bytes());
        let digest = hasher.finalize();
        let suffix = hex::encode(digest);
        let path = std::env::temp_dir().join(format!("coh-rest-mount-{suffix}.lock"));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .with_context(|| format!("open rest mount lock {}", path.display()))?;
        if let Err(err) = file.try_lock_exclusive() {
            if err.kind() == std::io::ErrorKind::WouldBlock {
                return Err(anyhow!(
                    "rest mount already active for {rest_url}; only one rest mount is allowed"
                ));
            }
            return Err(err).with_context(|| format!("lock rest mount {}", path.display()));
        }
        file.set_len(0)
            .with_context(|| format!("truncate rest mount lock {}", path.display()))?;
        writeln!(file, "rest_url={rest_url}")
            .with_context(|| format!("write rest mount lock {}", path.display()))?;
        Ok(Self { path, file })
    }
}

impl Drop for RestMountLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Mount validator enforcing allowlist and path constraints.
#[derive(Debug, Clone)]
pub struct MountValidator {
    root: String,
    allowlist: Vec<String>,
    allow_all_under_root: bool,
}

impl MountValidator {
    /// Build a validator from the manifest-derived policy.
    pub fn from_policy(policy: &CohPolicy) -> Result<Self> {
        let mut root = policy.mount.root.trim().to_owned();
        if root.len() > 1 && root.ends_with('/') {
            while root.ends_with('/') {
                root.pop();
            }
        }
        if root.is_empty() {
            root.push('/');
        }
        let allow_all_under_root = policy.mount.allowlist.iter().any(|entry| entry == &root);
        Ok(Self {
            root,
            allowlist: policy.mount.allowlist.clone(),
            allow_all_under_root,
        })
    }

    /// Resolve a mount-relative path into a remote path.
    pub fn resolve_remote(&self, relative: &str) -> Result<String> {
        let relative = if relative.is_empty() { "/" } else { relative };
        if !relative.starts_with('/') {
            return Err(anyhow!("paths must be absolute"));
        }
        let remote = if self.root == "/" {
            relative.to_owned()
        } else if relative == "/" {
            self.root.clone()
        } else {
            format!("{}{}", self.root, relative)
        };
        validate_path(&remote)?;
        if !self.is_allowed(&remote) {
            return Err(anyhow!("path {remote} is not allowlisted"));
        }
        Ok(remote)
    }

    /// Return true if the supplied remote path is allowlisted.
    #[must_use]
    pub fn is_allowed(&self, remote: &str) -> bool {
        if remote == self.root {
            return true;
        }
        if self.allow_all_under_root {
            return remote.starts_with(&format!("{}/", self.root));
        }
        self.allowlist
            .iter()
            .any(|entry| remote == entry || remote.starts_with(&format!("{entry}/")))
    }

    /// Return the entries permitted under the mount root.
    pub fn root_entries(&self) -> Vec<String> {
        if self.allow_all_under_root {
            return Vec::new();
        }
        let mut entries = BTreeSet::new();
        for entry in &self.allowlist {
            if entry == &self.root {
                continue;
            }
            let rel = entry.strip_prefix(&self.root).unwrap_or(entry);
            let rel = rel.trim_start_matches('/');
            if rel.is_empty() {
                continue;
            }
            let first = rel.split('/').next().unwrap_or(rel);
            if !first.is_empty() {
                entries.insert(first.to_owned());
            }
        }
        entries.into_iter().collect()
    }

    /// Return the remote root path used for the mount.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Returns true when the allowlist permits all paths under the root.
    #[must_use]
    pub fn allow_all_under_root(&self) -> bool {
        self.allow_all_under_root
    }
}

/// Validate a path against Secure9P constraints.
pub fn validate_path(path: &str) -> Result<()> {
    if !path.starts_with('/') {
        return Err(anyhow!("paths must be absolute"));
    }
    let mut depth = 0usize;
    for component in path.split('/').skip(1) {
        if component.is_empty() {
            continue;
        }
        if component == "." || component == ".." {
            return Err(anyhow!("path component '{component}' is not permitted"));
        }
        if component.as_bytes().contains(&0) {
            return Err(anyhow!("path component contains NUL byte"));
        }
        depth += 1;
        if depth > MAX_PATH_COMPONENTS {
            return Err(anyhow!(
                "path exceeds maximum depth of {MAX_PATH_COMPONENTS} components"
            ));
        }
    }
    Ok(())
}

/// Validate mount policy without starting FUSE.
pub fn validate_mount(policy: &CohPolicy) -> Result<()> {
    let _ = MountValidator::from_policy(policy)?;
    Ok(())
}

/// Perform a mock mount validation and create the mount directory.
pub fn mock_mount(at: &Path, policy: &CohPolicy) -> Result<()> {
    validate_mount(policy)?;
    std::fs::create_dir_all(at)
        .with_context(|| format!("create mount directory {}", at.display()))?;
    Ok(())
}

/// Start a FUSE mount backed by Secure9P.
pub fn mount<T: Secure9pTransport + Send + 'static>(
    client: CohClient<T>,
    policy: &CohPolicy,
    at: &Path,
) -> Result<()> {
    #[cfg(any(feature = "fuse", target_os = "linux"))]
    {
        let validator = MountValidator::from_policy(policy)?;
        let filesystem = CohFuse::new(client, validator);
        let options = [
            fuser::MountOption::FSName("coh".to_owned()),
            fuser::MountOption::AutoUnmount,
        ];
        fuser::mount2(filesystem, at, &options)
            .with_context(|| format!("mount {}", at.display()))?;
        Ok(())
    }
    #[cfg(not(any(feature = "fuse", target_os = "linux")))]
    {
        let _ = client;
        let _ = policy;
        let _ = at;
        Err(anyhow!(
            "fuse support disabled; rebuild coh with --features fuse or use --mock"
        ))
    }
}

/// Start a FUSE mount backed by the TCP console transport.
pub fn mount_console(session: ConsoleSession, policy: &CohPolicy, at: &Path) -> Result<()> {
    #[cfg(any(feature = "fuse", target_os = "linux"))]
    {
        let validator = MountValidator::from_policy(policy)?;
        let filesystem = AccessFuse::new(session, validator, policy.telemetry.root.as_str());
        let options = [
            fuser::MountOption::FSName("coh".to_owned()),
            fuser::MountOption::AutoUnmount,
        ];
        fuser::mount2(filesystem, at, &options)
            .with_context(|| format!("mount {}", at.display()))?;
        Ok(())
    }
    #[cfg(not(any(feature = "fuse", target_os = "linux")))]
    {
        let _ = session;
        let _ = policy;
        let _ = at;
        Err(anyhow!(
            "fuse support disabled; rebuild coh with --features fuse or use --mock"
        ))
    }
}

/// Start a FUSE mount backed by the hive-gateway REST transport.
pub fn mount_rest(session: RestSession, policy: &CohPolicy, at: &Path) -> Result<()> {
    #[cfg(any(feature = "fuse", target_os = "linux"))]
    {
        let validator = MountValidator::from_policy(policy)?;
        let filesystem = AccessFuse::new(session, validator, policy.telemetry.root.as_str());
        let options = [
            fuser::MountOption::FSName("coh".to_owned()),
            fuser::MountOption::AutoUnmount,
        ];
        fuser::mount2(filesystem, at, &options)
            .with_context(|| format!("mount {}", at.display()))?;
        Ok(())
    }
    #[cfg(not(any(feature = "fuse", target_os = "linux")))]
    {
        let _ = session;
        let _ = policy;
        let _ = at;
        Err(anyhow!(
            "fuse support disabled; rebuild coh with --features fuse or use --mock"
        ))
    }
}

#[cfg(any(feature = "fuse", target_os = "linux"))]
struct CohFuse<T: Secure9pTransport> {
    client: Mutex<CohClient<T>>,
    validator: MountValidator,
    inodes: Mutex<InodeTable>,
    handles: Mutex<HashMap<u64, FileHandle>>,
    next_handle: AtomicU64,
}

#[cfg(any(feature = "fuse", target_os = "linux"))]
impl<T: Secure9pTransport> CohFuse<T> {
    fn new(client: CohClient<T>, validator: MountValidator) -> Self {
        let mut inodes = InodeTable::new();
        inodes.insert("/", true);
        Self {
            client: Mutex::new(client),
            validator,
            inodes: Mutex::new(inodes),
            handles: Mutex::new(HashMap::new()),
            next_handle: AtomicU64::new(1),
        }
    }

    fn attr_for(&self, inode: u64, is_dir: bool) -> fuser::FileAttr {
        let now = SystemTime::now();
        fuser::FileAttr {
            ino: inode,
            size: 0,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: if is_dir {
                fuser::FileType::Directory
            } else {
                fuser::FileType::RegularFile
            },
            // Allow the host kernel to attempt writes; the underlying Cohesix namespace enforces
            // append-only and allowlist policy, so we do not rely on POSIX perms for safety.
            perm: if is_dir { 0o755 } else { 0o666 },
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
            blksize: 512,
        }
    }

    fn stat_remote(&self, remote: &str) -> Result<(u64, bool)> {
        let mut client = self.client.lock().expect("coh client lock");
        let (fid, qid) = client.walk_qid(remote)?;
        let _ = client.clunk(fid);
        let is_dir = qid.ty().is_directory();
        Ok((qid.path(), is_dir))
    }

    fn resolve_inode_path(&self, inode: u64) -> Option<String> {
        let inodes = self.inodes.lock().expect("inode lock");
        inodes.path_for(inode).map(|entry| entry.path.clone())
    }

    fn list_root_entries(&self) -> Result<Vec<String>> {
        if self.validator.allow_all_under_root() {
            let mut client = self.client.lock().expect("coh client lock");
            let entries = list_dir(&mut *client, self.validator.root(), MAX_DIR_LIST_BYTES)?;
            return Ok(entries);
        }
        Ok(self.validator.root_entries())
    }
}

#[cfg(any(feature = "fuse", target_os = "linux"))]
impl<T: Secure9pTransport> fuser::Filesystem for CohFuse<T> {
    fn lookup(
        &mut self,
        _req: &fuser::Request<'_>,
        parent: u64,
        name: &std::ffi::OsStr,
        reply: fuser::ReplyEntry,
    ) {
        let parent_path = match self.resolve_inode_path(parent) {
            Some(path) => path,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };
        let name = name.to_string_lossy();
        let child_path = if parent_path == "/" {
            format!("/{name}")
        } else {
            format!("{parent_path}/{name}")
        };
        let remote = match self.validator.resolve_remote(&child_path) {
            Ok(remote) => remote,
            Err(_) => {
                reply.error(libc::EACCES);
                return;
            }
        };
        let is_dir = match self.stat_remote(&remote) {
            Ok((_, is_dir)) => is_dir,
            Err(_) => {
                reply.error(libc::ENOENT);
                return;
            }
        };
        let inode = {
            let mut inodes = self.inodes.lock().expect("inode lock");
            inodes.insert(&child_path, is_dir)
        };
        let attr = self.attr_for(inode, is_dir);
        reply.entry(&TTL, &attr, 0);
    }

    fn getattr(
        &mut self,
        _req: &fuser::Request<'_>,
        inode: u64,
        _fh: Option<u64>,
        reply: fuser::ReplyAttr,
    ) {
        let entry = {
            let inodes = self.inodes.lock().expect("inode lock");
            inodes.path_for(inode).cloned()
        };
        let Some(entry) = entry else {
            reply.error(libc::ENOENT);
            return;
        };
        let attr = self.attr_for(inode, entry.is_dir);
        reply.attr(&TTL, &attr);
    }

    fn readdir(
        &mut self,
        _req: &fuser::Request<'_>,
        inode: u64,
        _fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectory,
    ) {
        let path = match self.resolve_inode_path(inode) {
            Some(path) => path,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };
        let entries = if path == "/" {
            match self.list_root_entries() {
                Ok(entries) => entries,
                Err(_) => {
                    reply.error(libc::EIO);
                    return;
                }
            }
        } else {
            let remote = match self.validator.resolve_remote(&path) {
                Ok(remote) => remote,
                Err(_) => {
                    reply.error(libc::EACCES);
                    return;
                }
            };
            let mut client = self.client.lock().expect("coh client lock");
            match list_dir(&mut *client, &remote, MAX_DIR_LIST_BYTES) {
                Ok(entries) => entries,
                Err(_) => {
                    reply.error(libc::EIO);
                    return;
                }
            }
        };
        let mut listing = Vec::with_capacity(entries.len().saturating_add(2));
        listing.push((inode, fuser::FileType::Directory, ".".to_owned()));
        listing.push((ROOT_INODE, fuser::FileType::Directory, "..".to_owned()));
        for entry in entries {
            let child_path = if path == "/" {
                format!("/{entry}")
            } else {
                format!("{path}/{entry}")
            };
            let remote = match self.validator.resolve_remote(&child_path) {
                Ok(remote) => remote,
                Err(_) => {
                    continue;
                }
            };
            // macOS uses the `readdir` entry type eagerly (unlike Linux where it is usually a hint),
            // so we must provide accurate directory/file classification here.
            let is_dir = match self.stat_remote(&remote) {
                Ok((_, is_dir)) => is_dir,
                Err(_) => false,
            };
            let inode = {
                let mut inodes = self.inodes.lock().expect("inode lock");
                inodes.insert(&child_path, is_dir)
            };
            let file_type = if is_dir {
                fuser::FileType::Directory
            } else {
                fuser::FileType::RegularFile
            };
            listing.push((inode, file_type, entry));
        }
        let start = offset.max(0) as usize;
        for (idx, (inode, file_type, name)) in listing.into_iter().enumerate().skip(start) {
            if reply.add(inode, (idx + 1) as i64, file_type, name) {
                break;
            }
        }
        reply.ok();
    }

    fn open(&mut self, _req: &fuser::Request<'_>, inode: u64, flags: i32, reply: fuser::ReplyOpen) {
        let path = match self.resolve_inode_path(inode) {
            Some(path) => path,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };
        let remote = match self.validator.resolve_remote(&path) {
            Ok(remote) => remote,
            Err(_) => {
                reply.error(libc::EACCES);
                return;
            }
        };
        let write = flags & libc::O_ACCMODE != libc::O_RDONLY;
        let mode = if write {
            OpenMode::write_append()
        } else {
            OpenMode::read_only()
        };
        let (fid, qid) = {
            let mut client = self.client.lock().expect("coh client lock");
            match client.open_with_qid(&remote, mode) {
                Ok(value) => value,
                Err(_) => {
                    reply.error(libc::EIO);
                    return;
                }
            }
        };
        if write && !qid.ty().is_append_only() {
            let mut client = self.client.lock().expect("coh client lock");
            let _ = client.clunk(fid);
            reply.error(libc::EACCES);
            return;
        }
        if qid.ty().is_directory() && write {
            let mut client = self.client.lock().expect("coh client lock");
            let _ = client.clunk(fid);
            reply.error(libc::EISDIR);
            return;
        }
        let handle = self.next_handle.fetch_add(1, Ordering::Relaxed);
        let file_handle = FileHandle {
            fid,
            append_tracker: AppendOnlyTracker::new(),
        };
        self.handles
            .lock()
            .expect("handle lock")
            .insert(handle, file_handle);
        reply.opened(handle, 0);
    }

    fn read(
        &mut self,
        _req: &fuser::Request<'_>,
        _inode: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyData,
    ) {
        let handle = {
            let handles = self.handles.lock().expect("handle lock");
            handles.get(&fh).cloned()
        };
        let Some(handle) = handle else {
            reply.error(libc::EBADF);
            return;
        };
        if offset < 0 {
            reply.error(libc::EINVAL);
            return;
        }
        let count = size.min(cohsh::SECURE9P_MSIZE);
        let mut client = self.client.lock().expect("coh client lock");
        let data = match client.read(handle.fid, offset as u64, count) {
            Ok(data) => data,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };
        reply.data(&data);
    }

    fn write(
        &mut self,
        _req: &fuser::Request<'_>,
        _inode: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyWrite,
    ) {
        let mut handles = self.handles.lock().expect("handle lock");
        let handle = match handles.get_mut(&fh) {
            Some(handle) => handle,
            None => {
                reply.error(libc::EBADF);
                return;
            }
        };
        if let Err(_) = handle.append_tracker.check_and_advance(offset, data.len()) {
            reply.error(libc::EINVAL);
            return;
        }
        let mut client = self.client.lock().expect("coh client lock");
        let written = match client.write(handle.fid, u64::MAX, data) {
            Ok(written) => written,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };
        reply.written(written);
    }

    fn release(
        &mut self,
        _req: &fuser::Request<'_>,
        _inode: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        let handle = {
            let mut handles = self.handles.lock().expect("handle lock");
            handles.remove(&fh)
        };
        if let Some(handle) = handle {
            let mut client = self.client.lock().expect("coh client lock");
            let _ = client.clunk(handle.fid);
        }
        reply.ok();
    }
}

#[cfg(any(feature = "fuse", target_os = "linux"))]
struct AccessFuse<C: CohAccess + Send> {
    client: Mutex<C>,
    validator: MountValidator,
    telemetry_root: String,
    inodes: Mutex<InodeTable>,
    handles: Mutex<HashMap<u64, AccessHandle>>,
    next_handle: AtomicU64,
}

#[cfg(any(feature = "fuse", target_os = "linux"))]
impl<C: CohAccess + Send> AccessFuse<C> {
    fn new(client: C, validator: MountValidator, telemetry_root: impl Into<String>) -> Self {
        let mut inodes = InodeTable::new();
        inodes.insert("/", true);
        let mut telemetry_root = telemetry_root.into();
        if telemetry_root.len() > 1 && telemetry_root.ends_with('/') {
            while telemetry_root.ends_with('/') {
                telemetry_root.pop();
            }
        }
        if telemetry_root.is_empty() {
            telemetry_root.push('/');
        }
        Self {
            client: Mutex::new(client),
            validator,
            telemetry_root,
            inodes: Mutex::new(inodes),
            handles: Mutex::new(HashMap::new()),
            next_handle: AtomicU64::new(1),
        }
    }

    fn attr_for(&self, inode: u64, is_dir: bool, size: u64) -> fuser::FileAttr {
        let now = SystemTime::now();
        fuser::FileAttr {
            ino: inode,
            size: if is_dir { 0 } else { size },
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: if is_dir {
                fuser::FileType::Directory
            } else {
                fuser::FileType::RegularFile
            },
            // Allow the host kernel to attempt writes; the underlying Cohesix namespace enforces
            // append-only and allowlist policy, so we do not rely on POSIX perms for safety.
            perm: if is_dir { 0o755 } else { 0o666 },
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
            blksize: 512,
        }
    }

    fn resolve_inode_path(&self, inode: u64) -> Option<String> {
        let inodes = self.inodes.lock().expect("inode lock");
        inodes.path_for(inode).map(|entry| entry.path.clone())
    }

    fn telemetry_dynamic_kind(&self, parent_path: &str, name: &str) -> Option<bool> {
        // Telemetry ingest uses OS-owned creation with bounded namespaces, but operators still need
        // to address new device roots before they appear in listings.
        let root = self.telemetry_root.as_str();
        if parent_path == root {
            return Some(true);
        }
        let prefix = format!("{root}/");
        let rest = parent_path.strip_prefix(prefix.as_str())?;
        if rest.is_empty() {
            return Some(true);
        }
        // Only allow the first-level `/queen/telemetry/<device>` nodes to resolve dynamically.
        if rest.contains('/') {
            return None;
        }
        match name {
            "ctl" | "latest" => Some(false),
            "seg" => Some(true),
            _ => None,
        }
    }

    fn file_size_bytes(&self, remote: &str) -> u64 {
        let mut client = self.client.lock().expect("coh client lock");
        match client.read_file(remote, MAX_DIR_LIST_BYTES) {
            Ok(data) => data.len() as u64,
            Err(err) if err.to_string().contains("exceeds max bytes") => MAX_DIR_LIST_BYTES as u64,
            Err(_) => 0,
        }
    }

    fn list_root_entries(&self) -> Result<Vec<String>> {
        if self.validator.allow_all_under_root() {
            let mut client = self.client.lock().expect("coh client lock");
            let entries = client.list_dir(self.validator.root(), MAX_DIR_LIST_BYTES)?;
            return Ok(entries);
        }
        Ok(self.validator.root_entries())
    }

    fn probe_is_dir(&self, remote: &str) -> Result<bool> {
        let mut client = self.client.lock().expect("coh client lock");
        match client.list_dir(remote, MAX_DIR_LIST_BYTES) {
            Ok(_) => return Ok(true),
            Err(err) if err.to_string().contains("exceeds max bytes") => return Ok(true),
            Err(_) => {}
        }
        Ok(false)
    }
}

#[cfg(any(feature = "fuse", target_os = "linux"))]
impl<C: CohAccess + Send> fuser::Filesystem for AccessFuse<C> {
    fn lookup(
        &mut self,
        _req: &fuser::Request<'_>,
        parent: u64,
        name: &std::ffi::OsStr,
        reply: fuser::ReplyEntry,
    ) {
        let parent_path = match self.resolve_inode_path(parent) {
            Some(path) => path,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };
        let name = name.to_string_lossy();
        let child_path = if parent_path == "/" {
            format!("/{name}")
        } else {
            format!("{parent_path}/{name}")
        };
        let remote = match self.validator.resolve_remote(&child_path) {
            Ok(remote) => remote,
            Err(_) => {
                reply.error(libc::EACCES);
                return;
            }
        };
        let dynamic_kind = self.telemetry_dynamic_kind(&parent_path, name.as_ref());
        // Prefer directory enumeration for existence checks so write-only files still resolve.
        let entries = if parent_path == "/" {
            match self.list_root_entries() {
                Ok(entries) => entries,
                Err(_) => {
                    reply.error(libc::EIO);
                    return;
                }
            }
        } else {
            let parent_remote = match self.validator.resolve_remote(&parent_path) {
                Ok(remote) => remote,
                Err(_) => {
                    reply.error(libc::EACCES);
                    return;
                }
            };
            let mut client = self.client.lock().expect("coh client lock");
            match client.list_dir(&parent_remote, MAX_DIR_LIST_BYTES) {
                Ok(entries) => entries,
                Err(_) => {
                    if dynamic_kind.is_none() {
                        reply.error(libc::EIO);
                        return;
                    }
                    Vec::new()
                }
            }
        };
        if !entries.iter().any(|entry| entry == name.as_ref()) && dynamic_kind.is_none() {
            reply.error(libc::ENOENT);
            return;
        }

        let is_dir = if let Some(kind) = dynamic_kind {
            kind
        } else {
            match self.probe_is_dir(&remote) {
                Ok(is_dir) => is_dir,
                Err(_) => {
                    reply.error(libc::EIO);
                    return;
                }
            }
        };
        let inode = {
            let mut inodes = self.inodes.lock().expect("inode lock");
            inodes.insert(&child_path, is_dir)
        };
        let size = if is_dir {
            0
        } else {
            self.file_size_bytes(&remote)
        };
        let attr = self.attr_for(inode, is_dir, size);
        reply.entry(&TTL, &attr, 0);
    }

    fn getattr(
        &mut self,
        _req: &fuser::Request<'_>,
        inode: u64,
        _fh: Option<u64>,
        reply: fuser::ReplyAttr,
    ) {
        let entry = {
            let inodes = self.inodes.lock().expect("inode lock");
            inodes.path_for(inode).cloned()
        };
        let Some(entry) = entry else {
            reply.error(libc::ENOENT);
            return;
        };
        let size = if entry.is_dir {
            0
        } else {
            match self.validator.resolve_remote(entry.path.as_str()) {
                Ok(remote) => self.file_size_bytes(&remote),
                Err(_) => 0,
            }
        };
        let attr = self.attr_for(inode, entry.is_dir, size);
        reply.attr(&TTL, &attr);
    }

    fn readdir(
        &mut self,
        _req: &fuser::Request<'_>,
        inode: u64,
        _fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectory,
    ) {
        let path = match self.resolve_inode_path(inode) {
            Some(path) => path,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };
        let entries = if path == "/" {
            match self.list_root_entries() {
                Ok(entries) => entries,
                Err(_) => {
                    reply.error(libc::EIO);
                    return;
                }
            }
        } else {
            let remote = match self.validator.resolve_remote(&path) {
                Ok(remote) => remote,
                Err(_) => {
                    reply.error(libc::EACCES);
                    return;
                }
            };
            let mut client = self.client.lock().expect("coh client lock");
            match client.list_dir(&remote, MAX_DIR_LIST_BYTES) {
                Ok(entries) => entries,
                Err(_) => {
                    reply.error(libc::EIO);
                    return;
                }
            }
        };
        let mut listing = Vec::with_capacity(entries.len().saturating_add(2));
        listing.push((inode, fuser::FileType::Directory, ".".to_owned()));
        listing.push((ROOT_INODE, fuser::FileType::Directory, "..".to_owned()));
        for entry in entries {
            let child_path = if path == "/" {
                format!("/{entry}")
            } else {
                format!("{path}/{entry}")
            };
            let remote = match self.validator.resolve_remote(&child_path) {
                Ok(remote) => remote,
                Err(_) => {
                    continue;
                }
            };
            // macOS uses the `readdir` entry type eagerly (unlike Linux where it is usually a hint),
            // so we must provide accurate directory/file classification here.
            let is_dir =
                if let Some(kind) = self.telemetry_dynamic_kind(path.as_str(), entry.as_str()) {
                    kind
                } else {
                    // CohAccess does not expose stat/qid metadata, so probe via directory listing.
                    self.probe_is_dir(&remote).unwrap_or(false)
                };
            let inode = {
                let mut inodes = self.inodes.lock().expect("inode lock");
                inodes.insert(&child_path, is_dir)
            };
            let file_type = if is_dir {
                fuser::FileType::Directory
            } else {
                fuser::FileType::RegularFile
            };
            listing.push((inode, file_type, entry));
        }
        let start = offset.max(0) as usize;
        for (idx, (inode, file_type, name)) in listing.into_iter().enumerate().skip(start) {
            if reply.add(inode, (idx + 1) as i64, file_type, name) {
                break;
            }
        }
        reply.ok();
    }

    fn open(&mut self, _req: &fuser::Request<'_>, inode: u64, flags: i32, reply: fuser::ReplyOpen) {
        let write = flags & libc::O_ACCMODE != libc::O_RDONLY;
        let entry = {
            let inodes = self.inodes.lock().expect("inode lock");
            inodes.path_for(inode).cloned()
        };
        let Some(entry) = entry else {
            reply.error(libc::ENOENT);
            return;
        };
        if entry.is_dir && write {
            reply.error(libc::EISDIR);
            return;
        }
        let remote = match self.validator.resolve_remote(&entry.path) {
            Ok(remote) => remote,
            Err(_) => {
                reply.error(libc::EACCES);
                return;
            }
        };
        let handle = self.next_handle.fetch_add(1, Ordering::Relaxed);
        let mut tracker = AppendOnlyTracker::new();
        if write && !entry.is_dir {
            tracker.cursor = self.file_size_bytes(&remote);
        }
        let file_handle = AccessHandle {
            path: remote,
            append_tracker: tracker,
        };
        self.handles
            .lock()
            .expect("handle lock")
            .insert(handle, file_handle);
        reply.opened(handle, 0);
    }

    fn read(
        &mut self,
        _req: &fuser::Request<'_>,
        _inode: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyData,
    ) {
        let handle = {
            let handles = self.handles.lock().expect("handle lock");
            handles.get(&fh).cloned()
        };
        let Some(handle) = handle else {
            reply.error(libc::EBADF);
            return;
        };
        if offset < 0 {
            reply.error(libc::EINVAL);
            return;
        }
        let mut client = self.client.lock().expect("coh client lock");
        let data: Vec<u8> = match client.read_file(&handle.path, MAX_DIR_LIST_BYTES) {
            Ok(data) => data,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };
        let offset = offset as usize;
        if offset >= data.len() {
            reply.data(&[]);
            return;
        }
        let count = size.min(cohsh::SECURE9P_MSIZE) as usize;
        let end = offset.saturating_add(count).min(data.len());
        reply.data(&data[offset..end]);
    }

    fn write(
        &mut self,
        _req: &fuser::Request<'_>,
        _inode: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyWrite,
    ) {
        let mut handles = self.handles.lock().expect("handle lock");
        let handle = match handles.get_mut(&fh) {
            Some(handle) => handle,
            None => {
                reply.error(libc::EBADF);
                return;
            }
        };
        if handle
            .append_tracker
            .check_and_advance(offset, data.len())
            .is_err()
        {
            if offset >= 0 {
                handle.append_tracker.cursor = self.file_size_bytes(&handle.path);
            }
            if handle
                .append_tracker
                .check_and_advance(offset, data.len())
                .is_err()
            {
                reply.error(libc::EINVAL);
                return;
            }
        }
        let mut client = self.client.lock().expect("coh client lock");
        let written: usize = match client.write_append(&handle.path, data) {
            Ok(written) => written,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };
        let written = written.min(u32::MAX as usize) as u32;
        reply.written(written);
    }

    fn release(
        &mut self,
        _req: &fuser::Request<'_>,
        _inode: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        let handle = {
            let mut handles = self.handles.lock().expect("handle lock");
            handles.remove(&fh)
        };
        if handle.is_none() {
            reply.error(libc::EBADF);
            return;
        }
        reply.ok();
    }
}

#[cfg(any(feature = "fuse", target_os = "linux"))]
#[derive(Debug, Clone)]
struct AccessHandle {
    path: String,
    append_tracker: AppendOnlyTracker,
}

#[cfg(any(feature = "fuse", target_os = "linux"))]
#[derive(Debug, Clone)]
struct FileHandle {
    fid: u32,
    append_tracker: AppendOnlyTracker,
}

#[cfg(any(feature = "fuse", target_os = "linux"))]
#[derive(Debug, Clone)]
struct InodeEntry {
    path: String,
    is_dir: bool,
}

#[cfg(any(feature = "fuse", target_os = "linux"))]
#[derive(Debug, Default)]
struct InodeTable {
    by_inode: HashMap<u64, InodeEntry>,
    by_path: HashMap<String, u64>,
    next_inode: u64,
}

#[cfg(any(feature = "fuse", target_os = "linux"))]
impl InodeTable {
    fn new() -> Self {
        Self {
            by_inode: HashMap::new(),
            by_path: HashMap::new(),
            next_inode: ROOT_INODE + 1,
        }
    }

    fn insert(&mut self, path: &str, is_dir: bool) -> u64 {
        if let Some(existing) = self.by_path.get(path) {
            if let Some(entry) = self.by_inode.get_mut(existing) {
                entry.is_dir = is_dir;
            }
            return *existing;
        }
        let inode = if path == "/" {
            ROOT_INODE
        } else {
            self.next_inode
        };
        if inode == self.next_inode {
            self.next_inode = self.next_inode.saturating_add(1);
        }
        let entry = InodeEntry {
            path: path.to_owned(),
            is_dir,
        };
        self.by_inode.insert(inode, entry);
        self.by_path.insert(path.to_owned(), inode);
        inode
    }

    fn path_for(&self, inode: u64) -> Option<&InodeEntry> {
        self.by_inode.get(&inode)
    }
}

#[cfg(all(test, any(feature = "fuse", target_os = "linux")))]
mod access_fuse_tests {
    use super::*;

    #[derive(Debug, Clone)]
    enum DummyRead {
        Ok(Vec<u8>),
        Err(String),
    }

    #[derive(Debug)]
    struct DummyAccess {
        read: DummyRead,
    }

    impl DummyAccess {
        fn ok(bytes: impl Into<Vec<u8>>) -> Self {
            Self {
                read: DummyRead::Ok(bytes.into()),
            }
        }

        fn err(message: impl Into<String>) -> Self {
            Self {
                read: DummyRead::Err(message.into()),
            }
        }
    }

    impl CohAccess for DummyAccess {
        fn list_dir(&mut self, _path: &str, _max_bytes: usize) -> Result<Vec<String>> {
            Err(anyhow!("not implemented"))
        }

        fn read_file(&mut self, _path: &str, _max_bytes: usize) -> Result<Vec<u8>> {
            match &self.read {
                DummyRead::Ok(bytes) => Ok(bytes.clone()),
                DummyRead::Err(message) => Err(anyhow!("{message}")),
            }
        }

        fn write_append(&mut self, _path: &str, _payload: &[u8]) -> Result<usize> {
            Err(anyhow!("not implemented"))
        }
    }

    fn allow_all_validator() -> MountValidator {
        MountValidator {
            root: "/".to_owned(),
            allowlist: vec!["/".to_owned()],
            allow_all_under_root: true,
        }
    }

    #[test]
    fn telemetry_root_is_normalized() {
        let access = AccessFuse::new(
            DummyAccess::ok(Vec::<u8>::new()),
            allow_all_validator(),
            "/queen/telemetry/",
        );
        assert_eq!(access.telemetry_root, "/queen/telemetry");
    }

    #[test]
    fn telemetry_dynamic_kind_is_scoped_to_device_roots() {
        let access = AccessFuse::new(
            DummyAccess::ok(Vec::<u8>::new()),
            allow_all_validator(),
            "/queen/telemetry",
        );
        assert_eq!(
            access.telemetry_dynamic_kind("/queen/telemetry", "dev-a"),
            Some(true)
        );
        assert_eq!(
            access.telemetry_dynamic_kind("/queen/telemetry/dev-a", "ctl"),
            Some(false)
        );
        assert_eq!(
            access.telemetry_dynamic_kind("/queen/telemetry/dev-a", "latest"),
            Some(false)
        );
        assert_eq!(
            access.telemetry_dynamic_kind("/queen/telemetry/dev-a", "seg"),
            Some(true)
        );
        assert_eq!(
            access.telemetry_dynamic_kind("/queen/telemetry/dev-a", "other"),
            None
        );
        assert_eq!(
            access.telemetry_dynamic_kind("/queen/telemetry/dev-a/seg", "seg-000001"),
            None
        );
    }

    #[test]
    fn file_size_bytes_returns_remote_length() {
        let access = AccessFuse::new(
            DummyAccess::ok("hello\n"),
            allow_all_validator(),
            "/queen/telemetry",
        );
        assert_eq!(access.file_size_bytes("/proc/lifecycle/state"), 6);
    }

    #[test]
    fn file_size_bytes_caps_at_max_bytes_on_bounds_errors() {
        let access = AccessFuse::new(
            DummyAccess::err("read /log/queen.log exceeds max bytes 65536"),
            allow_all_validator(),
            "/queen/telemetry",
        );
        assert_eq!(
            access.file_size_bytes("/log/queen.log"),
            MAX_DIR_LIST_BYTES as u64
        );
    }

    #[test]
    fn fuse_file_perms_allow_writes() {
        let access = AccessFuse::new(
            DummyAccess::ok(Vec::<u8>::new()),
            allow_all_validator(),
            "/queen/telemetry",
        );
        let file_attr = access.attr_for(2, false, 1);
        assert_eq!(file_attr.perm, 0o666);
        let dir_attr = access.attr_for(1, true, 0);
        assert_eq!(dir_attr.perm, 0o755);
    }
}
