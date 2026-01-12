// Author: Lukas Bower
// Purpose: Synthetic namespace builder backing the NineDoor Secure9P server.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use gpu_bridge_host::{GpuModelCatalog, TelemetrySchema};
use secure9p_codec::{ErrorCode, Qid, QidType};
use trace_model::TraceLevel;

use super::telemetry::{TelemetryAudit, TelemetryAuditLevel, TelemetryConfig, TelemetryFile};
use super::tracefs::TraceFs;
use crate::NineDoorError;

const SELFTEST_QUICK_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../resources/proc_tests/selftest_quick.coh"
));
const SELFTEST_FULL_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../resources/proc_tests/selftest_full.coh"
));
const SELFTEST_NEGATIVE_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../resources/proc_tests/selftest_negative.coh"
));

/// Synthetic namespace backing the NineDoor Secure9P server.
#[derive(Debug)]
pub struct Namespace {
    root: Node,
    trace: TraceFs,
    telemetry: TelemetryConfig,
}

impl Namespace {
    /// Construct the namespace with the predefined synthetic tree.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::new_with_telemetry(TelemetryConfig::default())
    }

    /// Construct the namespace with explicit telemetry configuration.
    pub fn new_with_telemetry(telemetry: TelemetryConfig) -> Self {
        let mut namespace = Self {
            root: Node::directory(Vec::new()),
            trace: TraceFs::new(),
            telemetry,
        };
        namespace.bootstrap();
        namespace
    }

    /// Retrieve the root Qid.
    pub fn root_qid(&self) -> Qid {
        self.root.qid
    }

    /// Read bytes from the supplied path.
    pub fn read(
        &mut self,
        path: &[String],
        offset: u64,
        count: u32,
    ) -> Result<Vec<u8>, NineDoorError> {
        enum ReadAction {
            Data(Vec<u8>, Option<TelemetryAudit>),
            TraceControl,
            TraceEvents,
            KernelMessages,
            TaskTrace(String),
        }

        let mut audit = None;
        let action = {
            let node = self.lookup_mut(path)?;
            match node.node.kind_mut() {
                NodeKind::Directory { .. } => {
                    let listing = render_directory_listing(node.list_children());
                    ReadAction::Data(read_slice(&listing, offset, count), None)
                }
                NodeKind::File(FileNode::ReadOnly(data))
                | NodeKind::File(FileNode::AppendOnly(data)) => {
                    ReadAction::Data(read_slice(data, offset, count), None)
                }
                NodeKind::File(FileNode::Telemetry(file)) => match file.read(offset, count) {
                    Ok(outcome) => ReadAction::Data(outcome.data, outcome.audit),
                    Err(err) => {
                        if let Some(audit) = err.audit {
                            self.record_telemetry_audit(audit)?;
                        }
                        return Err(NineDoorError::protocol(
                            ErrorCode::Invalid,
                            err.message,
                        ));
                    }
                },
                NodeKind::File(FileNode::TraceControl) => ReadAction::TraceControl,
                NodeKind::File(FileNode::TraceEvents) => ReadAction::TraceEvents,
                NodeKind::File(FileNode::KernelMessages) => ReadAction::KernelMessages,
                NodeKind::File(FileNode::TaskTrace(task)) => {
                    ReadAction::TaskTrace(task.clone())
                }
            }
        };
        let data = match action {
            ReadAction::Data(data, read_audit) => {
                audit = read_audit;
                data
            }
            ReadAction::TraceControl => self.trace.read_ctl(offset, count),
            ReadAction::TraceEvents => self.trace.read_events(offset, count),
            ReadAction::KernelMessages => self.trace.read_kmesg(offset, count),
            ReadAction::TaskTrace(task) => self.trace.read_task(&task, offset, count),
        };
        if let Some(audit) = audit {
            self.record_telemetry_audit(audit)?;
        }
        Ok(data)
    }

    /// Append bytes to the supplied path.
    pub fn write_append(
        &mut self,
        path: &[String],
        offset: u64,
        data: &[u8],
    ) -> Result<u32, NineDoorError> {
        let mut audit = None;
        let result = {
            let node = self.lookup_mut(path)?;
            match node.node.kind_mut() {
                NodeKind::File(FileNode::AppendOnly(buffer)) => {
                    buffer.extend_from_slice(data);
                    Ok(data.len() as u32)
                }
                NodeKind::File(FileNode::Telemetry(file)) => match file.append(offset, data) {
                    Ok(outcome) => {
                        audit = outcome.audit;
                        Ok(outcome.count)
                    }
                    Err(err) => {
                        audit = err.audit;
                        let code = match err.kind {
                            super::telemetry::TelemetryErrorKind::QuotaExceeded => {
                                ErrorCode::TooBig
                            }
                            super::telemetry::TelemetryErrorKind::InvalidOffset
                            | super::telemetry::TelemetryErrorKind::InvalidFrame
                            | super::telemetry::TelemetryErrorKind::CursorStale => ErrorCode::Invalid,
                        };
                        Err(NineDoorError::protocol(code, err.message))
                    }
                },
                NodeKind::File(FileNode::ReadOnly(_)) => Err(NineDoorError::protocol(
                    ErrorCode::Permission,
                    format!("cannot write read-only file /{}", join_path(path)),
                )),
                NodeKind::File(FileNode::TraceControl) => self.trace.write_ctl(data),
                NodeKind::File(FileNode::TraceEvents)
                | NodeKind::File(FileNode::KernelMessages)
                | NodeKind::File(FileNode::TaskTrace(_)) => Err(NineDoorError::protocol(
                    ErrorCode::Permission,
                    format!("cannot write read-only file /{}", join_path(path)),
                )),
                NodeKind::Directory { .. } => Err(NineDoorError::protocol(
                    ErrorCode::Permission,
                    format!("cannot write directory /{}", join_path(path)),
                )),
            }
        };
        if let Some(audit) = audit {
            self.record_telemetry_audit(audit)?;
        }
        result
    }

    /// Create namespace entries for a spawned worker.
    pub fn create_worker(&mut self, worker_id: &str) -> Result<(), NineDoorError> {
        if worker_id.is_empty() || worker_id.contains('/') {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("invalid worker id '{worker_id}'"),
            ));
        }
        let telemetry_config = self.telemetry;
        let worker_root = vec!["worker".to_owned()];
        let mut node = self.lookup_mut(&worker_root)?;
        if node.has_child(worker_id) {
            return Err(NineDoorError::protocol(
                ErrorCode::Busy,
                format!("worker {worker_id} already exists"),
            ));
        }
        let worker_dir = node.ensure_directory(worker_id);
        let telemetry_file = TelemetryFile::new(telemetry_config);
        worker_dir.ensure_file("telemetry", FileNode::Telemetry(telemetry_file));
        let proc_root = vec!["proc".to_owned()];
        self.ensure_dir(&proc_root, worker_id)?;
        let proc_path = vec!["proc".to_owned(), worker_id.to_owned()];
        let mut proc_node = self.lookup_mut(&proc_path)?;
        proc_node.ensure_file("trace", FileNode::TaskTrace(worker_id.to_owned()));
        Ok(())
    }

    /// Remove namespace entries for a killed worker.
    pub fn remove_worker(&mut self, worker_id: &str) -> Result<(), NineDoorError> {
        let worker_root = vec!["worker".to_owned()];
        let mut node = self.lookup_mut(&worker_root)?;
        if node.remove_child(worker_id).is_none() {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("worker {worker_id} not found"),
            ));
        }
        let proc_root = vec!["proc".to_owned()];
        if let Ok(mut proc_dir) = self.lookup_mut(&proc_root) {
            let _ = proc_dir.remove_child(worker_id);
        }
        Ok(())
    }

    /// Borrow the trace filesystem for mutation.
    pub fn tracefs_mut(&mut self) -> &mut TraceFs {
        &mut self.trace
    }

    fn record_telemetry_audit(&mut self, audit: TelemetryAudit) -> Result<(), NineDoorError> {
        let level = match audit.level {
            TelemetryAuditLevel::Info => TraceLevel::Info,
            TelemetryAuditLevel::Warn => TraceLevel::Warn,
        };
        self.trace
            .record(level, "telemetry", None, &audit.message);
        let log_path = vec!["log".to_owned(), "queen.log".to_owned()];
        let log_node = self.lookup_mut(&log_path)?;
        let line = format!("[audit] {}\n", audit.message);
        match log_node.node.kind_mut() {
            NodeKind::File(FileNode::AppendOnly(buffer)) => {
                buffer.extend_from_slice(line.as_bytes());
                Ok(())
            }
            _ => Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "cannot append telemetry audit line",
            )),
        }
    }

    /// Lookup a node by path.
    pub fn lookup(&self, path: &[String]) -> Result<NodeView<'_>, NineDoorError> {
        let mut node = &self.root;
        for component in path {
            node = node.child(component).ok_or_else(|| {
                NineDoorError::protocol(
                    ErrorCode::NotFound,
                    format!("path /{} not found", join_path(path)),
                )
            })?;
        }
        Ok(NodeView { node })
    }

    fn lookup_mut(&mut self, path: &[String]) -> Result<NodeViewMut<'_>, NineDoorError> {
        let mut node = &mut self.root;
        for component in path {
            node = node.child_mut(component).ok_or_else(|| {
                NineDoorError::protocol(
                    ErrorCode::NotFound,
                    format!("path /{} not found", join_path(path)),
                )
            })?;
        }
        Ok(NodeViewMut { node })
    }

    fn bootstrap(&mut self) {
        self.ensure_dir(&[], "proc").expect("create /proc");
        let proc_path = vec!["proc".to_owned()];
        let boot_text = b"Cohesix boot: root-task online\nspawned user-component endpoint 1\ntick 1\nPING 1\nPONG 1\ntick 2\ntick 3\nroot-task shutdown\n".to_vec();
        self.ensure_read_only_file(&proc_path, "boot", &boot_text)
            .expect("create /proc/boot");
        self.ensure_dir(&proc_path, "tests")
            .expect("create /proc/tests");
        let tests_path = vec!["proc".to_owned(), "tests".to_owned()];
        self.ensure_read_only_file(
            &tests_path,
            "selftest_quick.coh",
            SELFTEST_QUICK_SCRIPT.as_bytes(),
        )
        .expect("create /proc/tests/selftest_quick.coh");
        self.ensure_read_only_file(
            &tests_path,
            "selftest_full.coh",
            SELFTEST_FULL_SCRIPT.as_bytes(),
        )
        .expect("create /proc/tests/selftest_full.coh");
        self.ensure_read_only_file(
            &tests_path,
            "selftest_negative.coh",
            SELFTEST_NEGATIVE_SCRIPT.as_bytes(),
        )
        .expect("create /proc/tests/selftest_negative.coh");

        self.ensure_dir(&[], "log").expect("create /log");
        let log_path = vec!["log".to_owned()];
        self.ensure_append_only_file(&log_path, "queen.log", &boot_text)
            .expect("create /log/queen.log");

        self.ensure_dir(&[], "queen").expect("create /queen");
        let queen_path = vec!["queen".to_owned()];
        self.ensure_append_only_file(&queen_path, "ctl", b"")
            .expect("create /queen/ctl");

        self.ensure_dir(&[], "worker").expect("create /worker");
        self.ensure_dir(&[], "gpu").expect("create /gpu");
        self.ensure_dir(&[], "trace").expect("create /trace");
        let trace_path = vec!["trace".to_owned()];
        self.ensure_trace_control(&trace_path, "ctl")
            .expect("create /trace/ctl");
        self.ensure_trace_events(&trace_path, "events")
            .expect("create /trace/events");
        self.ensure_kernel_messages().expect("create /kmesg");
    }

    fn ensure_dir(&mut self, parent: &[String], name: &str) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.ensure_directory(name);
        Ok(())
    }

    fn ensure_read_only_file(
        &mut self,
        parent: &[String],
        name: &str,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.ensure_file(name, FileNode::ReadOnly(data.to_vec()));
        Ok(())
    }

    fn ensure_append_only_file(
        &mut self,
        parent: &[String],
        name: &str,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.ensure_file(name, FileNode::AppendOnly(data.to_vec()));
        Ok(())
    }

    fn ensure_trace_control(&mut self, parent: &[String], name: &str) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.ensure_file(name, FileNode::TraceControl);
        Ok(())
    }

    fn ensure_trace_events(&mut self, parent: &[String], name: &str) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.ensure_file(name, FileNode::TraceEvents);
        Ok(())
    }

    fn ensure_kernel_messages(&mut self) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(&[])?;
        node.ensure_file("kmesg", FileNode::KernelMessages);
        Ok(())
    }

    pub fn set_gpu_node(
        &mut self,
        gpu_id: &str,
        info: &[u8],
        ctl: &[u8],
        status: &[u8],
    ) -> Result<(), NineDoorError> {
        let root = vec!["gpu".to_owned()];
        let base = vec!["gpu".to_owned(), gpu_id.to_owned()];
        self.ensure_dir(&root, gpu_id)?;
        self.set_read_only_file(&base, "info", info)?;
        self.set_append_only_file(&base, "ctl", ctl)?;
        self.set_append_only_file(&base, "status", status)?;
        self.set_append_only_file(&base, "job", b"")?;
        Ok(())
    }

    /// Install the GPU model lifecycle namespace.
    pub fn set_gpu_models(&mut self, catalog: &GpuModelCatalog) -> Result<(), NineDoorError> {
        let root = vec!["gpu".to_owned()];
        self.ensure_dir(&root, "models")?;
        let models_root = vec!["gpu".to_owned(), "models".to_owned()];
        self.ensure_dir(&models_root, "available")?;
        let available_root = vec![
            "gpu".to_owned(),
            "models".to_owned(),
            "available".to_owned(),
        ];
        for manifest in &catalog.available {
            self.ensure_dir(&available_root, &manifest.model_id)?;
            let model_path = vec![
                "gpu".to_owned(),
                "models".to_owned(),
                "available".to_owned(),
                manifest.model_id.clone(),
            ];
            self.set_read_only_file(
                &model_path,
                "manifest.toml",
                manifest.manifest_toml.as_bytes(),
            )?;
        }
        self.set_append_only_file(
            &models_root,
            "active",
            catalog.active_pointer_payload().as_bytes(),
        )?;
        Ok(())
    }

    /// Install the telemetry schema descriptor under `/gpu/telemetry/schema.json`.
    pub fn set_gpu_telemetry_schema(
        &mut self,
        schema: &TelemetrySchema,
    ) -> Result<(), NineDoorError> {
        let root = vec!["gpu".to_owned()];
        self.ensure_dir(&root, "telemetry")?;
        let telemetry_root = vec!["gpu".to_owned(), "telemetry".to_owned()];
        self.set_read_only_file(
            &telemetry_root,
            "schema.json",
            schema.descriptor_json().as_bytes(),
        )?;
        Ok(())
    }

    pub fn append_gpu_status(&mut self, gpu_id: &str, payload: &[u8]) -> Result<(), NineDoorError> {
        let path = vec!["gpu".to_owned(), gpu_id.to_owned(), "status".to_owned()];
        self.write_append(&path, u64::MAX, payload)?;
        Ok(())
    }

    fn set_read_only_file(
        &mut self,
        parent: &[String],
        name: &str,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.remove_child(name);
        node.ensure_file(name, FileNode::ReadOnly(data.to_vec()));
        Ok(())
    }

    fn set_append_only_file(
        &mut self,
        parent: &[String],
        name: &str,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.remove_child(name);
        node.ensure_file(name, FileNode::AppendOnly(data.to_vec()));
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct Node {
    path: Vec<String>,
    qid: Qid,
    kind: NodeKind,
}

impl Node {
    fn directory(path: Vec<String>) -> Self {
        Self {
            qid: Qid::new(QidType::DIRECTORY, 0, hash_path(&path)),
            path,
            kind: NodeKind::Directory {
                children: BTreeMap::new(),
            },
        }
    }

    fn file(path: Vec<String>, file: FileNode) -> Self {
        let ty = match file {
            FileNode::ReadOnly(_) => QidType::FILE,
            FileNode::AppendOnly(_) => QidType::APPEND_ONLY,
            FileNode::Telemetry(_) => QidType::APPEND_ONLY,
            FileNode::TraceControl => QidType::APPEND_ONLY,
            FileNode::TraceEvents | FileNode::KernelMessages | FileNode::TaskTrace(_) => {
                QidType::FILE
            }
        };
        Self {
            qid: Qid::new(ty, 0, hash_path(&path)),
            path,
            kind: NodeKind::File(file),
        }
    }

    fn child(&self, name: &str) -> Option<&Node> {
        match &self.kind {
            NodeKind::Directory { children } => children.get(name),
            NodeKind::File(_) => None,
        }
    }

    fn child_mut(&mut self, name: &str) -> Option<&mut Node> {
        match &mut self.kind {
            NodeKind::Directory { children } => children.get_mut(name),
            NodeKind::File(_) => None,
        }
    }

    fn remove_child(&mut self, name: &str) -> Option<Node> {
        match &mut self.kind {
            NodeKind::Directory { children } => children.remove(name),
            NodeKind::File(_) => None,
        }
    }

    fn ensure_directory(&mut self, name: &str) -> &mut Node {
        match &mut self.kind {
            NodeKind::Directory { children } => {
                let mut path = self.path.clone();
                path.push(name.to_owned());
                children
                    .entry(name.to_owned())
                    .or_insert_with(|| Node::directory(path))
            }
            NodeKind::File(_) => panic!("cannot create directory under file"),
        }
    }

    fn ensure_file(&mut self, name: &str, file: FileNode) -> &mut Node {
        match &mut self.kind {
            NodeKind::Directory { children } => {
                let mut path = self.path.clone();
                path.push(name.to_owned());
                children
                    .entry(name.to_owned())
                    .or_insert_with(|| Node::file(path, file.clone()))
            }
            NodeKind::File(_) => panic!("cannot create file under file"),
        }
    }

    fn qid(&self) -> Qid {
        self.qid
    }

    fn is_directory(&self) -> bool {
        matches!(self.kind, NodeKind::Directory { .. })
    }

    fn kind_mut(&mut self) -> &mut NodeKind {
        &mut self.kind
    }
}

#[derive(Debug, Clone)]
enum NodeKind {
    Directory { children: BTreeMap<String, Node> },
    File(FileNode),
}

#[derive(Debug, Clone)]
enum FileNode {
    ReadOnly(Vec<u8>),
    AppendOnly(Vec<u8>),
    Telemetry(TelemetryFile),
    TraceControl,
    TraceEvents,
    KernelMessages,
    TaskTrace(String),
}

/// Borrowed node view used by callers.
pub struct NodeView<'a> {
    node: &'a Node,
}

