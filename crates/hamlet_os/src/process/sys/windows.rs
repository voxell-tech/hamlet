//! Windows: each command runs in a Job Object of its own. It starts
//! suspended and joins its job before its first instruction, so
//! nothing it starts runs outside the job, and ending the job ends
//! all of it. A bound command's job ends when its last handle, held
//! only by this process, closes.

#![expect(
    unsafe_code,
    reason = "the Win32 job, process, thread and handle calls"
)]

use std::fs::File;
use std::io::{self, Write as _};
use std::iter;
use std::os::windows::io::{
    AsHandle as _, AsRawHandle as _, BorrowedHandle,
    FromRawHandle as _, HandleOrInvalid, HandleOrNull,
    IntoRawHandle as _, OwnedHandle, RawHandle,
};
use std::process::{ExitStatus, Stdio};

use tokio::process::{Child, ChildStderr, ChildStdout};
use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle,
};
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
use windows_sys::Win32::System::Pipes::PeekNamedPipe;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, GetCurrentProcess,
    OpenProcess, OpenThread, PROCESS_SYNCHRONIZE, ResumeThread,
    THREAD_SUSPEND_RESUME, TerminateProcess, WaitForSingleObject,
};

use crate::process::{Command, Ends, Limits, Pipes, Preamble};

/// A command's job and the leader it was started with.
pub(crate) struct Group {
    job: Job,
    leader: Child,
    /// tokio forgets the id once the leader has been waited for.
    leader_id: u32,
    /// The leader's own handle, which tokio closes once it has
    /// waited for it. Held, it keeps the leader's id from another
    /// process.
    holds_id: OwnedHandle,
}

impl Group {
    pub(crate) fn kill(&mut self) {
        self.job.terminate();
    }

    pub(crate) async fn exited(&mut self) -> io::Result<ExitStatus> {
        self.leader.wait().await
    }

    /// Whether a process of the job other than its exited leader
    /// still runs. Kept when unknown, as ending them reaches only
    /// this job.
    pub(crate) fn is_running(&self) -> bool {
        self.job.members().is_none_or(|ids| {
            ids.iter().any(|&id| id != self.leader_id)
        })
    }

    /// Returns once they have exited: the system ends them after the
    /// call, and until then they hold their working folders, which
    /// the job's list stops naming first.
    pub(crate) fn end(self) {
        let ending = self
            .job
            .members()
            .unwrap_or_default()
            .into_iter()
            .filter(|&id| id != self.leader_id)
            .filter_map(waitable)
            .collect::<Vec<_>>();
        self.job.terminate();
        for process in ending {
            // SAFETY: the call only reads the handle.
            unsafe {
                WaitForSingleObject(process.as_raw_handle(), 2000)
            };
        }
    }
}

