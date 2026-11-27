#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(super) use self::unix::{
    Group, end_now, has_ended, is_executable, limited, own_session,
    reap_with_server, spawn, spawn_bound, standard_input, take_ends,
};
#[cfg(windows)]
pub(super) use self::windows::{
    Group, end_now, has_ended, is_executable, limited,
    reap_with_server, spawn, spawn_bound, standard_input, take_ends,
};
