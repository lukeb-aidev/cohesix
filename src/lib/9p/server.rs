// CLASSIFICATION: COMMUNITY
// Filename: server.rs v1.2
// Author: Lukas Bower
// Date Modified: 2026-02-20

//! 9P file server implementation for Cohesix.
//! Handles incoming 9P requests and routes them to appropriate virtual filesystem backends.

use super::protocol::{parse_message, serialize_message, P9Message};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::fs;
use std::sync::Mutex;

#[derive(Default)]
struct Node {
    data: Vec<u8>,
}

static FS: Lazy<Mutex<HashMap<String, Node>>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert("/".into(), Node { data: Vec::new() });
    for d in ["/srv", "/mnt", "/history"].iter() {
        map.insert((*d).into(), Node { data: Vec::new() });
    }
    Mutex::new(map)
});

/// Reset the in-memory FS for isolated tests.
pub fn reset_fs() {
    let mut map = FS.lock().unwrap();
    map.clear();
    map.insert("/".into(), Node::default());
    for d in ["/srv", "/mnt", "/history"].iter() {
        map.insert((*d).into(), Node::default());
    }
}

fn cohrole() -> String {
    let path = std::env::var("COHROLE_PATH").unwrap_or_else(|_| "/srv/cohrole".into());
    fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("[9P] failed to read cohrole {}: {}", path, e);
        "Unknown".into()
    })
}

fn can_write(role: &str, path: &str) -> bool {
    match role {
        "QueenPrimary" => true,
        "DroneWorker" => !path.starts_with("/history"),
        _ => {
            !path.starts_with("/history") && !path.starts_with("/srv")
        }
    }
}

fn split_path_data(rest: &str) -> (&str, &str) {
    if let Some(idx) = rest.find('|') {
        let (p, d) = rest.split_at(idx);
        (p, &d[1..])
    } else {
        (rest, "")
    }
}

/// Basic handler for a 9P server session used by unit tests.
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
            if fs.contains_key(rest) {
                P9Message::Rwalk
            } else {
                P9Message::Unknown(0xfe)
            }
        }
        P9Message::Tstat => {
            if fs.contains_key(rest) {
                P9Message::Rstat
            } else {
                P9Message::Unknown(0xfe)
            }
        }
        P9Message::Topen => {
            if fs.contains_key(rest) {
                P9Message::Ropen
            } else {
                P9Message::Unknown(0xfe)
            }
        }
        P9Message::Tread => {
            if fs.contains_key(rest) {
                P9Message::Rread
            } else {
                P9Message::Unknown(0xfe)
            }
        }
        P9Message::Twrite => {
            let (path, data) = split_path_data(rest);
            if can_write(&role, path) {
                fs.entry(path.to_string())
                    .or_default()
                    .data
                    .extend_from_slice(data.as_bytes());
                P9Message::Rwrite
            } else {
                P9Message::Unknown(0xfd)
            }
        }
        P9Message::Tclunk => P9Message::Rclunk,
        _ => P9Message::Unknown(0xff),
    };

    let mut out = serialize_message(&response);
    if let P9Message::Rread = response {
        if let Some(node) = fs.get(rest) {
            out.extend_from_slice(&node.data);
        }
    }
    out
}
