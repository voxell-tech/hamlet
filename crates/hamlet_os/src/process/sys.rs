#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(super) use self::unix::{
    Group, end_now, end_with_parent, is_executable, limited,
    reap_with_server, spawn, take_stdio, unenforced,
};
#[cfg(windows)]
pub(super) use self::windows::{
    Group, end_now, end_with_parent, is_executable, limited,
    reap_with_server, spawn, take_stdio, unenforced,
};
