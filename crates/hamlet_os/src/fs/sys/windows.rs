//! Windows through the NT API. Each name is opened by one
//! `NtCreateFile` relative to a held directory, with
//! `OBJ_DONT_REPARSE` and `FILE_OPEN_REPARSE_POINT`, so no reparse
//! point is followed at the name or before it, and every check reads
//! the opened handle, as std's own `remove_dir_all` does.

#![expect(unsafe_code, reason = "the NT and Win32 file calls")]

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::mem::offset_of;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::{
    AsRawHandle, FromRawHandle as _, HandleOrInvalid, OwnedHandle,
};
use std::path::Path;
use std::{io, ptr};

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_CREATE, FILE_DIRECTORY_FILE, FILE_INFORMATION_CLASS,
    FILE_OPEN, FILE_OPEN_IF, FILE_OPEN_REPARSE_POINT,
    FILE_RENAME_IGNORE_READONLY_ATTRIBUTE, FILE_RENAME_INFORMATION,
    FILE_RENAME_INFORMATION_0, FILE_RENAME_POSIX_SEMANTICS,
    FILE_RENAME_REPLACE_IF_EXISTS, FILE_SYNCHRONOUS_IO_NONALERT,
    FileRenameInformation, FileRenameInformationEx, NtCreateFile,
    NtSetInformationFile, REPARSE_DATA_BUFFER,
    REPARSE_DATA_BUFFER_0_0, REPARSE_DATA_BUFFER_0_1,
    SYMLINK_FLAG_RELATIVE,
};
use windows_sys::Win32::Foundation::{
    ERROR_CANT_RESOLVE_FILENAME, ERROR_DIRECTORY,
    ERROR_DIRECTORY_NOT_SUPPORTED, ERROR_INVALID_FUNCTION,
    ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES,
    ERROR_NOT_SUPPORTED, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE,
    RtlNtStatusToDosError, STATUS_REPARSE_POINT_ENCOUNTERED,
    UNICODE_STRING,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_APPEND_DATA,
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
    FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO, FILE_BASIC_INFO,
    FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO,
    FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_FULL_DIR_INFO,
    FILE_GENERIC_READ, FILE_INFO_BY_HANDLE_CLASS,
    FILE_LIST_DIRECTORY, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_TRAVERSE, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA,
    FileAttributeTagInfo, FileBasicInfo, FileDispositionInfo,
    FileDispositionInfoEx, FileFullDirectoryInfo,
    GetFileInformationByHandle, GetFileInformationByHandleEx,
    GetFinalPathNameByHandleW, MAXIMUM_REPARSE_DATA_BUFFER_SIZE,
    ReOpenFile, SYNCHRONIZE, SetFileInformationByHandle,
    VOLUME_NAME_DOS,
};
use windows_sys::Win32::System::IO::{
    DeviceIoControl, IO_STATUS_BLOCK,
};
use windows_sys::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;
use windows_sys::Win32::System::SystemServices::{
    IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK,
};

use super::System;
use crate::fs::{
    Exists, FileId, Kind, LastLink, Mode, Name, Open, Removing,
    Replace, Stat, path,
};

pub(crate) struct Windows;

/// Every handle lets others read, write and delete, as a Unix
/// descriptor stops nobody.
const SHARE_ALL: u32 =
    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// What a held directory may do: list its names and open them.
const DIRECTORY: u32 = FILE_LIST_DIRECTORY | FILE_TRAVERSE;

/// The tag bit of a reparse point that names another file, as a
/// symbolic link's and a junction's do.
const NAME_SURROGATE: u32 = 0x2000_0000;

/// Where a reparse point's data starts, past its tag and length.
const REPARSE_DATA: usize =
    offset_of!(REPARSE_DATA_BUFFER, Anonymous);

impl System for Windows {
    type Handle = OwnedHandle;

