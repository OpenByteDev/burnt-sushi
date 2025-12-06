use dll_syringe::process::{OwnedProcess, Process};
use fallible_iterator::FallibleIterator;
use log::info;
use project_uninit::partial_init;
use std::{
    io,
    mem::{self, MaybeUninit},
    num::{NonZeroU32, NonZeroUsize},
    os::windows::prelude::{AsRawHandle, HandleOrInvalid, OwnedHandle},
    ptr,
};
use winapi::{
    shared::{
        minwindef::{BOOL, FALSE},
        windef::HWND,
        winerror::ERROR_NO_MORE_FILES,
    },
    um::{
        errhandlingapi::{GetLastError, SetLastError},
        tlhelp32::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        },
        winuser::{
            EnumChildWindows, EnumThreadWindows, GetClassNameW, GetWindowTextLengthW,
            GetWindowTextW, GetWindowThreadProcessId,
        },
    },
};
use wineventhook::{raw_event, AccessibleObjectId, EventFilter, WindowEventHook, WindowHandle};

#[derive(Debug)]
pub struct SpotifyProcessScanner {
    notifier: tokio::sync::watch::Sender<SpotifyState>,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum SpotifyState {
    Running(SpotifyInfo),
    Stopped,
}

impl SpotifyState {
    pub fn try_clone(&self) -> io::Result<Self> {
        match self {
            SpotifyState::Running(info) => Ok(SpotifyState::Running(info.try_clone()?)),
            SpotifyState::Stopped => Ok(SpotifyState::Stopped),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct SpotifyInfo {
    pub process: OwnedProcess,
    pub main_window: WindowHandle,
}

unsafe impl Send for SpotifyInfo {}
unsafe impl Sync for SpotifyInfo {}

impl SpotifyInfo {
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            process: self.process.try_clone()?,
            main_window: self.main_window,
        })
    }
}

impl SpotifyProcessScanner {
    pub fn new() -> (Self, tokio::sync::watch::Receiver<SpotifyState>) {
        let (tx, rx) = tokio::sync::watch::channel(SpotifyState::Stopped);
        let scanner = Self { notifier: tx };
        (scanner, rx)
    }

    #[allow(dead_code)]
    pub fn spawn(self) -> tokio::task::JoinHandle<io::Result<()>> {
        tokio::spawn(async move { self.run().await })
    }

    pub async fn run(&self) -> io::Result<()> {
        self.scan()?;

        while !self.notifier.is_closed() {
            let state = self.notifier.borrow().try_clone()?;
            let new_state = match state {
                SpotifyState::Stopped => self.listen_stopped().await?,
                SpotifyState::Running(info) => self.listen_running(info).await?,
            };

            if let Some(new_state) = new_state {
                self.change_state(new_state);
            } else {
                break;
            }
        }

        Ok(())
    }

