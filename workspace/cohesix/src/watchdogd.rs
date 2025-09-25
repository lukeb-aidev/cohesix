// CLASSIFICATION: COMMUNITY
// Filename: watchdogd.rs v0.2
// Author: Lukas Bower
// Date Modified: 2029-10-27

/// Background watchdog daemon monitoring worker health.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::Command;
use std::time::{Duration, SystemTime};

use chrono::Utc;

/// System watchdog daemon.
pub struct WatchdogDaemon {
    heartbeat_path: String,
    tasks_path: String,
    trace_path: String,
    backoff: Duration,
    last_restart: Option<SystemTime>,
}

impl Default for WatchdogDaemon {
    fn default() -> Self {
        Self::new()
    }
}

impl WatchdogDaemon {
    /// Create a new watchdog daemon with default paths.
    pub fn new() -> Self {
        Self {
            heartbeat_path: "/srv/worker/heartbeat".into(),
            tasks_path: "/srv/tasks/status".into(),
            trace_path: "/srv/trace/live.log".into(),
            backoff: Duration::from_secs(60),
            last_restart: None,
        }
    }

    /// Start monitoring in a blocking loop.
    pub fn run(&mut self) {
        loop {
            self.check();
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    fn is_stale(&self, path: &str, max_age: Duration) -> bool {
        let now = SystemTime::now();
        match fs::metadata(path) {
            Ok(metadata) => match metadata.modified() {
                Ok(modified) => match now.duration_since(modified) {
                    Ok(age) => age > max_age,
                    Err(err) => {
                        self.log(&format!("clock skew when reading {path}: {err}"));
                        false
                    }
                },
                Err(err) => {
                    self.log(&format!("failed to read mtime for {path}: {err}"));
                    true
                }
            },
            Err(err) => {
                self.log(&format!("missing or unreadable {path}: {err}"));
                true
            }
        }
    }

    /// Check monitored files and trigger restarts as needed.
    pub fn check(&mut self) {
        if self.is_stale(&self.heartbeat_path, Duration::from_secs(300)) {
            self.restart("worker_agent");
        }
        if self.is_stale(&self.tasks_path, Duration::from_secs(120)) {
            self.restart("orchestratord");
        }
        if self.is_stale(&self.trace_path, Duration::from_secs(120)) {
            self.restart("trace_loop");
        }
    }

    fn restart(&mut self, svc: &str) {
        let now = SystemTime::now();
        if self
            .last_restart
            .map(|t| now.duration_since(t).unwrap_or_default() < self.backoff)
            .unwrap_or(false)
        {
            self.log(&format!("backoff active, skipping restart for {svc}"));
            return;
        }
        self.log(&format!("restarting {svc}"));
        match Command::new("systemctl").arg("restart").arg(svc).status() {
            Ok(status) if status.success() => {
                self.log(&format!("restart of {svc} completed successfully"));
            }
            Ok(status) => {
                self.log(&format!("restart of {svc} failed: {status}"));
            }
            Err(err) => {
                self.log(&format!("failed to invoke systemctl for {svc}: {err}"));
            }
        }
        self.last_restart = Some(now);
    }

    fn log(&self, msg: &str) {
        fs::create_dir_all("/srv/logs").ok();
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/srv/logs/watchdog.log")
        {
            let _ = writeln!(f, "[{}] {}", Utc::now().to_rfc3339(), msg);
        }
    }
}
