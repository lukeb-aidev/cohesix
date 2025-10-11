// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::Role;
use nine_door::NineDoor;
use secure9p_wire::{OpenMode, MAX_MSIZE};

#[test]
fn attach_walk_read_and_write() {
    let server = NineDoor::new();
    let mut client = server.connect().expect("create session");
    client.version(MAX_MSIZE).expect("version handshake");
    client.attach(1, Role::Queen).expect("attach");

    let proc_path = vec!["proc".to_owned(), "boot".to_owned()];
    client.walk(1, 2, &proc_path).expect("walk /proc/boot");
    client
        .open(2, OpenMode::read_only())
        .expect("open /proc/boot");
    let data = client.read(2, 0, MAX_MSIZE).expect("read /proc/boot");
    let text = String::from_utf8(data).expect("utf8");
    assert!(text.contains("Cohesix boot"));

    let queen_ctl = vec!["queen".to_owned(), "ctl".to_owned()];
    client.walk(1, 3, &queen_ctl).expect("walk /queen/ctl");
    client
        .open(3, OpenMode::write_append())
        .expect("open /queen/ctl for append");
    let payload = b"{\"spawn\":\"heartbeat\"}\n";
    let written = client.write(3, payload).expect("write /queen/ctl");
    assert_eq!(written as usize, payload.len());
}
