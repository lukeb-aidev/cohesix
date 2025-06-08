// CLASSIFICATION: COMMUNITY
// Filename: server.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! 9P file server implementation for Cohesix.
//! Handles incoming 9P requests and routes them to appropriate virtual filesystem backends.

use super::protocol::{P9Message, parse_message, serialize_message};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::fs;

#[derive(Default)]
struct Node {
    data: Vec<u8>,
    is_dir: bool,
}

static FS: Lazy<Mutex<HashMap<String, Node>>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert("/".into(), Node { data: Vec::new(), is_dir: true });
    for d in ["/srv", "/proc", "/mnt", "/history"].iter() {
        map.insert((*d).into(), Node { data: Vec::new(), is_dir: true });
    }
    Mutex::new(map)
});

fn cohrole() -> String {
    fs::read_to_string("/srv/cohrole").unwrap_or_else(|_| "Unknown".into())
}

fn can_write(role: &str, path: &str) -> bool {
    match role {
        "QueenPrimary" => true,
        "DroneWorker" => !(path.starts_with("/proc") || path.starts_with("/history")),
        _ => !path.starts_with("/proc") && !path.starts_with("/history") && !path.starts_with("/srv"),
    }
}

/// Stub handler for a 9P server session.
pub fn handle_9p_session(stream: &[u8]) -> Vec<u8> {
    let request = parse_message(stream);
    let rest = std::str::from_utf8(&stream[1..]).unwrap_or("");
    println!("[9P] Received message: {:?} path={}", request, rest);

    let role = cohrole();
    let mut fs = FS.lock().unwrap();

    let response = match request {
        P9Message::Tversion => P9Message::Rversion,
        P9Message::Tattach => P9Message::Rattach,
        P9Message::Twalk => {
            if fs.contains_key(rest) { P9Message::Rwalk } else { P9Message::Unknown(0xfe) }
        }
        P9Message::Tstat => {
            if fs.contains_key(rest) { P9Message::Rstat } else { P9Message::Unknown(0xfe) }
        }
        P9Message::Topen => {
            if fs.contains_key(rest) { P9Message::Ropen } else { P9Message::Unknown(0xfe) }
        }
        P9Message::Tread => {
            if fs.contains_key(rest) { P9Message::Rread } else { P9Message::Unknown(0xfe) }
        }
        P9Message::Twrite => {
            if can_write(&role, rest) {
                fs.entry(rest.to_string()).or_default().data.extend_from_slice(b"stub");
                P9Message::Rwrite
            } else {
                P9Message::Unknown(0xfd)
            }
        }
        P9Message::Tclunk => {
            P9Message::Rclunk
        }
        _ => P9Message::Unknown(0xff),
    };

    serialize_message(&response)
}
