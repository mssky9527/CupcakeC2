//! Windows Job Object — kill workers when agent dies; resource limits.

#[cfg(windows)]
use std::ptr;

/// Max processes in a worker job tree (worker + children).
#[cfg(windows)]
const JOB_ACTIVE_PROCESS_LIMIT: u32 = 32;

/// Aggregate job memory cap (bytes).
#[cfg(windows)]
const JOB_MEMORY_LIMIT_BYTES: usize = 512 * 1024 * 1024;

/// Per-process user-mode CPU time limit (100-ns units). 60 seconds.
#[cfg(windows)]
const PER_PROCESS_USER_TIME_100NS: i64 = 60 * 10_000_000;

#[cfg(windows)]
pub(crate) struct JobObject {
    handle: usize,
}

#[cfg(windows)]
impl JobObject {
    /// Create a Job Object with kill-on-close and resource limits.
    /// Returns `None` if creation or limit configuration fails (fail-closed).
    pub fn create() -> Option<Self> {
        unsafe {
            type CreateJobObjectW = unsafe extern "system" fn(
                *mut core::ffi::c_void,
                *const u16,
            )
                -> *mut core::ffi::c_void;
            let k32 = crate::stealth::ensure_module_base(
                b"kernel32.dll",
                crate::stealth::hash_module_name(b"kernel32.dll"),
            );
            if k32 == 0 {
                return None;
            }
            let addr = crate::stealth::get_api_addr(
                k32,
                crate::stealth::hash_api_name(b"CreateJobObjectW"),
            )?;
            let f: CreateJobObjectW = std::mem::transmute(addr);
            let h = f(ptr::null_mut(), ptr::null());
            if h.is_null() {
                return None;
            }
            let job = Self { handle: h as usize };
            // Fail-closed: without kill-on-close + limits the worker is uncontained.
            if job.set_limits().is_err() {
                // Drop closes the handle
                drop(job);
                return None;
            }
            Some(job)
        }
    }

    /// Configure kill-on-close, active process limit, job memory, and CPU time.
    fn set_limits(&self) -> Result<(), ()> {
        // JobObjectExtendedLimitInformation = 9
        #[repr(C)]
        struct JobObjectBasicLimitInformation {
            per_process_user_time_limit: i64,
            per_job_user_time_limit: i64,
            limit_flags: u32,
            minimum_working_set_size: usize,
            maximum_working_set_size: usize,
            active_process_limit: u32,
            affinity: usize,
            priority_class: u32,
            scheduling_class: u32,
        }
        #[repr(C)]
        struct IoCounters {
            read_op: u64,
            write_op: u64,
            other_op: u64,
            read_tx: u64,
            write_tx: u64,
            other_tx: u64,
        }
        #[repr(C)]
        struct JobObjectExtendedLimitInformation {
            basic: JobObjectBasicLimitInformation,
            io: IoCounters,
            process_memory_limit: usize,
            job_memory_limit: usize,
            peak_process_memory: usize,
            peak_job_memory: usize,
        }
        const JOB_OBJECT_LIMIT_PROCESS_TIME: u32 = 0x0000_0002;
        const JOB_OBJECT_LIMIT_ACTIVE_PROCESS: u32 = 0x0000_0008;
        const JOB_OBJECT_LIMIT_JOB_MEMORY: u32 = 0x0000_0200;
        const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

        unsafe {
            type SetInfo = unsafe extern "system" fn(
                *mut core::ffi::c_void,
                i32,
                *const core::ffi::c_void,
                u32,
            ) -> i32;
            let k32 = crate::stealth::ensure_module_base(
                b"kernel32.dll",
                crate::stealth::hash_module_name(b"kernel32.dll"),
            );
            if k32 == 0 {
                return Err(());
            }
            let addr = crate::stealth::get_api_addr(
                k32,
                crate::stealth::hash_api_name(b"SetInformationJobObject"),
            )
            .ok_or(())?;
            let f: SetInfo = std::mem::transmute(addr);
            let mut info: JobObjectExtendedLimitInformation = std::mem::zeroed();
            info.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                | JOB_OBJECT_LIMIT_JOB_MEMORY
                | JOB_OBJECT_LIMIT_PROCESS_TIME;
            info.basic.active_process_limit = JOB_ACTIVE_PROCESS_LIMIT;
            info.basic.per_process_user_time_limit = PER_PROCESS_USER_TIME_100NS;
            info.job_memory_limit = JOB_MEMORY_LIMIT_BYTES;
            let rc = f(
                self.handle as *mut _,
                9, // JobObjectExtendedLimitInformation
                &info as *const _ as *const _,
                std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
            );
            if rc == 0 {
                Err(())
            } else {
                Ok(())
            }
        }
    }

