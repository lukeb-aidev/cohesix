// CLASSIFICATION: COMMUNITY
// Filename: test_cohesix_shell.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-01

#[cfg(unix)]
#[test]
fn cohesix_shell_delegates_to_busybox() {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "cohesix-shell-test-{}-{}",
        std::process::id(),
        timestamp
    ));

    fs::create_dir_all(&base).expect("create temp directory");
    let fake_busybox = base.join("cohbox");
    let log_path = base.join("log.txt");

    let mut script = fs::File::create(&fake_busybox).expect("create fake busybox");
    writeln!(
        script,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"",
        log_path.display()
    )
    .expect("write fake busybox script");
    drop(script);

    let mut perms = fs::metadata(&fake_busybox)
        .expect("metadata for fake busybox")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_busybox, perms).expect("set exec perms");

    let status = Command::new(env!("CARGO_BIN_EXE_cohesix-shell"))
        .arg("-c")
        .arg("echo hi")
        .env("COHESIX_BUSYBOX_PATH", &fake_busybox)
        .status()
        .expect("run cohesix-shell");
    assert!(status.success(), "cohesix-shell exit status: {:?}", status);

    let log_contents = fs::read_to_string(&log_path).expect("read fake busybox log");
    let logged: Vec<&str> = log_contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(
        logged,
        ["sh", "-c", "echo hi"],
        "busybox arguments: {:?}",
        logged
    );

    let _ = fs::remove_dir_all(&base);
}

#[cfg(not(unix))]
#[test]
fn cohesix_shell_delegates_to_busybox() {
    // Skip test on non-Unix platforms where BusyBox semantics are not available.
}
