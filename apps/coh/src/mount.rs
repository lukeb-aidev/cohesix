// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide Secure9P-backed mount helpers for coh.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
#[cfg(feature = "fuse")]
use std::collections::HashMap;
use std::path::Path;
#[cfg(feature = "fuse")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "fuse")]
use std::sync::Mutex;
#[cfg(feature = "fuse")]
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context, Result};
use cohsh::client::CohClient;
use cohsh_core::Secure9pTransport;
#[cfg(feature = "fuse")]
use secure9p_codec::OpenMode;

use crate::console::ConsoleSession;
#[cfg(feature = "fuse")]
use crate::CohAccess;
#[cfg(feature = "fuse")]
use crate::{list_dir, MAX_DIR_LIST_BYTES};
use crate::MAX_PATH_COMPONENTS;
use crate::policy::CohPolicy;

#[cfg(feature = "fuse")]
const ROOT_INODE: u64 = 1;
#[cfg(feature = "fuse")]
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
        self.allowlist.iter().any(|entry| {
            remote == entry || remote.starts_with(&format!("{entry}/"))
        })
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
        if component.as_bytes().iter().any(|byte| *byte == 0) {
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
    #[cfg(feature = "fuse")]
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
    #[cfg(not(feature = "fuse"))]
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
pub fn mount_console(
    session: ConsoleSession,
    policy: &CohPolicy,
    at: &Path,
) -> Result<()> {
    #[cfg(feature = "fuse")]
    {
        let validator = MountValidator::from_policy(policy)?;
        let filesystem = ConsoleFuse::new(session, validator);
        let options = [
            fuser::MountOption::FSName("coh".to_owned()),
            fuser::MountOption::AutoUnmount,
        ];
        fuser::mount2(filesystem, at, &options)
            .with_context(|| format!("mount {}", at.display()))?;
        Ok(())
    }
    #[cfg(not(feature = "fuse"))]
    {
        let _ = session;
        let _ = policy;
        let _ = at;
        Err(anyhow!(
            "fuse support disabled; rebuild coh with --features fuse or use --mock"
        ))
    }
}

#[cfg(feature = "fuse")]
struct CohFuse<T: Secure9pTransport> {
    client: Mutex<CohClient<T>>,
    validator: MountValidator,
    inodes: Mutex<InodeTable>,
    handles: Mutex<HashMap<u64, FileHandle>>,
    next_handle: AtomicU64,
}

#[cfg(feature = "fuse")]
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

    fn attr_for(inode: u64, is_dir: bool) -> fuser::FileAttr {
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
            perm: if is_dir { 0o755 } else { 0o644 },
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
            let entries = list_dir(
                &mut *client,
                self.validator.root(),
                MAX_DIR_LIST_BYTES,
            )?;
            return Ok(entries);
        }
        Ok(self.validator.root_entries())
    }
}

#[cfg(feature = "fuse")]
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
        let attr = Self::attr_for(inode, is_dir);
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
        let attr = Self::attr_for(inode, entry.is_dir);
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
            let _remote = match self.validator.resolve_remote(&child_path) {
                Ok(remote) => remote,
                Err(_) => {
                    continue;
                }
            };
            let is_dir = path == "/" && !self.validator.allow_all_under_root();
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

    fn open(
        &mut self,
        _req: &fuser::Request<'_>,
        inode: u64,
        flags: i32,
        reply: fuser::ReplyOpen,
    ) {
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
            is_dir: qid.ty().is_directory(),
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

#[cfg(feature = "fuse")]
struct ConsoleFuse {
    client: Mutex<ConsoleSession>,
    validator: MountValidator,
    inodes: Mutex<InodeTable>,
    handles: Mutex<HashMap<u64, ConsoleHandle>>,
    next_handle: AtomicU64,
}

#[cfg(feature = "fuse")]
impl ConsoleFuse {
    fn new(session: ConsoleSession, validator: MountValidator) -> Self {
        let mut inodes = InodeTable::new();
        inodes.insert("/", true);
        Self {
            client: Mutex::new(session),
            validator,
            inodes: Mutex::new(inodes),
            handles: Mutex::new(HashMap::new()),
            next_handle: AtomicU64::new(1),
        }
    }

    fn attr_for(inode: u64, is_dir: bool) -> fuser::FileAttr {
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
            perm: if is_dir { 0o755 } else { 0o644 },
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

    fn list_root_entries(&self) -> Result<Vec<String>> {
        if self.validator.allow_all_under_root() {
            let mut client = self.client.lock().expect("coh client lock");
            let entries = client.list_dir(
                self.validator.root(),
                MAX_DIR_LIST_BYTES,
            )?;
            return Ok(entries);
        }
        Ok(self.validator.root_entries())
    }

    fn stat_remote(&self, remote: &str) -> Result<bool> {
        let mut client = self.client.lock().expect("coh client lock");
        if client.list_dir(remote, MAX_DIR_LIST_BYTES).is_ok() {
            return Ok(true);
        }
        if client.read_file(remote, usize::MAX).is_ok() {
            return Ok(false);
        }
        Err(anyhow!("path {remote} not found"))
    }
}

#[cfg(feature = "fuse")]
impl fuser::Filesystem for ConsoleFuse {
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
            Ok(is_dir) => is_dir,
            Err(_) => {
                reply.error(libc::ENOENT);
                return;
            }
        };
        let inode = {
            let mut inodes = self.inodes.lock().expect("inode lock");
            inodes.insert(&child_path, is_dir)
        };
        let attr = Self::attr_for(inode, is_dir);
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
        let attr = Self::attr_for(inode, entry.is_dir);
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
            let _remote = match self.validator.resolve_remote(&child_path) {
                Ok(remote) => remote,
                Err(_) => {
                    continue;
                }
            };
            let is_dir = path == "/" && !self.validator.allow_all_under_root();
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

    fn open(
        &mut self,
        _req: &fuser::Request<'_>,
        inode: u64,
        flags: i32,
        reply: fuser::ReplyOpen,
    ) {
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
        let is_dir = match self.stat_remote(&remote) {
            Ok(is_dir) => is_dir,
            Err(_) => {
                reply.error(libc::ENOENT);
                return;
            }
        };
        let write = flags & libc::O_ACCMODE != libc::O_RDONLY;
        if is_dir && write {
            reply.error(libc::EISDIR);
            return;
        }
        let handle = self.next_handle.fetch_add(1, Ordering::Relaxed);
        let file_handle = ConsoleHandle {
            path: remote,
            is_dir,
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
        if let Err(_) = handle.append_tracker.check_and_advance(offset, data.len()) {
            reply.error(libc::EINVAL);
            return;
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

#[cfg(feature = "fuse")]
#[derive(Debug, Clone)]
struct ConsoleHandle {
    path: String,
    is_dir: bool,
    append_tracker: AppendOnlyTracker,
}

#[cfg(feature = "fuse")]
#[derive(Debug, Clone)]
struct FileHandle {
    fid: u32,
    is_dir: bool,
    append_tracker: AppendOnlyTracker,
}

#[cfg(feature = "fuse")]
#[derive(Debug, Clone)]
struct InodeEntry {
    path: String,
    is_dir: bool,
}

#[cfg(feature = "fuse")]
#[derive(Debug, Default)]
struct InodeTable {
    by_inode: HashMap<u64, InodeEntry>,
    by_path: HashMap<String, u64>,
    next_inode: u64,
}

#[cfg(feature = "fuse")]
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
        let inode = if path == "/" { ROOT_INODE } else { self.next_inode };
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
