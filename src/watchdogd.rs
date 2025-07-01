// CLASSIFICATION: COMMUNITY
// Filename: watchdogd.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-12

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

    fn stale(path: &str, max_age: Duration) -> bool {
        fs::metadata(path)
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().unwrap_or(Duration::from_secs(0)) > max_age)
            .unwrap_or(true)
    }

    /// Check monitored files and trigger restarts as needed.
    pub fn check(&mut self) {
        if Self::stale(&self.heartbeat_path, Duration::from_secs(300)) {
            self.restart("worker_agent");
        }
        if Self::stale(&self.tasks_path, Duration::from_secs(120)) {
            self.restart("orchestratord");
        }
        if Self::stale(&self.trace_path, Duration::from_secs(120)) {
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
        let _ = Command::new("systemctl")
            .arg("restart")
            .arg(svc)
            .status();
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