    fn open_path(
        path: &Path,
        last: LastLink,
    ) -> io::Result<OwnedHandle> {
        let link = match last {
            LastLink::Follow => 0,
            LastLink::Refuse => FILE_FLAG_OPEN_REPARSE_POINT,
        };
        let dir = OpenOptions::new()
            .access_mode(
                DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            )
            .share_mode(SHARE_ALL)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | link)
            .open(path)?;
        directory(dir.into())
    }

    fn lstat(path: &Path) -> io::Result<Stat> {
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(SHARE_ALL)
            .custom_flags(
                FILE_FLAG_BACKUP_SEMANTICS
                    | FILE_FLAG_OPEN_REPARSE_POINT,
            )
            .open(path)?;
        stat_of(&file)
    }

    fn duplicate(dir: &OwnedHandle) -> io::Result<OwnedHandle> {
        dir.try_clone()
    }

    fn status(dir: &OwnedHandle) -> io::Result<Stat> {
        stat_of(dir)
    }

    fn stat_at(dir: &OwnedHandle, name: &Name) -> io::Result<Stat> {
        stat_of(&open_at(dir, name, 0, FILE_OPEN, 0, 0)?)
    }

    fn open_directory_at(
        dir: &OwnedHandle,
        name: &Name,
    ) -> io::Result<OwnedHandle> {
        directory(open_at(
            dir,
            name,
            DIRECTORY,
            FILE_OPEN,
            FILE_DIRECTORY_FILE,
            0,
        )?)
    }

    /// A folder keeps no mode here.
    fn make_directory_at(
        dir: &OwnedHandle,
        name: &Name,
        _: Mode,
        exists: Exists,
    ) -> io::Result<OwnedHandle> {
        let disposition = match exists {
            Exists::Fail => FILE_CREATE,
            Exists::Open => FILE_OPEN_IF,
        };
        directory(open_at(
            dir,
            name,
            DIRECTORY,
            disposition,
            FILE_DIRECTORY_FILE,
            FILE_ATTRIBUTE_NORMAL,
        )?)
    }

    fn open_file_at(
        dir: &OwnedHandle,
        name: &Name,
        how: Open,
    ) -> io::Result<(File, Stat)> {
        let (access, disposition, mode) = match how {
            Open::Read => (FILE_GENERIC_READ, FILE_OPEN, None),
            Open::Append => (FILE_APPEND_DATA, FILE_OPEN, None),
            Open::New(mode) => {
                (FILE_WRITE_DATA, FILE_CREATE, Some(mode))
            }
            Open::Truncate(mode) => {
                (FILE_WRITE_DATA, FILE_OPEN_IF, Some(mode))
            }
            Open::Extend(mode) => {
                (FILE_APPEND_DATA, FILE_OPEN_IF, Some(mode))
            }
        };
        let attributes = match mode {
            Some(mode)
                if mode.kept(Kind::File) == Mode::READ_ONLY =>
            {
                FILE_ATTRIBUTE_READONLY
            }
            _ => FILE_ATTRIBUTE_NORMAL,
        };
        let handle =
            open_at(dir, name, access, disposition, 0, attributes)?;
        let mut stat = stat_of(&handle)?;
        let file = File::from(handle);
        // Emptied only once it is known to be a regular file, never a
        // link opened as itself.
        if matches!(how, Open::Truncate(_)) && stat.kind == Kind::File
        {
            file.set_len(0)?;
            stat.size = 0;
        }
        Ok((file, stat))
    }

    fn read_link_at(
        dir: &OwnedHandle,
        name: &Name,
    ) -> io::Result<String> {
        let link = open_at(dir, name, 0, FILE_OPEN, 0, 0)?;
        let mut data =
            vec![0_u8; MAXIMUM_REPARSE_DATA_BUFFER_SIZE as usize];
        let mut written = 0;
        // SAFETY: the call writes at most `data.len()` bytes into
        // `data` and their count into `written`. The handle is
        // synchronous, so it takes no overlapped structure.
        let done = unsafe {
            DeviceIoControl(
                link.as_raw_handle(),
                FSCTL_GET_REPARSE_POINT,
                ptr::null(),
                0,
                data.as_mut_ptr().cast(),
                data.len() as u32,
                &mut written,
                ptr::null_mut(),
            )
        };
        if done == 0 {
            return Err(io::Error::last_os_error());
        }
        data.truncate(written as usize);
        target(&data).ok_or_else(|| io::ErrorKind::InvalidData.into())
    }

    /// A folder keeps no mode here.
    fn set_mode(_: &OwnedHandle, _: Mode) -> io::Result<()> {
        Ok(())
    }

    /// Opened again for its attributes, since `file` may be open only
    /// to read.
    fn set_file_mode(file: &File, mode: Mode) -> io::Result<()> {
        let file = reopen(
            file,
            FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
        )?;
        let basic: FILE_BASIC_INFO = query(&file, FileBasicInfo)?;
        let others = basic.FileAttributes & !FILE_ATTRIBUTE_READONLY;
        // No attribute at all would leave the attributes unchanged.
        let attributes = if mode.kept(Kind::File) == Mode::READ_ONLY {
            others | FILE_ATTRIBUTE_READONLY
        } else if others == 0 {
            FILE_ATTRIBUTE_NORMAL
        } else {
            others
        };
        // Times of zero are left unchanged.
        let basic = FILE_BASIC_INFO {
            FileAttributes: attributes,
            ..FILE_BASIC_INFO::default()
        };
        set(&file, FileBasicInfo, &basic)
    }

    fn rename_at(
        from: &OwnedHandle,
        name: &Name,
        to: &OwnedHandle,
        to_name: &Name,
        replace: Replace,
    ) -> io::Result<()> {
        let file = open_at(from, name, DELETE, FILE_OPEN, 0, 0)?;
        let rename =
            |class, how| rename_to(&file, to, to_name, class, how);
        // `Flags` sets every byte of the union, where the one-byte
        // `ReplaceIfExists` would leave three unset. A read-only file
        // is replaced, as on Unix.
        let replacing = match replace {
            Replace::Never => 0,
            Replace::Always => FILE_RENAME_REPLACE_IF_EXISTS,
        };
        let posix = FILE_RENAME_INFORMATION_0 {
            Flags: replacing
                | FILE_RENAME_POSIX_SEMANTICS
                | FILE_RENAME_IGNORE_READONLY_ATTRIBUTE,
        };
        match rename(FileRenameInformationEx, posix) {
            Err(error) if unsupported(&error) => rename(
                FileRenameInformation,
                FILE_RENAME_INFORMATION_0 { Flags: replacing },
            ),
            renamed => renamed,
        }
    }

    /// Through the handle whose kind was checked, so nothing swapped
    /// in at the name is removed in its place.
    fn remove_at(
        dir: &OwnedHandle,
        name: &Name,
        removing: Removing,
    ) -> io::Result<()> {
        let file = open_at(dir, name, DELETE, FILE_OPEN, 0, 0)?;
        match (removing, kind_of(&file)?) {
            (Removing::File, Kind::Directory) => {
                return Err(code(ERROR_DIRECTORY_NOT_SUPPORTED));
            }
            (
                Removing::Directory,
                Kind::File | Kind::Link | Kind::Special,
            ) => return Err(code(ERROR_DIRECTORY)),
            _ => {}
        }
        let posix = FILE_DISPOSITION_INFO_EX {
            Flags: FILE_DISPOSITION_FLAG_DELETE
                | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
                | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
        };
        match set(&file, FileDispositionInfoEx, &posix) {
            Err(error) if unsupported(&error) => set(
                &file,
                FileDispositionInfo,
                &FILE_DISPOSITION_INFO { DeleteFile: true },
            ),
            removed => removed,
        }
    }

    /// Read through a handle of its own, so that no other copy of
    /// `dir` shares where the listing is.
    fn names(
        dir: &OwnedHandle,
    ) -> io::Result<impl Iterator<Item = io::Result<Name>>> {
        // The folder itself, by an empty name: `ReOpenFile` refuses
        // a folder.
        let listing = open_wide(
            dir,
            Vec::new(),
            FILE_LIST_DIRECTORY,
            FILE_OPEN,
            FILE_DIRECTORY_FILE,
            0,
        )?;
        // Eight-byte words, as each entry is aligned.
        let mut words = vec![0_u64; 8192];
        let mut names = Vec::new();
        loop {
            // SAFETY: the call writes at most the buffer's size in
            // bytes into it.
            let done = unsafe {
                GetFileInformationByHandleEx(
                    listing.as_raw_handle(),
                    FileFullDirectoryInfo,
                    words.as_mut_ptr().cast(),
                    (words.len() * 8) as u32,
                )
            };
            if done == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error()
                    == Some(raw(ERROR_NO_MORE_FILES))
                {
                    break;
                }
                return Err(error);
            }
            let bytes = words
                .iter()
                .flat_map(|word| word.to_ne_bytes())
                .collect::<Vec<_>>();
            listed(&bytes, &mut names).ok_or_else(|| {
                io::Error::from(io::ErrorKind::InvalidData)
            })?;
        }
        Ok(names.into_iter().map(Ok))
    }

    /// NTFS journals its metadata, and a folder has nothing to flush.
    fn sync_directory(_: &OwnedHandle) -> io::Result<()> {
        Ok(())
    }

    /// The path of the very handle held, so a folder renamed on the
    /// way reads as moved.
    fn real_of(dir: &OwnedHandle, _: &str) -> io::Result<String> {
        let mut wide = vec![0_u16; 512];
        loop {
            // SAFETY: the call writes at most `wide.len()` units into
            // `wide`.
            let len = unsafe {
                GetFinalPathNameByHandleW(
                    dir.as_raw_handle(),
                    wide.as_mut_ptr(),
                    wide.len() as u32,
                    FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
                )
            } as usize;
            if len == 0 {
                return Err(io::Error::last_os_error());
            }
            if len < wide.len() {
                let real = OsString::from_wide(&wide[..len]);
                return Ok(path::text(Path::new(&real)));
            }
            wide.resize(len, 0);
        }
    }
}

