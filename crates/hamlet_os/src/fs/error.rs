use std::io::{self, ErrorKind};

#[cfg(unix)]
use rustix::io::Errno;
#[cfg(windows)]
use windows_sys::Win32::Foundation::ERROR_CANT_RESOLVE_FILENAME;

use super::BadName;

/// A system error's name and words, as Node gives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemError {
    pub name: &'static str,
    pub words: &'static str,
}

impl SystemError {
    /// A path through more symbolic links than the system follows.
    pub const LOOP: Self = Self {
        name: "ELOOP",
        words: "too many symbolic links encountered",
    };
}

struct Row {
    error: SystemError,
    kind: Option<ErrorKind>,
    #[cfg(unix)]
    errno: Errno,
}

const fn named(
    name: &'static str,
    words: &'static str,
) -> SystemError {
    SystemError { name, words }
}

macro_rules! rows {
    ($($error:expr, $kind:expr, $errno:ident;)*) => {
        [$(Row {
            error: $error,
            kind: $kind,
            #[cfg(unix)]
            errno: Errno::$errno,
        }),*]
    };
}

const ROWS: [Row; 20] = rows! {
    named("ENOENT", "no such file or directory"), Some(ErrorKind::NotFound), NOENT;
    named("EACCES", "permission denied"), Some(ErrorKind::PermissionDenied), ACCESS;
    named("EPERM", "operation not permitted"), None, PERM;
    named("EISDIR", "illegal operation on a directory"), Some(ErrorKind::IsADirectory), ISDIR;
    named("ENOTDIR", "not a directory"), Some(ErrorKind::NotADirectory), NOTDIR;
    named("EEXIST", "file already exists"), Some(ErrorKind::AlreadyExists), EXIST;
    SystemError::LOOP, None, LOOP;
    named("ENAMETOOLONG", "name too long"), Some(ErrorKind::InvalidFilename), NAMETOOLONG;
    named("EROFS", "read-only file system"), Some(ErrorKind::ReadOnlyFilesystem), ROFS;
    named("ENOSPC", "no space left on device"), Some(ErrorKind::StorageFull), NOSPC;
    named("EINVAL", "invalid argument"), Some(ErrorKind::InvalidInput), INVAL;
    named("EBUSY", "resource busy or locked"), Some(ErrorKind::ResourceBusy), BUSY;
    named("EMFILE", "too many open files"), None, MFILE;
    named("ETXTBSY", "text file is busy"), Some(ErrorKind::ExecutableFileBusy), TXTBSY;
    named("EIO", "i/o error"), None, IO;
    named("EFBIG", "file too large"), Some(ErrorKind::FileTooLarge), FBIG;
    named("EAGAIN", "resource temporarily unavailable"), Some(ErrorKind::WouldBlock), AGAIN;
    named("ENXIO", "no such device or address"), None, NXIO;
    named("ENOTEMPTY", "directory not empty"), Some(ErrorKind::DirectoryNotEmpty), NOTEMPTY;
    named("EXDEV", "cross-device link not permitted"), Some(ErrorKind::CrossesDevices), XDEV;
};

const STAND_INS: [ErrorKind; 2] =
    [ErrorKind::NotFound, ErrorKind::IsADirectory];

/// `error` in pi's tools' words, found by the system's own code. An
/// error with no code has no name, unless this crate made it in place
/// of a system's.
pub fn system_error(error: &io::Error) -> Option<SystemError> {
    #[cfg(unix)]
    if let Some(errno) = Errno::from_io_error(error) {
        return ROWS
            .iter()
            .find(|row| row.errno == errno)
            .map(|row| row.error);
    }
    // The refusal of a link at a name, as `O_NOFOLLOW`'s on Unix.
    #[cfg(windows)]
    if error.raw_os_error()
        == Some(ERROR_CANT_RESOLVE_FILENAME.cast_signed())
    {
        return Some(SystemError::LOOP);
    }
    let stand_in = STAND_INS.contains(&error.kind())
        || matches!(error.get_ref(), Some(inner) if inner.is::<BadName>());
    if error.raw_os_error().is_none() && !stand_in {
        return None;
    }
    ROWS.iter()
        .find(|row| row.kind == Some(error.kind()))
        .map(|row| row.error)
}
