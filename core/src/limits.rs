//! Windows job lifetime bounds all descendants, including Tesseract.
#[cfg(windows)]
pub struct WorkerJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl WorkerJob {
    pub fn attach(child: &std::process::Child) -> anyhow::Result<Self> {
        use std::{mem, os::windows::io::AsRawHandle};
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            anyhow::ensure!(
                !handle.is_null(),
                "Не удалось создать ограничение обработчика"
            );
            let job = Self(handle);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_JOB_MEMORY;
            limits.JobMemoryLimit = 1536 * 1024 * 1024;
            anyhow::ensure!(
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const _,
                    mem::size_of_val(&limits) as u32
                ) != 0,
                "Не удалось ограничить память обработчика"
            );
            anyhow::ensure!(
                AssignProcessToJobObject(handle, child.as_raw_handle() as _) != 0,
                "Не удалось изолировать обработчик"
            );
            Ok(job)
        }
    }
    pub fn terminate(&self) {
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}
#[cfg(windows)]
impl Drop for WorkerJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
