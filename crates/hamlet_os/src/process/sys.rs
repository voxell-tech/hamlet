#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(super) use self::unix::{
    Group, PtyInput, PtyOutput, is_executable, limited,
    reap_with_server, resize_pty, spawn, spawn_pty, unenforced,
};
#[cfg(windows)]
pub(super) use self::windows::{
    Group, PtyInput, PtyOutput, is_executable, limited,
    reap_with_server, resize_pty, spawn, spawn_pty, unenforced,
};