/// `name` in `dir`, opened by one `NtCreateFile` that follows no
/// reparse point: `name` opens as itself, and `OBJ_DONT_REPARSE`
/// refuses one on the way.
fn open_at(
    dir: &OwnedHandle,
    name: &Name,
    access: u32,
    disposition: u32,
    options: u32,
    attributes: u32,
) -> io::Result<OwnedHandle> {
    let wide = name.as_os().encode_wide().collect();
    open_wide(dir, wide, access, disposition, options, attributes)
}

/// [`open_at`] for the name `wide`, where an empty one opens `dir`
/// itself.
fn open_wide(
    dir: &OwnedHandle,
    mut wide: Vec<u16>,
    access: u32,
    disposition: u32,
    options: u32,
    attributes: u32,
) -> io::Result<OwnedHandle> {
    let bytes = u16::try_from(wide.len() * 2).map_err(|_| {
        io::Error::from(io::ErrorKind::InvalidFilename)
    })?;
    let text = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: wide.as_mut_ptr(),
    };
    let object = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: dir.as_raw_handle(),
        ObjectName: &text,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        SecurityDescriptor: ptr::null(),
        SecurityQualityOfService: ptr::null(),
    };
    let mut handle = ptr::null_mut();
    let mut done = IO_STATUS_BLOCK::default();
    // SAFETY: `object` points at `text`, which points at `wide` with
    // its length in bytes, and all three outlive the call. The call
    // writes only `handle` and `done`.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &object,
            &mut done,
            ptr::null(),
            attributes,
            SHARE_ALL,
            disposition,
            options
                | FILE_OPEN_REPARSE_POINT
                | FILE_SYNCHRONOUS_IO_NONALERT,
            ptr::null(),
            0,
        )
    };
    if status == STATUS_REPARSE_POINT_ENCOUNTERED {
        return Err(code(ERROR_CANT_RESOLVE_FILENAME));
    }
    if status < 0 {
        // SAFETY: the call reads only `status`.
        return Err(code(unsafe { RtlNtStatusToDosError(status) }));
    }
    // SAFETY: the call succeeded, so `handle` is a new handle that
    // nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// `handle`, which must be a directory and no reparse point. A link