    /// Assign a child process. Failure means the caller must terminate the child.
    pub fn assign_process(&self, h_process: usize) -> Result<(), ()> {
        unsafe {
            type Assign =
                unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void) -> i32;
            let k32 = crate::stealth::ensure_module_base(
                b"kernel32.dll",
                crate::stealth::hash_module_name(b"kernel32.dll"),
            );
            if k32 == 0 {
                return Err(());
            }
            let addr = crate::stealth::get_api_addr(
                k32,
                crate::stealth::hash_api_name(b"AssignProcessToJobObject"),
            )
            .ok_or(())?;
            let f: Assign = std::mem::transmute(addr);
            if f(self.handle as *mut _, h_process as *mut _) == 0 {
                Err(())
            } else {
                Ok(())
            }
        }
    }

    pub fn terminate(&self, exit_code: u32) -> Result<(), ()> {
        unsafe {
            type Term = unsafe extern "system" fn(*mut core::ffi::c_void, u32) -> i32;
            let k32 = crate::stealth::ensure_module_base(
                b"kernel32.dll",
                crate::stealth::hash_module_name(b"kernel32.dll"),
            );
            if k32 == 0 {
                return Err(());
            }
            let addr = crate::stealth::get_api_addr(
                k32,
                crate::stealth::hash_api_name(b"TerminateJobObject"),
            )
            .ok_or(())?;
            let f: Term = std::mem::transmute(addr);
            if f(self.handle as *mut _, exit_code) == 0 {
                Err(())
            } else {
                Ok(())
            }
        }
    }
}

#[cfg(windows)]
impl Drop for JobObject {
    fn drop(&mut self) {
        unsafe {
            type Close = unsafe extern "system" fn(*mut core::ffi::c_void) -> i32;
            let k32 = crate::stealth::ensure_module_base(
                b"kernel32.dll",
                crate::stealth::hash_module_name(b"kernel32.dll"),
            );
            if k32 == 0 {
                return;
            }
            if let Some(addr) =
                crate::stealth::get_api_addr(k32, crate::stealth::hash_api_name(b"CloseHandle"))
            {
                let f: Close = std::mem::transmute(addr);
                let _ = f(self.handle as *mut _);
            }
        }
    }
}

#[cfg(not(windows))]
pub struct JobObject;

#[cfg(not(windows))]
impl JobObject {
    pub fn create() -> Option<Self> {
        None
    }
    pub fn assign_process(&self, _: usize) -> Result<(), ()> {
        Err(())
    }
    pub fn terminate(&self, _: u32) -> Result<(), ()> {
        Err(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn create_fail_closed_on_non_windows_or_ok_on_windows() {
        // On non-Windows, create always returns None (fail-closed path).
        // On Windows, create may succeed if kernel32 resolves; either way the
        // API contract is Option and callers treat None as isolation unavailable.
        let job = super::JobObject::create();
        #[cfg(not(windows))]
        {
            assert!(job.is_none(), "non-windows must fail-closed");
        }
        #[cfg(windows)]
        {
            // If create succeeds, limits were applied (set_limits fail → None).
            // If create fails, callers already fail-closed.
            let _ = job;
        }
    }
}
