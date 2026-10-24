//! Groups a tool's process with every process it starts, so stopping the tool stops them all.
//!
//! On Windows this is a job object set to kill its processes when its last handle closes. The
//! launcher holds the only handle, so if the launcher dies, Windows closes the handle and the
//! tools die with it.

use std::io;
use std::process::Child;

#[cfg(windows)]
pub use windows::Job;

#[cfg(not(windows))]
pub use other::Job;

#[cfg(windows)]
mod windows {
	use super::*;
	use std::os::windows::io::AsRawHandle;
	use std::ptr;
	use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
	use windows_sys::Win32::System::JobObjects::{
		AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
		SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
		JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
	};

	pub struct Job(HANDLE);

	// A job handle can be used from any thread.
	unsafe impl Send for Job {}
	unsafe impl Sync for Job {}

	impl Job {
		/// Creates a job and puts the child in it. Processes the child starts from then on join it too.
		pub fn for_child(child: &Child) -> io::Result<Job> {
			// SAFETY: plain Win32 calls on a handle this function owns; the info struct outlives the call.
			unsafe {
				let handle = CreateJobObjectW(ptr::null(), ptr::null());
				if handle.is_null() {
					return Err(io::Error::last_os_error());
				}
				let job = Job(handle);
				let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
				info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
				if SetInformationJobObject(
					job.0,
					JobObjectExtendedLimitInformation,
					&info as *const _ as *const _,
					std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
				) == 0
				{
					return Err(io::Error::last_os_error());
				}
				if AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) == 0 {
					return Err(io::Error::last_os_error());
				}
				Ok(job)
			}
		}

		/// Ends every process in the job.
		pub fn terminate(&self) {
			// SAFETY: the handle is valid until drop.
			unsafe {
				TerminateJobObject(self.0, 1);
			}
		}
	}

	impl Drop for Job {
		fn drop(&mut self) {
			// SAFETY: the handle is owned and closed once. Closing it ends anything still in the job.
			unsafe {
				CloseHandle(self.0);
			}
		}
	}
}

// Windows comes first; elsewhere only the tool's own process is stopped for now.
#[cfg(not(windows))]
mod other {
	use super::*;

	pub struct Job(u32);

	impl Job {
		pub fn for_child(child: &Child) -> io::Result<Job> {
			Ok(Job(child.id()))
		}

		pub fn terminate(&self) {
			let _ = std::process::Command::new("kill")
				.arg("-9")
				.arg(self.0.to_string())
				.status();
		}
	}
}