impl<'a> NodeView<'a> {
    pub fn qid(&self) -> Qid {
        self.node.qid()
    }

    pub fn is_directory(&self) -> bool {
        self.node.is_directory()
    }
}

struct NodeViewMut<'a> {
    node: &'a mut Node,
}

impl<'a> NodeViewMut<'a> {
    fn ensure_directory(&mut self, name: &str) -> &mut Node {
        self.node.ensure_directory(name)
    }

    fn ensure_file(&mut self, name: &str, file: FileNode) -> &mut Node {
        self.node.ensure_file(name, file)
    }

    fn has_child(&self, name: &str) -> bool {
        self.node.child(name).is_some()
    }

    fn remove_child(&mut self, name: &str) -> Option<Node> {
        self.node.remove_child(name)
    }

    fn list_children(&self) -> Vec<String> {
        match &self.node.kind {
            NodeKind::Directory { children } => children.keys().cloned().collect(),
            NodeKind::File(_) => Vec::new(),
        }
    }
}

fn hash_path(path: &[String]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for component in path {
        component.hash(&mut hasher);
    }
    hasher.finish()
}

fn join_path(path: &[String]) -> String {
    if path.is_empty() {
        String::new()
    } else {
        path.join("/")
    }
}

fn read_slice(data: &[u8], offset: u64, count: u32) -> Vec<u8> {
    let start = offset as usize;
    if start >= data.len() {
        return Vec::new();
    }
    let end = start.saturating_add(count as usize).min(data.len());
    data[start..end].to_vec()
}

fn render_directory_listing(entries: Vec<String>) -> Vec<u8> {
    if entries.is_empty() {
        return Vec::new();
    }
    let mut output = String::new();
    for (idx, entry) in entries.iter().enumerate() {
        if idx > 0 {
            output.push('\n');
        }
        output.push_str(entry);
    }
    output.push('\n');
    output.into_bytes()
}