/// is refused as `O_NOFOLLOW` refuses it, as a loop.
fn directory(handle: OwnedHandle) -> io::Result<OwnedHandle> {
    match kind_of(&handle)? {
        Kind::Directory => Ok(handle),
        Kind::Link => Err(code(ERROR_CANT_RESOLVE_FILENAME)),
        Kind::File | Kind::Special => Err(code(ERROR_DIRECTORY)),
    }
}

/// What `handle` is, a reparse point itself rather than what it
/// names. A reparse point that names no other file, such as a cloud
/// file's, is `Special`.
fn kind_of(handle: &impl AsRawHandle) -> io::Result<Kind> {
    let info: FILE_ATTRIBUTE_TAG_INFO =
        query(handle, FileAttributeTagInfo)?;
    Ok(if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        if info.ReparseTag & NAME_SURROGATE == 0 {
            Kind::Special
        } else {
            Kind::Link
        }
    } else if info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        Kind::Directory
    } else {
        Kind::File
    })
}

fn stat_of(handle: &impl AsRawHandle) -> io::Result<Stat> {
    let kind = kind_of(handle)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the call writes only `info`.
    let done = unsafe {
        GetFileInformationByHandle(handle.as_raw_handle(), &mut info)
    };
    if done == 0 {
        return Err(io::Error::last_os_error());
    }
    let read_only =
        info.dwFileAttributes & FILE_ATTRIBUTE_READONLY != 0;
    let mode = if read_only {
        Mode::READ_ONLY
    } else {
        Mode::SHARED
    };
    Ok(Stat {
        kind,
        size: u64::from(info.nFileSizeHigh) << 32
            | u64::from(info.nFileSizeLow),
        mode: mode.kept(kind),
        links: info.nNumberOfLinks.into(),
        id: FileId {
            volume: info.dwVolumeSerialNumber.into(),
            index: u64::from(info.nFileIndexHigh) << 32
                | u64::from(info.nFileIndexLow),
        },
    })
}

