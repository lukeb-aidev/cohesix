// CLASSIFICATION: COMMUNITY
// Filename: 9p_server.rs v0.4
// Author: Lukas Bower
// Date Modified: 2028-12-31

use cohesix_9p::{FsConfig, FsServer};
use ninep::client::TcpClient;
use serial_test::serial;
use std::io;
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

fn next_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral")
        .local_addr()
        .expect("local addr")
        .port()
}

fn start_server(root: &PathBuf, port: u16) -> io::Result<FsServer> {
    let mut srv = FsServer::new(FsConfig {
        root: root.to_string_lossy().to_string(),
        port,
        readonly: false,
    });
    srv.start().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    // allow thread to start accepting
    thread::sleep(Duration::from_millis(100));
    Ok(srv)
}

fn client_for(port: u16, user: &str) -> io::Result<TcpClient> {
    TcpClient::new_tcp(user.to_string(), format!("127.0.0.1:{port}"), "/")
}

#[test]
#[serial]
fn walk_srv() -> io::Result<()> {
    let tmp = tempdir()?;
    std::fs::create_dir_all(tmp.path().join("srv"))?;
    let port = next_port();
    let _srv = start_server(&tmp.path().to_path_buf(), port)?;
    let mut client = client_for(port, "QueenPrimary")?;
    client.walk("/srv".to_string())?;
    Ok(())
}

#[test]
#[serial]
fn worker_write_allowed_proc() -> io::Result<()> {
    let tmp = tempdir()?;
    std::fs::create_dir_all(tmp.path().join("proc"))?;
    let port = next_port();
    let _srv = start_server(&tmp.path().to_path_buf(), port)?;
    let mut client = client_for(port, "DroneWorker")?;
    client.write("/proc/x".to_string(), 0, b"data")?;
    let disk_path = tmp.path().join("proc").join("x");
    let content = std::fs::read_to_string(disk_path)?;
    assert_eq!(content, "data");
    Ok(())
}

#[test]
#[serial]
fn queen_write_and_read() -> io::Result<()> {
    let tmp = tempdir()?;
    std::fs::create_dir_all(tmp.path().join("mnt"))?;
    let port = next_port();
    let _srv = start_server(&tmp.path().to_path_buf(), port)?;
    let mut client = client_for(port, "QueenPrimary")?;
    client.write("/mnt/data".to_string(), 0, b"hello")?;
    let data = client.read("/mnt/data".to_string())?;
    assert_eq!(data, b"hello");
    Ok(())
}

#[test]
#[serial]
fn cross_role_read_access() -> io::Result<()> {
    let tmp = tempdir()?;
    std::fs::create_dir_all(tmp.path().join("srv"))?;
    let port = next_port();
    let _srv = start_server(&tmp.path().to_path_buf(), port)?;
    let mut queen = client_for(port, "QueenPrimary")?;
    queen.write("/srv/shared".to_string(), 0, b"data")?;
    drop(queen);
    let mut kiosk = client_for(port, "KioskInteractive")?;
    let data = kiosk.read("/srv/shared".to_string())?;
    assert_eq!(data, b"data");
    Ok(())
}

#[test]
#[serial]
fn kiosk_write_denied_srv() -> io::Result<()> {
    let tmp = tempdir()?;
    std::fs::create_dir_all(tmp.path().join("srv"))?;
    let port = next_port();
    let _srv = start_server(&tmp.path().to_path_buf(), port)?;
    let mut kiosk = client_for(port, "KioskInteractive")?;
    let err = kiosk.write("/srv/blocked".to_string(), 0, b"x").unwrap_err();
    assert!(err.kind() == io::ErrorKind::Other || err.kind() == io::ErrorKind::PermissionDenied);
    assert!(!tmp.path().join("srv/blocked").exists());
    Ok(())
}

