use std::io;
use tokio::process::{Child, Command};

#[derive(Debug)]
pub(crate) struct ProcessTreeOwner {
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(not(windows))]
    pid: u32,
}

impl ProcessTreeOwner {
    pub(crate) async fn attach_and_resume(child: &mut Child, pid: u32) -> Result<Self, String> {
        #[cfg(windows)]
        {
            attach_windows_job(child, pid)
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            Ok(Self { pid })
        }
    }

    pub(crate) async fn terminate(&mut self) -> Result<(), String> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::JobObjects::TerminateJobObject;
            if self.job.is_null() {
                return Err("process job handle is null".to_owned());
            }
            if unsafe { TerminateJobObject(self.job, 1) } == 0 {
                return Err(format!(
                    "TerminateJobObject failed: {}",
                    io::Error::last_os_error()
                ));
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let mut child = Command::new("kill")
                .args(["-KILL", &self.pid.to_string()])
                .spawn()
                .await
                .map_err(|e| format!("kill could not start: {e}"))?;
            let status = child
                .wait()
                .await
                .map_err(|e| format!("kill wait failed: {e}"))?;
            if status.success() {
                Ok(())
            } else {
                Err(format!("kill returned {status}"))
            }
        }
    }
}

impl Drop for ProcessTreeOwner {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::CloseHandle;
            if !self.job.is_null() {
                unsafe { CloseHandle(self.job) };
                self.job = std::ptr::null_mut();
            }
        }
    }
}

#[cfg(windows)]
pub(crate) fn configure_suspended_creation(command: &mut Command) {
    use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;
    command.creation_flags(CREATE_SUSPENDED);
}

#[cfg(not(windows))]
pub(crate) fn configure_suspended_creation(_command: &mut Command) {}

#[cfg(windows)]
fn attach_windows_job(child: &mut Child, pid: u32) -> Result<ProcessTreeOwner, String> {
    use std::{mem, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
                THREADENTRY32,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
        },
    };

    let process_handle = child
        .raw_handle()
        .ok_or_else(|| "child process exposes no raw Windows handle".to_owned())?;

    let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if job.is_null() {
        let _ = child.start_kill();
        return Err(format!(
            "CreateJobObjectW failed: {}",
            io::Error::last_os_error()
        ));
    }

    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

    if unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &mut limits as *mut _ as *mut _,
            mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        unsafe { CloseHandle(job) };
        let _ = child.start_kill();
        return Err(format!(
            "SetInformationJobObject failed: {}",
            io::Error::last_os_error()
        ));
    }

    if unsafe { AssignProcessToJobObject(job, process_handle) } == 0 {
        let err = io::Error::last_os_error();
        unsafe { CloseHandle(job) };
        let _ = child.start_kill();
        return Err(format!("AssignProcessToJobObject failed: {err}"));
    }

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        unsafe {
            TerminateJobObject(job, 1);
            CloseHandle(job);
        }
        let _ = child.start_kill();
        return Err(format!(
            "CreateToolhelp32Snapshot failed: {}",
            io::Error::last_os_error()
        ));
    }

    let mut entry: THREADENTRY32 = unsafe { mem::zeroed() };
    entry.dwSize = mem::size_of::<THREADENTRY32>() as u32;
    let mut thread_id = None;
    let mut ok = unsafe { Thread32First(snapshot, &mut entry) } != 0;
    while ok {
        if entry.th32OwnerProcessID == pid {
            thread_id = Some(entry.th32ThreadID);
            break;
        }
        ok = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };

    let Some(primary_thread_id) = thread_id else {
        unsafe {
            TerminateJobObject(job, 1);
            CloseHandle(job);
        }
        let _ = child.start_kill();
        return Err(format!(
            "could not locate suspended primary thread for pid {pid}"
        ));
    };

    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, primary_thread_id) };
    if thread.is_null() {
        unsafe {
            TerminateJobObject(job, 1);
            CloseHandle(job);
        }
        let _ = child.start_kill();
        return Err(format!(
            "OpenThread failed for primary thread {primary_thread_id}: {}",
            io::Error::last_os_error()
        ));
    }

    let previous_suspend_count = unsafe { ResumeThread(thread) };
    unsafe { CloseHandle(thread) };
    if previous_suspend_count == u32::MAX {
        unsafe {
            TerminateJobObject(job, 1);
            CloseHandle(job);
        }
        let _ = child.start_kill();
        return Err(format!(
            "ResumeThread failed for primary thread {primary_thread_id}: {}",
            io::Error::last_os_error()
        ));
    }

    Ok(ProcessTreeOwner { job })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn suspended_child_is_contained_before_resume() {
        let mut command = Command::new("cmd.exe");
        command.args(["/C", "exit", "0"]);
        configure_suspended_creation(&mut command);
        let mut child = command.spawn().expect("spawn suspended child");
        let pid = child.id().expect("pid");
        let mut owner = attach_windows_job(&mut child, pid).expect("job ownership");
        owner.terminate().await.expect("job termination");
        let _ = child.wait().await;
    }
}