/// The information `class` of `handle`, which is a `T`.
fn query<T: Default>(
    handle: &impl AsRawHandle,
    class: FILE_INFO_BY_HANDLE_CLASS,
) -> io::Result<T> {
    let mut info = T::default();
    // SAFETY: the call writes at most `size_of::<T>()` bytes into
    // `info`, and each class asked for here fills a plain struct of
    // that size.
    let done = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle(),
            class,
            (&raw mut info).cast(),
            size_of::<T>() as u32,
        )
    };
    if done == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}

/// Sets the information `class` of `handle` to `info`.
fn set<T: ?Sized>(
    handle: &impl AsRawHandle,
    class: FILE_INFO_BY_HANDLE_CLASS,
    info: &T,
) -> io::Result<()> {
    // SAFETY: the call reads `size_of_val(info)` bytes of `info`.
    let done = unsafe {
        SetFileInformationByHandle(
            handle.as_raw_handle(),
            class,
            ptr::from_ref(info).cast(),
            size_of_val(info) as u32,
        )
    };
    if done == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// `handle`'s file opened again, as a handle of its own.
fn reopen(
    handle: &impl AsRawHandle,
    access: u32,
) -> io::Result<OwnedHandle> {
    // SAFETY: `ReOpenFile` returns a new handle that nothing else
    // owns, or `INVALID_HANDLE_VALUE`, which `HandleOrInvalid` holds
    // as it is.
    let handle = unsafe {
        HandleOrInvalid::from_raw_handle(ReOpenFile(
            handle.as_raw_handle(),
            access,
            SHARE_ALL,
            // A link set on the file since it was opened is not
            // followed.
            FILE_FLAG_OPEN_REPARSE_POINT,
        ))
    };
    OwnedHandle::try_from(handle)
        .map_err(|_| io::Error::last_os_error())
}

/// Renames the opened `file` to `name` in `to`, the class and `how`
/// saying whether it may replace what is there. Through the NT call:
/// `SetFileInformationByHandle` reads the name as a path from the
/// current folder, and refuses it beside `RootDirectory`.
fn rename_to(
    file: &OwnedHandle,
    to: &OwnedHandle,
    name: &Name,
    class: FILE_INFORMATION_CLASS,
    how: FILE_RENAME_INFORMATION_0,
) -> io::Result<()> {
    let wide = name.as_os().encode_wide().collect::<Vec<_>>();
    let start = offset_of!(FILE_RENAME_INFORMATION, FileName);
    // The name runs past the struct, followed by a NUL.
    let size = start + 2 * (wide.len() + 1);
    let mut info =
        vec![
            FILE_RENAME_INFORMATION::default();
            size.div_ceil(size_of::<FILE_RENAME_INFORMATION>())
        ];
    info[0] = FILE_RENAME_INFORMATION {
        Anonymous: how,
        RootDirectory: to.as_raw_handle(),
        FileNameLength: (wide.len() * 2) as u32,
        FileName: [0],
    };
    // SAFETY: `info` holds at least `size` bytes, so the name fits
    // from `start`, which is two-byte aligned in it. `wide` is a
    // separate allocation.
    unsafe {
        info.as_mut_ptr()
            .cast::<u8>()
            .add(start)
            .cast::<u16>()
            .copy_from_nonoverlapping(wide.as_ptr(), wide.len());
    }
    let mut done = IO_STATUS_BLOCK::default();
    // SAFETY: the call reads `size_of_val(info)` bytes of `info`, and
    // writes only `done`.
    let status = unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut done,
            info.as_ptr().cast(),
            size_of_val(info.as_slice()) as u32,
            class,
        )
    };
    if status < 0 {
        // SAFETY: the call reads only `status`.
        return Err(code(unsafe { RtlNtStatusToDosError(status) }));
    }
    Ok(())
}

