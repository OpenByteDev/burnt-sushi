#![allow(dead_code)]

use std::{
    fmt::Display,
    fs::File,
    io::{self, Write},
    mem::{self, MaybeUninit},
    os::windows::prelude::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle},
    ptr,
};

use dll_syringe::process::{OwnedProcess, Process};
use project_uninit::partial_init;
use widestring::U16CString;
use winapi::{
    shared::minwindef::{FALSE, TRUE},
    um::{
        handleapi::{CloseHandle, SetHandleInformation},
        minwinbase::SECURITY_ATTRIBUTES,
        namedpipeapi::CreatePipe,
        processthreadsapi::{CreateProcessW, STARTUPINFOW},
        winbase::{CREATE_NEW_CONSOLE, HANDLE_FLAG_INHERIT, STARTF_USESTDHANDLES},
    },
};

use crate::{APP_NAME_WITH_VERSION, logger::SimpleLog};

use super::raw;

#[derive(Debug)]
pub struct Console(ConsoleImpl);

#[derive(Debug)]
enum ConsoleImpl {
    Attach,
    Alloc,
    Piped {
        process: OwnedProcess,
        pipe: File,
    },
}

unsafe impl Send for Console {}

impl Console {
    pub fn attach() -> Option<Self> {
        raw::attach().then(|| Self(ConsoleImpl::Attach))
    }
    pub fn alloc() -> Option<Self> {
        raw::alloc().then(|| Self(ConsoleImpl::Alloc))
    }
    pub fn piped() -> io::Result<Self> {
        let mut security_attributes = SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as _,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: TRUE, // Set the bInheritHandle flag so pipe handles are inherited.
        };
        let mut child_stdin_read_pipe = MaybeUninit::uninit();
        let mut child_stdin_write_pipe = MaybeUninit::uninit();
        // Create a pipe for the child process's STDIN.
        if unsafe {
            CreatePipe(
                child_stdin_read_pipe.as_mut_ptr(),
                child_stdin_write_pipe.as_mut_ptr(),
                &mut security_attributes,
                0,
            )
        } == FALSE
        {
            return Err(io::Error::last_os_error());
        }
        let child_stdin_read_pipe =
            unsafe { OwnedHandle::from_raw_handle(child_stdin_read_pipe.assume_init()) };
        let child_stdin_write_pipe =
            unsafe { OwnedHandle::from_raw_handle(child_stdin_write_pipe.assume_init()) };

        // Ensure the write handle to the pipe for STDIN is not inherited.
        if unsafe {
            SetHandleInformation(
                child_stdin_write_pipe.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                0,
            )
        } == FALSE
        {
            return Err(io::Error::last_os_error());
        }

        let mut title = U16CString::from_str(APP_NAME_WITH_VERSION).unwrap();
        let mut command_line =
            U16CString::from_str("powershell.exe -Command \"for(;;) { $m = Read-Host }\"").unwrap();
        let mut startup_info = MaybeUninit::<STARTUPINFOW>::uninit();
        partial_init!(startup_info => {
            cb: mem::size_of::<STARTUPINFOW>() as _,
            lpReserved: ptr::null_mut(),
            lpDesktop: ptr::null_mut(),
            lpTitle: title.as_mut_ptr(),
            cbReserved2: 0,
            dwFlags: STARTF_USESTDHANDLES,
            lpReserved2: ptr::null_mut(),
            hStdInput: child_stdin_read_pipe.into_raw_handle(),
            hStdOutput: ptr::null_mut(),
            hStdError: ptr::null_mut(),
        });

        let mut process_info = MaybeUninit::uninit();
        let result = unsafe {
            CreateProcessW(
                ptr::null_mut(),
                command_line.as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                TRUE,
                CREATE_NEW_CONSOLE,
                ptr::null_mut(),
                ptr::null_mut(),
                startup_info.as_mut_ptr(),
                process_info.as_mut_ptr(),
            )
        };
        if result == FALSE {
            return Err(io::Error::last_os_error());
        }

        let process_info = unsafe { process_info.assume_init() };
        unsafe { CloseHandle(process_info.hThread) };

        let process = unsafe { OwnedProcess::from_raw_handle(process_info.hProcess) };
        let pipe = File::from(child_stdin_write_pipe);
        Ok(Self(ConsoleImpl::Piped { process, pipe }))
    }

    pub fn is_active(&self) -> bool {
        match &self.0 {
            ConsoleImpl::Attach | ConsoleImpl::Alloc => true,
            ConsoleImpl::Piped { process, .. } => process.is_alive(),
        }
    }
    pub fn is_attached(&self) -> bool {
        matches!(self.0, ConsoleImpl::Attach | ConsoleImpl::Alloc)
    }
    pub fn is_piped(&self) -> bool {
        matches!(self.0, ConsoleImpl::Piped { .. })
    }

    pub fn println(&mut self, message: impl Display) -> io::Result<()> {
        match &mut self.0 {
            ConsoleImpl::Attach | ConsoleImpl::Alloc => println!("{message}"),
            ConsoleImpl::Piped { pipe, .. } => writeln!(pipe, "{message}")?,
        }
        Ok(())
    }
}

impl SimpleLog for Console {
    fn log(&mut self, message: &str) {
        self.println(message).unwrap()
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        match self.0 {
            ConsoleImpl::Attach => {}
            ConsoleImpl::Alloc => raw::free(),
            ConsoleImpl::Piped { ref process, .. } => {
                let _ = process.kill();
            }
        }
    }
}
