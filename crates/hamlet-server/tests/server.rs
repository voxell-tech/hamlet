//! The `hamlet-server` executable.

#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use hamlet_server::{Held, Home};
use hamlet_testing::Scratch;

#[test]
fn the_server_holds_its_home_until_terminated() {
    let dir = Scratch::new("server-binary");
    let mut server =
        Command::new(env!("CARGO_BIN_EXE_hamlet-server"))
            .args(["--home".as_ref(), dir.as_os_str()])
            .env_remove("HAMLET_HOME")
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    let mut line = String::new();
    BufReader::new(server.stderr.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains("holding"), "{line}");
    let refused = Home::open(&dir).err().unwrap();
    assert!(refused.downcast_ref::<Held>().is_some(), "{refused}");
    // SAFETY: kill sends a signal to the child's PID and has no
    // preconditions.
    let sent =
        unsafe { libc::kill(server.id() as i32, libc::SIGTERM) };
    assert_eq!(sent, 0);
    assert!(server.wait().unwrap().success());
    let mut home = Home::open(&dir).unwrap();
    let markers = home
        .store()
        .connection()
        .query_row("SELECT count(*) FROM marker", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();
    assert_eq!(markers, 0);
}
