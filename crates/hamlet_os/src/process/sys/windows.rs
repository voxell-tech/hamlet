//! Windows: each command runs in a Job Object of its own. It starts
//! suspended and joins its job before its first instruction, so
//! nothing it starts runs outside the job, and ending the job ends
//! all of it.

#![expect(unsafe_code, reason = "the Win32 job and thread calls")]

use std::os::windows::io::{
    AsRawHandle as _, HandleOrInvalid, HandleOrNull,
    IntoRawHandle as _, OwnedHandle, RawHandle,
};
use std::process::{ExitStatus, Stdio};
use std::{io, iter};

use tokio::process::{Child, ChildStderr, ChildStdout};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32,
    Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT,
    JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_TIME, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, GetCurrentProcess,
    OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};

use crate::process::{Command, Limits};

/// A command's job and the leader it was started with.
pub(crate) struct Group {
    job: Job,
    leader: Child,
    /// tokio forgets the id once the leader has been waited for.
    leader_id: u32,
}

impl Group {
    pub(crate) fn kill(&mut self) {
        self.job.terminate();
    }

    pub(crate) async fn exited(&mut self) -> io::Result<ExitStatus> {
        self.leader.wait().await
    }

    /// Whether a process of the job other than its exited leader
    /// still runs.
    pub(crate) fn is_running(&self) -> bool {
        self.job.holds_other_than(self.leader_id)
    }

    pub(crate) fn end(self) {
        self.job.terminate();
    }
}

struct Job(OwnedHandle);

impl Job {
    fn new(
        info: &JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    ) -> io::Result<Self> {
        // SAFETY: with no name and no attributes the call only makes
        // a job, and returns a new handle or null.
        let job = unsafe {
            HandleOrNull::from_raw_handle(CreateJobObjectW(
                std::ptr::null(),
                std::ptr::null(),
            ))
        };
        let job = OwnedHandle::try_from(job)
            .map_err(|_| io::Error::last_os_error())?;
        // SAFETY: the call reads `size_of_val(info)` bytes of `info`.
        let done = unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(info).cast(),
                size_of_val(info) as u32,
            )
        };
        if done == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(job))
    }

    fn assign(&self, process: RawHandle) -> io::Result<()> {
        // SAFETY: the call only reads both handles, open for its
        // length.
        let done = unsafe {
            AssignProcessToJobObject(self.0.as_raw_handle(), process)
        };
        if done == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn terminate(&self) {
        // SAFETY: the call only reads the handle.
        unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) };
    }

    /// Kept when unknown, as ending them reaches only this job.
    fn holds_other_than(&self, leader: u32) -> bool {
        /// Room for 32 ids. A job with more holds others anyway.
        #[repr(C)]
        struct Ids {
            list: JOBOBJECT_BASIC_PROCESS_ID_LIST,
            more: [usize; 31],
        }
        let mut ids = Ids {
            list: JOBOBJECT_BASIC_PROCESS_ID_LIST::default(),
            more: [0; 31],
        };
        // SAFETY: the call writes at most `size_of::<Ids>()` bytes
        // into `ids`, whose ids follow the list's first one.
        let done = unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectBasicProcessIdList,
                std::ptr::from_mut(&mut ids).cast(),
                size_of::<Ids>() as u32,
                std::ptr::null_mut(),
            )
        };
        done == 0
            || iter::once(ids.list.ProcessIdList[0])
                .chain(ids.more)
                .take(ids.list.NumberOfProcessIdsInList as usize)
                .any(|id| id != leader as usize)
    }
}

fn limits_of(
    flags: JOB_OBJECT_LIMIT,
) -> JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = flags;
    info
}

