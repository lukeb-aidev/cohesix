// CLASSIFICATION: COMMUNITY
// Filename: test_cloud_hooks.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-10-27

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use cohesix::cloud::orchestrator::{register_queen, send_heartbeat};

#[test]
fn queen_worker_cloud_flow() {
    if std::net::TcpListener::bind("127.0.0.1:0").is_err() {
        eprintln!("skipping test: cannot bind local port");
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let log_ref = logs.clone();
    thread::spawn(move || {
        for _ in 0..3 {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 512];
                if let Ok(n) = stream.read(&mut buf) {
                    log_ref
                        .lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&buf[..n]).to_string());
                    if buf.starts_with(b"POST /register") {
                        let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nQ123";
                        let _ = stream.write_all(resp);
                    } else {
                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
                    }
                }
            }
        }
        std::io::stdout().flush().unwrap();
    });
    std::fs::create_dir_all("/srv").ok();
    std::env::set_var("CLOUD_HOOK_URL", format!("http://127.0.0.1:{port}"));
    println!("CLOUD_HOOK_URL={}", std::env::var("CLOUD_HOOK_URL").unwrap());
    std::io::stdout().flush().unwrap();
    let id = register_queen(&format!("http://127.0.0.1:{port}")).unwrap();
    send_heartbeat(id).unwrap();
    for _ in 0..20 {
        let log_text = { logs.lock().unwrap().join("\n") };
        if log_text.contains("POST /register")
            || log_text.contains("POST /heartbeat")
            || log_text.contains("status=ready")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let l = logs.lock().unwrap();
    let log_text = l.join("\n");
    assert!(
        log_text.contains("POST /register")
            || log_text.contains("POST /heartbeat")
            || log_text.contains("status=ready"),
        "log output: {}",
        log_text
    );
}
