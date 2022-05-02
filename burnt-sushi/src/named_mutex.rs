#![allow(dead_code)]

use std::{io, marker::PhantomData, os::windows::raw::HANDLE, ptr};

use widestring::U16CString;
use winapi::{
    shared::winerror::WAIT_TIMEOUT,
    um::{
        synchapi::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
        winbase::{INFINITE, WAIT_ABANDONED, WAIT_OBJECT_0},
    },
};

#[derive(Debug)]
pub struct NamedMutex(HANDLE);

impl NamedMutex {
    pub fn new(name: &str) -> io::Result<Self> {
        let name = U16CString::from_str(format!("Global\\{}", &name)).unwrap();

        let handle = unsafe { CreateMutexW(ptr::null_mut(), 0, name.as_ptr()) };

        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }

    pub fn try_lock(&self) -> io::Result<Option<NamedMutexGuard>> {
        let rc = unsafe { WaitForSingleObject(self.0, 0) };

        if rc == WAIT_OBJECT_0 || rc == WAIT_ABANDONED {
            Ok(Some(unsafe { self.new_guard() }))
        } else if rc == WAIT_TIMEOUT {
            Ok(None)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn lock(&self) -> io::Result<NamedMutexGuard> {
        let rc = unsafe { WaitForSingleObject(self.0, INFINITE) };

        if rc == WAIT_OBJECT_0 || rc == WAIT_ABANDONED {
            Ok(unsafe { self.new_guard() })
        } else {
            Err(io::Error::last_os_error())
        }
    }

    unsafe fn new_guard(&self) -> NamedMutexGuard {
        NamedMutexGuard(self.0, PhantomData)
    }
}

#[derive(Debug)]
pub struct NamedMutexGuard<'lock>(HANDLE, PhantomData<&'lock NamedMutex>);

impl<'lock> NamedMutexGuard<'lock> {
    pub fn unlock(mut self) -> io::Result<()> {
        unsafe { self._unlock() }?;
        self.0 = ptr::null_mut();
        Ok(())
    }

    unsafe fn _unlock(&mut self) -> io::Result<()> {
        let result = unsafe { ReleaseMutex(self.0) };

        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

impl Drop for NamedMutexGuard<'_> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let result = unsafe { self._unlock() };
            debug_assert!(
                result.is_ok(),
                "Failed to unlock mutex: {:?}",
                result.unwrap_err()
            );
        }
    }
}
