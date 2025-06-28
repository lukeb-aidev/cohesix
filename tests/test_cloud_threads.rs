// CLASSIFICATION: COMMUNITY
// Filename: test_cloud_threads.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-10-28

use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use tempfile;
use tiny_http::{Response, Server};

use cohesix::cloud::orchestrator::{register_queen, send_heartbeat};

#[test]
fn orchestrator_threads_flow() {
    // skip test if we can't bind to a local port
    if TcpListener::bind("127.0.0.1:0").is_err() {
        eprintln!("skipping test: cannot bind local port");
        return;
    }

    println!("Opening file: {:?}", "127.0.0.1:0");
    let server = Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let srv_logs = logs.clone();
    let server_handle = thread::spawn(move || {
        for _ in 0..5 {
            if let Ok(req) = server.recv() {
                let entry = format!("{} {}", req.method(), req.url());
                srv_logs.lock().unwrap().push(entry);
                let _ = req.respond(Response::empty(200));
            }
        }
    });

    let tmp_dir = tempfile::tempdir().unwrap();
    std::env::set_var("COHESIX_SRV_ROOT", tmp_dir.path());
    println!("Opening file: {:?}", tmp_dir.path().join("cloud"));
    std::fs::create_dir_all(tmp_dir.path().join("cloud")).unwrap();
    let url = format!("http://127.0.0.1:{port}");

    let queen = thread::spawn({
        let url = url.clone();
        move || {
            let id = register_queen(&url).unwrap();
            send_heartbeat(id).unwrap();
        }
    });

    let workers: Vec<_> = (0..2)
        .map(|_| {
            let url = url.clone();
            thread::spawn(move || {
                let _ = ureq::post(&format!("{}/worker_ping", url)).send_string("status=ready");
            })
        })
        .collect();

    queen.join().unwrap();
    for w in workers {
        let _ = w.join();
    }

    std::thread::sleep(std::time::Duration::from_millis(50));
    server_handle.join().unwrap();

    let l = logs.lock().unwrap();
    let text = l.join("\n");
    assert!(text.contains("/register"));
    assert!(text.contains("/heartbeat"));
    let worker_pings = l.iter().filter(|e| e.contains("/worker_ping")).count();
    assert!(worker_pings >= 2);
}