pub(crate) fn spawn(
    command: Command<'_>,
    limits: &Limits,
) -> io::Result<(Group, ChildStdout, ChildStderr)> {
    let mut info = limits_of(
        JOB_OBJECT_LIMIT_PROCESS_TIME
            | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    );
    let basic = &mut info.BasicLimitInformation;
    // In ticks of 100 ns, of user time.
    basic.PerProcessUserTimeLimit =
        i64::try_from(limits.cpu_secs.saturating_mul(10_000_000))
            .unwrap_or(i64::MAX);
    basic.ActiveProcessLimit =
        u32::try_from(limits.max_procs).unwrap_or(u32::MAX);
    let job = Job::new(&info)?;
    let mut spawn = tokio::process::Command::new(&command.argv[0]);
    spawn
        .args(&command.argv[1..])
        .env_clear()
        .envs(command.env)
        .current_dir(command.cwd)
        .stdin(command.stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A leader that fails to join its job is dropped suspended,
        // and killed.
        .kill_on_drop(true)
        .creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
    let mut leader = spawn.spawn()?;
    let (Some(leader_id), Some(handle)) =
        (leader.id(), leader.raw_handle())
    else {
        unreachable!("a leader that has not run has not exited");
    };
    job.assign(handle)?;
    resume(leader_id)?;
    let (Some(stdout), Some(stderr)) =
        (leader.stdout.take(), leader.stderr.take())
    else {
        unreachable!("both outputs are piped");
    };
    Ok((
        Group {
            job,
            leader,
            leader_id,
        },
        stdout,
        stderr,
    ))
}

/// Resumes the threads of the suspended process `id`. Neither std
/// nor tokio hands out the handle of the thread it started.
fn resume(id: u32) -> io::Result<()> {
    // SAFETY: the call returns a new handle or an invalid one.
    let snapshot = unsafe {
        HandleOrInvalid::from_raw_handle(CreateToolhelp32Snapshot(
            TH32CS_SNAPTHREAD,
            0,
        ))
    };
    let snapshot = OwnedHandle::try_from(snapshot)
        .map_err(|_| io::Error::last_os_error())?;
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    let mut resumed = false;
    for next in iter::once(Thread32First as Walk)
        .chain(iter::repeat(Thread32Next as Walk))
    {
        // SAFETY: the call writes only `entry`, whose size it reads
        // from `dwSize`.
        if unsafe { next(snapshot.as_raw_handle(), &raw mut entry) }
            == 0
        {
            break;
        }
        if entry.th32OwnerProcessID == id {
            resume_thread(entry.th32ThreadID)?;
            resumed = true;
        }
    }
    if resumed {
        Ok(())
    } else {
        Err(io::Error::other("the command's thread was not found"))
    }
}

type Walk = unsafe extern "system" fn(
    RawHandle,
    *mut THREADENTRY32,
) -> windows_sys::core::BOOL;

fn resume_thread(id: u32) -> io::Result<()> {
    // SAFETY: the call returns a new handle or null.
    let thread = unsafe {
        HandleOrNull::from_raw_handle(OpenThread(
            THREAD_SUSPEND_RESUME,
            0,
            id,
        ))
    };
    let thread = OwnedHandle::try_from(thread)
        .map_err(|_| io::Error::last_os_error())?;
    // SAFETY: the call only reads the handle.
    if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Puts this process in a job that kills everything in it when its
/// last handle closes. The handle is never closed by hand: the
/// system closes it as this process exits, however it exits.
pub(crate) fn reap_with_server() -> io::Result<()> {
    let job =
        Job::new(&limits_of(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE))?;
    // SAFETY: the call returns a constant that names this process.
    job.assign(unsafe { GetCurrentProcess() })?;
    let _ = job.0.into_raw_handle();
    Ok(())
}

pub(crate) fn limited(
    _limits: &Limits,
    argv: &[String],
) -> Vec<String> {
    argv.to_vec()
}

/// No job limit bounds the size of a file.
pub(crate) fn unenforced(
    _limits: &Limits,
) -> &'static [&'static str] {
    &["max_file_mb"]
}

pub(crate) fn is_executable(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}