    pub fn scan(&self) -> io::Result<()> {
        for process in OwnedProcess::all() {
            if !is_spotify_process(process.borrowed()) {
                continue;
            }

            let mut windows = list_process_windows(process.borrowed())?;
            while let Some(window) = windows.next()? {
                if is_main_spotify_window(window) {
                    drop(windows);
                    self.change_state(SpotifyState::Running(SpotifyInfo {
                        process,
                        main_window: window,
                    }));
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    fn change_state(&self, new_state: SpotifyState) {
        let _ = self.notifier.send(new_state);
    }

    async fn listen_stopped(&self) -> io::Result<Option<SpotifyState>> {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

        let event_hook = WindowEventHook::hook(
            EventFilter::default()
                .all_processes()
                .all_threads()
                .skip_own_thread(true)
                .skip_own_process(true)
                .event(raw_event::OBJECT_SHOW)
                .predicate(|event| {
                    event.child_id().is_none() && event.object_type() == AccessibleObjectId::Window
                }),
            event_tx,
        )
        .await?;

        while let Some(event) = event_rx.recv().await {
            // scoped to make future Send
            let state = {
                let Some(window) = event.window_handle() else {
                    continue;
                };
                let Ok(process) = get_window_process(window) else {
                    continue;
                };
                if !is_spotify_process(process.borrowed()) || !is_main_spotify_window(window) {
                    continue;
                }

                SpotifyState::Running(SpotifyInfo {
                    process,
                    main_window: window,
                })
            };

            event_hook.unhook().await?;
            return Ok(Some(state));
        }

        event_hook.unhook().await?;
        Ok(None)
    }

    async fn listen_running(&self, info: SpotifyInfo) -> io::Result<Option<SpotifyState>> {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

        let thread_id = get_window_thread_id(info.main_window);
        let process_id = info.process.pid()?;
        let event_hook = WindowEventHook::hook(
            EventFilter::default()
                .thread(thread_id)
                .process(process_id)
                .skip_own_thread(true)
                .skip_own_process(true)
                .event(raw_event::OBJECT_DESTROY)
                .predicate(|event| {
                    event.child_id().is_none() && event.object_type() == AccessibleObjectId::Window
                }),
            event_tx,
        )
        .await?;

        let new_state = loop {
            if let Some(event) = event_rx.recv().await {
                assert_eq!(event.thread_id(), thread_id.get());
                if event.window_handle() != Some(info.main_window) {
                    continue;
                }

                break Some(SpotifyState::Stopped);
            } else {
                break None;
            }
        };

        event_hook.unhook().await?;
        Ok(new_state)
    }
}

fn get_window_title_length(window: WindowHandle) -> io::Result<Option<NonZeroUsize>> {
    unsafe { SetLastError(0) };
    let result = unsafe { GetWindowTextLengthW(window.as_ptr()) };
    if result == 0 && unsafe { GetLastError() } != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(NonZeroUsize::new(result as usize))
    }
}

fn get_window_title(window: WindowHandle) -> io::Result<Option<String>> {
    let text_len = if let Some(length) = get_window_title_length(window)? {
        length.get()
    } else {
        return Ok(None);
    };

    let mut text = Vec::with_capacity(text_len + 1); // +1 for null terminator
    let result =
        unsafe { GetWindowTextW(window.as_ptr(), text.as_mut_ptr(), text.capacity() as i32) };
    if result != 0 {
        unsafe { text.set_len(text_len) };
        let text = String::from_utf16_lossy(&text);
        Ok(Some(text))
    } else {
        Err(io::Error::last_os_error())
    }
}

fn get_window_process(window: WindowHandle) -> io::Result<OwnedProcess> {
    let process_id = get_window_process_id(window);
    OwnedProcess::from_pid(process_id.get())
}

fn get_window_thread_id(window: WindowHandle) -> NonZeroU32 {
    let thread_id = unsafe { GetWindowThreadProcessId(window.as_ptr(), ptr::null_mut()) };
    NonZeroU32::new(thread_id).unwrap()
}

fn get_window_process_id(window: WindowHandle) -> NonZeroU32 {
    let mut process_id = MaybeUninit::uninit();
    let _thread_id = unsafe { GetWindowThreadProcessId(window.as_ptr(), process_id.as_mut_ptr()) };
    NonZeroU32::new(unsafe { process_id.assume_init() }).unwrap()
}

fn is_spotify_process(process: impl Process) -> bool {
    match process.base_name() {
        Ok(mut name) => {
            name.make_ascii_lowercase();
            name.to_string_lossy().contains("spotify")
        }
        Err(_) => false,
    }
}

fn is_main_spotify_window(window: WindowHandle) -> bool {
    let title = match get_window_title(window) {
        Ok(Some(title)) => title,
        _ => return false,
    };

    if title.trim().is_empty() || title == "G" || title == "Default IME" {
        return false;
    }

    let class_name = match get_window_class_name(window) {
        Ok(class_name) => class_name,
        _ => return false,
    };
    info!("Found window '{title}' of class '{class_name}'.");
    class_name.starts_with("Chrome_WidgetWin")
        || class_name == "Chrome_RenderWidgetHostHWND"
        || class_name == "GDI+ Hook Window Class"
}

fn get_window_class_name(window: WindowHandle) -> io::Result<String> {
    let mut class_name_buf = [const { MaybeUninit::uninit() }; 256];
    let result = unsafe {
        GetClassNameW(
            window.as_ptr(),
            class_name_buf[0].as_mut_ptr(),
            class_name_buf.len() as i32,
        )
    };
    match result {
        0 => Err(io::Error::last_os_error()),
        name_len => {
            let name_len = name_len as usize;
            let class_name = unsafe { class_name_buf[..name_len].assume_init_ref() };
            Ok(String::from_utf16_lossy(class_name))
        }
    }
}

fn list_threads() -> io::Result<impl FallibleIterator<Item = THREADENTRY32, Error = io::Error>> {
    Toolhelp32ThreadIterator::new()
}

fn list_process_threads(
    process: impl Process,
) -> io::Result<impl FallibleIterator<Item = u32, Error = io::Error>> {
    let process_id = process.pid()?.get();
    list_threads().map(move |iter| {
        iter.filter(move |thread| Ok(thread.th32OwnerProcessID == process_id))
            .map(|thread| Ok(thread.th32ThreadID))
    })
}

fn list_process_windows(
    process: impl Process,
) -> io::Result<impl FallibleIterator<Item = WindowHandle, Error = io::Error>> {
    list_process_threads(process).map(|iter| {
        iter.flat_map(|thread| {
            Ok(fallible_iterator::convert(
                list_thread_windows(thread, true).into_iter().map(Ok),
            ))
        })
    })
}

struct Toolhelp32ThreadIterator {
    snapshot: OwnedHandle,
    first: bool,
}

impl Toolhelp32ThreadIterator {
    pub fn new() -> io::Result<Self> {
        let snapshot = unsafe {
            CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0 /* ignored for SNAPTHREAD */)
        };
        let snapshot = unsafe { HandleOrInvalid::from_raw_handle(snapshot) };
        let snapshot: OwnedHandle = snapshot
            .try_into()
            .map_err(|_| io::Error::last_os_error())?;

        Ok(Toolhelp32ThreadIterator {
            snapshot,
            first: true,
        })
    }
}

impl FallibleIterator for Toolhelp32ThreadIterator {
    type Item = THREADENTRY32;
    type Error = io::Error;

    fn next(&mut self) -> io::Result<Option<Self::Item>> {
        let mut thread = MaybeUninit::<THREADENTRY32>::uninit();
        partial_init!(thread => {
            dwSize: mem::size_of::<THREADENTRY32>() as u32
        });

        let result = if self.first {
            self.first = false;
            unsafe { Thread32First(self.snapshot.as_raw_handle(), thread.as_mut_ptr()) }
        } else {
            unsafe { Thread32Next(self.snapshot.as_raw_handle(), thread.as_mut_ptr()) }
        };
        if result == FALSE {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                return Ok(None);
            } else {
                return Err(err);
            }
        }

        let thread = unsafe { thread.assume_init() };
        Ok(Some(thread))
    }
}

fn list_thread_windows(thread_id: u32, include_children: bool) -> Vec<WindowHandle> {
    extern "system" fn enum_proc(window_handle: HWND, windows: isize) -> BOOL {
        let windows = unsafe { &mut *(windows as *mut Vec<WindowHandle>) };
        windows.push(unsafe { WindowHandle::new_unchecked(window_handle) });
        FALSE
    }

    let mut windows = Vec::<WindowHandle>::new();
    let _result =
        unsafe { EnumThreadWindows(thread_id, Some(enum_proc), &mut windows as *mut _ as isize) };

    if include_children {
        let root_window_count = windows.len();
        for i in 0..root_window_count {
            let window = windows[i];
            unsafe {
                EnumChildWindows(
                    window.as_ptr(),
                    Some(enum_proc),
                    &mut windows as *mut _ as isize,
                )
            };
        }
    }

    windows
}
