use std::{marker::PhantomData, ptr};

use windows_sys::Win32::{
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW,
        TranslateMessage, WM_QUIT, WM_USER,
    },
};

pub struct MessageLoop {
    thread_id: u32,
    _not_send: PhantomData<*const ()>,
}

impl MessageLoop {
    pub fn new() -> Self {
        // Windows creates a thread's message queue lazily, and posting to a thread
        // without one fails. Peeking forces the queue to exist before a QuitHandle can post.
        let mut msg = MSG::default();
        unsafe { PeekMessageW(&raw mut msg, ptr::null_mut(), WM_USER, WM_USER, PM_NOREMOVE) };

        Self {
            thread_id: unsafe { GetCurrentThreadId() },
            _not_send: PhantomData,
        }
    }

    pub fn quit_handle(&self) -> QuitHandle {
        QuitHandle {
            thread_id: self.thread_id,
        }
    }

    pub fn run(self) {
        let mut msg = MSG::default();
        while unsafe { GetMessageW(&raw mut msg, ptr::null_mut(), 0, 0) } > 0 {
            unsafe {
                TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }
        }

        let _ = self;
    }
}

#[derive(Clone, Copy)]
pub struct QuitHandle {
    thread_id: u32,
}

impl QuitHandle {
    pub fn quit(self) {
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
    }
}