/// The process `id`, opened only to wait for its exit.
fn waitable(id: u32) -> Option<OwnedHandle> {
    // SAFETY: the call returns a new handle or null.
    let process = unsafe {
        HandleOrNull::from_raw_handle(OpenProcess(
            PROCESS_SYNCHRONIZE,
            0,
            id,
        ))
    };
    OwnedHandle::try_from(process).ok()
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

    /// The ids of the job's processes, none when unknown or more than
    /// 32.
    fn members(&self) -> Option<Vec<u32>> {
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
        let listed = ids.list.NumberOfProcessIdsInList;
        (done != 0 && listed == ids.list.NumberOfAssignedProcesses)
            .then(|| {
                iter::once(ids.list.ProcessIdList[0])
                    .chain(ids.more)
                    .take(listed as usize)
                    .map(|id| id as u32)
                    .collect()
            })
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
    stdin: Stdio,
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
    let mut spawn = command.program();
    spawn
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut group = Suspended::start(spawn, &info)?.resume()?;
    let (Some(stdout), Some(stderr)) =
        (group.leader.stdout.take(), group.leader.stderr.take())
    else {
        unreachable!("both outputs are piped");
    };
    Ok((group, stdout, stderr))
}

pub(crate) fn spawn_bound(
    command: Command<'_>,
) -> io::Result<(Group, Pipes)> {
    let (preamble, mut told) = io::pipe()?;
    let (child_input, input) = io::pipe()?;
    let (output, child_output) = io::pipe()?;
    let (errors, child_errors) = io::pipe()?;
    let mut spawn = command.program();
    // The child's C runtime fills its descriptors 0 to 2 from these
    // once, at its start.
    spawn
        .stdin(preamble)
        .stdout(child_errors.try_clone()?)
        .stderr(child_errors);
    let leader = Suspended::start(
        spawn,
        &limits_of(
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
        ),
    )?;
    told.write_all(
        &Preamble {
            input: leader.give(child_input.into())?,
            output: leader.give(child_output.into())?,
        }
        .encode(),
    )?;
    drop(told);
    Ok((
        leader.resume()?,
        Pipes {
            input,
            output,
            errors,
        },
    ))
}

/// A group whose leader is in its job and has not run. Dropped, it
/// kills the leader.
struct Suspended(Group);

impl Suspended {
    /// Starts `spawn` suspended, with no window, in a new job with
    /// `info`.
    fn start(
        mut spawn: tokio::process::Command,
        info: &JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    ) -> io::Result<Self> {
        let job = Job::new(info)?;
        // A leader that fails to join its job is dropped suspended,
        // and killed.
        spawn.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
        let leader = spawn.spawn()?;
        let (Some(leader_id), Some(handle)) =
            (leader.id(), leader.raw_handle())
        else {
            unreachable!("a leader that has not run has not exited");
        };
        // SAFETY: the handle is the leader's, open while `leader`
        // lives.
        let holds_id = unsafe { BorrowedHandle::borrow_raw(handle) }
            .try_clone_to_owned()?;
        job.assign(handle)?;
        Ok(Self(Group {
            job,
            leader,
            leader_id,
            holds_id,
        }))
    }

    /// Gives the leader its own handle to what `end` is, which the
    /// processes it starts do not inherit, and returns its value
    /// there. This process keeps no copy.
    fn give(&self, end: OwnedHandle) -> io::Result<u64> {
        let mut given = std::ptr::null_mut();
        // SAFETY: both process handles and `end` are open for the
        // call, which writes only `given`. The leader's handle, from
        // its creation, has every access.
        let done = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                end.as_raw_handle(),
                self.0.holds_id.as_raw_handle(),
                &raw mut given,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if done == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(given as usize as u64)
    }

    /// Lets the leader run.
    fn resume(self) -> io::Result<Group> {
        resume(self.0.leader_id)?;
        Ok(self.0)
    }
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

/// A copy of this process's standard input, and the bytes waiting on
/// it now. Fails when it is not a pipe.
pub(crate) fn standard_input() -> io::Result<(File, u64)> {
    let stdin = io::stdin();
    let mut waiting = 0;
    // SAFETY: the call reads no byte and writes only `waiting`.
    let done = unsafe {
        PeekNamedPipe(
            stdin.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &raw mut waiting,
            std::ptr::null_mut(),
        )
    };
    if done == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((
        File::from(stdin.as_handle().try_clone_to_owned()?),
        waiting.into(),
    ))
}

pub(crate) fn take_ends(preamble: &Preamble) -> io::Result<Ends> {
    let handle = |value: u64| {
        usize::try_from(value)
            .map(|value| value as RawHandle)
            .map_err(io::Error::other)
    };
    let (input, output) =
        (handle(preamble.input)?, handle(preamble.output)?);
    // SAFETY: the parent made both for this process alone before it
    // ran, and the preamble that names them is read once, so nothing
    // else owns them.
    let (input, output) = unsafe {
        (
            OwnedHandle::from_raw_handle(input),
            OwnedHandle::from_raw_handle(output),
        )
    };
    Ok(Ends {
        input: input.into(),
        output: output.into(),
    })
}

pub(crate) fn end_now() -> ! {
    // SAFETY: GetCurrentProcess returns a pseudo handle that is
    // always valid.
    unsafe { TerminateProcess(GetCurrentProcess(), 1) };
    std::process::abort()
}