/// The target a symbolic link's or a junction's reparse `data` names,
/// as path text: relative as it is written, or from a drive. Any
/// other reparse point, or a target on no drive, names none.
fn target(data: &[u8]) -> Option<String> {
    let (names, relative) = match u32_at(data, 0)? {
        IO_REPARSE_TAG_SYMLINK => {
            let flags = u32_at(
                data,
                REPARSE_DATA
                    + offset_of!(REPARSE_DATA_BUFFER_0_0, Flags),
            )?;
            (
                REPARSE_DATA
                    + offset_of!(REPARSE_DATA_BUFFER_0_0, PathBuffer),
                flags & SYMLINK_FLAG_RELATIVE != 0,
            )
        }
        IO_REPARSE_TAG_MOUNT_POINT => (
            REPARSE_DATA
                + offset_of!(REPARSE_DATA_BUFFER_0_1, PathBuffer),
            false,
        ),
        _ => return None,
    };
    let field =
        |offset| u16_at(data, REPARSE_DATA + offset).map(usize::from);
    let at = names
        + field(offset_of!(
            REPARSE_DATA_BUFFER_0_0,
            SubstituteNameOffset
        ))?;
    let len = field(offset_of!(
        REPARSE_DATA_BUFFER_0_0,
        SubstituteNameLength
    ))?;
    let target = String::from_utf16(&wide_at(data, at, len)?).ok()?;
    if relative {
        return (!target.starts_with('\\'))
            .then(|| target.replace('\\', "/"));
    }
    let on_drive = target.strip_prefix(r"\??\")?;
    path::is_absolute(on_drive)
        .then(|| path::text(Path::new(on_drive)))
}

/// Adds the names a `FILE_FULL_DIR_INFO` listing in `bytes` holds,
/// but `.` and `..`.
fn listed(bytes: &[u8], names: &mut Vec<Name>) -> Option<()> {
    let mut at = 0;
    loop {
        let entry = bytes.get(at..)?;
        let len = u32_at(
            entry,
            offset_of!(FILE_FULL_DIR_INFO, FileNameLength),
        )?;
        let name = OsString::from_wide(&wide_at(
            entry,
            offset_of!(FILE_FULL_DIR_INFO, FileName),
            len as usize,
        )?);
        if name != "." && name != ".." {
            names.push(Name::listed(name));
        }
        match u32_at(
            entry,
            offset_of!(FILE_FULL_DIR_INFO, NextEntryOffset),
        )? {
            0 => return Some(()),
            next => at += next as usize,
        }
    }
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_ne_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// The `len` bytes at `at` as UTF-16 units.
fn wide_at(data: &[u8], at: usize, len: usize) -> Option<Vec<u16>> {
    let (units, _) = data.get(at..at.checked_add(len)?)?.as_chunks();
    Some(units.iter().map(|unit| u16::from_ne_bytes(*unit)).collect())
}

/// Whether an `Ex` information class failed only because the volume
/// or the system lacks it, as FAT does.
fn unsupported(error: &io::Error) -> bool {
    [
        ERROR_INVALID_PARAMETER,
        ERROR_NOT_SUPPORTED,
        ERROR_INVALID_FUNCTION,
    ]
    .into_iter()
    .any(|unsupported| error.raw_os_error() == Some(raw(unsupported)))
}

fn code(error: u32) -> io::Error {
    io::Error::from_raw_os_error(raw(error))
}

fn raw(error: u32) -> i32 {
    error.cast_signed()
}
