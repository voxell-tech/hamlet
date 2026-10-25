#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(super) use self::unix::{
    Group, is_executable, limited, reap_with_server, spawn,
    unenforced,
};
#[cfg(windows)]
pub(super) use self::windows::{
    Group, is_executable, limited, reap_with_server, spawn,
    unenforced,
};
