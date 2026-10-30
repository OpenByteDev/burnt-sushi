use std::{
    mem, ptr,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use native_windows_derive as nwd;
use native_windows_gui as nwg;

use log::{debug, warn};
use nwd::NwgUi;
use nwg::NativeUi;
use widestring::u16cstr;
use windows_sys::Win32::{
    Foundation::{FALSE, HWND},
    System::Threading::GetCurrentThreadId,
    UI::{
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_MODIFY, NOTIFYICON_VERSION_4,
            NOTIFYICONDATAW, Shell_NotifyIconW,
        },
        WindowsAndMessaging::{
            ChangeWindowMessageFilterEx, HICON, MSGFLT_ALLOW, PostThreadMessageW,
            RegisterWindowMessageW, WM_QUIT, WM_USER,
        },
    },
};

use crate::{
    APP_NAME,
    logger::{self, Console},
};

static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Id of the raw event handler watching for the taskbar being recreated.
/// `nwg` reserves the ids up to `0xFFFF` for itself.
const TASKBAR_CREATED_HANDLER_ID: usize = 0x1_0000;

/// The callback message `nwg` adds its tray icons with (`NWG_TRAY`), which it does not export.
/// Adding the icon with the same message keeps clicks routed to the `nwg` events.
const NWG_TRAY_MESSAGE: u32 = WM_USER + 102;

/// How often adding the icon is tried at startup before waiting for the taskbar instead.
const ADD_ICON_ATTEMPTS: u32 = 5;
const ADD_ICON_RETRY_DELAY: Duration = Duration::from_secs(1);

pub struct SystemTrayManager {
    ui_thread: Option<thread::JoinHandle<()>>,
    exit_recv: tokio::sync::watch::Receiver<bool>,
    thread_id: u32,
}

impl SystemTrayManager {
    pub async fn build_and_run() -> Result<Self, nwg::NwgError> {
        if !INITIALIZED.swap(true, Ordering::SeqCst) {
            nwg::init()?;
        }

        let (start_tx, start_rx) = tokio::sync::oneshot::channel();
        let (exit_tx, exit_rx) = tokio::sync::watch::channel(false);

        let ui_thread = thread::spawn(move || {
            let tray_icon = match SystemTrayIcon::build_ui(SystemTrayIcon::default()) {
                Ok(tray_icon) => tray_icon,
                Err(err) => {
                    start_tx.send(Err(err)).unwrap();
                    exit_tx.send(true).unwrap();
                    return;
                }
            };
            let taskbar_handler = tray_icon.watch_taskbar();

            let thread_id = unsafe { GetCurrentThreadId() };
            start_tx.send(Ok(thread_id)).unwrap();

            tray_icon.ensure_icon_added();

            nwg::dispatch_thread_events();

            // Unbound while the window still exists, which `nwg` requires.
            if let Some(handler) = taskbar_handler {
                let _ = nwg::unbind_raw_event_handler(&handler);
            }

            exit_tx.send(true).unwrap();
        });

        Ok(Self {
            ui_thread: Some(ui_thread),
            thread_id: start_rx.await.unwrap()?,
            exit_recv: exit_rx,
        })
    }

    pub async fn wait_for_exit(&mut self) {
        if self.ui_thread.is_none() || *self.exit_recv.borrow() {
            return;
        }

        self.exit_recv.changed().await.unwrap();

        if let Some(ui_thread) = self.ui_thread.take() {
            ui_thread.join().unwrap();
        }
    }

    pub async fn exit(mut self) {
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        self.wait_for_exit().await;
    }
}

#[derive(NwgUi, Default)]
pub struct SystemTrayIcon {
    // A hidden top-level window rather than a message-only one, which would never receive
    // the `TaskbarCreated` broadcast.
    #[nwg_control(flags: "POPUP")]
    window: nwg::Window,

    #[nwg_resource]
    embed: nwg::EmbedResource,

    #[nwg_resource(source_embed: Some(&data.embed), source_embed_str: Some("TRAYICON"))]
    icon: nwg::Icon,

    #[nwg_control(icon: Some(&data.icon), tip: Some(APP_NAME))]
    #[nwg_events(MousePressLeftUp: [SystemTrayIcon::show_menu], OnContextMenu: [SystemTrayIcon::show_menu])]
    tray: nwg::TrayNotification,

    #[nwg_control(parent: window, popup: true)]
    tray_menu: nwg::Menu,

    #[nwg_control(parent: tray_menu, text: "Show Console")]
    #[nwg_events(OnMenuItemSelected: [SystemTrayIcon::show_console])]
    tray_item2: nwg::MenuItem,

    #[nwg_control(parent: tray_menu, text: "Exit")]
    #[nwg_events(OnMenuItemSelected: [SystemTrayIcon::exit])]
    tray_item3: nwg::MenuItem,
}

#[allow(clippy::unused_self)]
impl SystemTrayIcon {
    fn exit(&self) {
        nwg::stop_thread_dispatch();
    }

    fn show_menu(&self) {
        let (x, y) = nwg::GlobalCursor::position();

        let log = logger::global::get();
        let has_console = log.console.is_some();
        self.tray_item2.set_enabled(!has_console);
        self.tray_menu.popup(x, y);
    }

    fn show_console(&self) {
        let mut l = logger::global::get();
        if l.console.is_none() {
            l.console = Some(Console::piped().unwrap());
        }
    }

    fn icon_data(&self) -> Option<TrayIconData> {
        Some(TrayIconData {
            window: self.window.handle.hwnd()?.cast(),
            icon: self.icon.handle.cast(),
        })
    }

    /// Makes sure the icon made it into the tray. `nwg` adds it once and ignores a failure,
    /// which happens while Explorer is busy or not running yet, e.g. right after logging in.
    fn ensure_icon_added(&self) {
        let Some(data) = self.icon_data() else {
            return;
        };

        for attempt in 1..=ADD_ICON_ATTEMPTS {
            if data.ensure_added() {
                return;
            }
            if attempt < ADD_ICON_ATTEMPTS {
                thread::sleep(ADD_ICON_RETRY_DELAY);
            }
        }

        warn!("Failed to add the tray icon, it will be added once the taskbar is available");
    }

    /// Adds the icon again whenever the taskbar is recreated, which removes every icon from
    /// the tray. Explorer does this when it restarts, e.g. after crashing.
    fn watch_taskbar(&self) -> Option<nwg::RawEventHandler> {
        let data = self.icon_data()?;

        let message = unsafe { RegisterWindowMessageW(u16cstr!("TaskbarCreated").as_ptr()) };
        if message == 0 {
            warn!("Failed to register the TaskbarCreated message");
            return None;
        }

        // Lets the message through when running elevated, it is sent by the unelevated Explorer.
        unsafe { ChangeWindowMessageFilterEx(data.window, message, MSGFLT_ALLOW, ptr::null_mut()) };

        let handler = nwg::bind_raw_event_handler(
            &self.window.handle,
            TASKBAR_CREATED_HANDLER_ID,
            move |_, msg, _, _| {
                if msg == message {
                    debug!("Taskbar was recreated, adding the tray icon again");
                    if !data.ensure_added() {
                        warn!("Failed to add the tray icon again after the taskbar was recreated");
                    }
                }
                None
            },
        );

        match handler {
            Ok(handler) => Some(handler),
            Err(e) => {
                warn!("Failed to watch for the taskbar being recreated: {e}");
                None
            }
        }
    }
}

/// What is needed to add the tray icon without `nwg`. It is added the way `nwg` adds it, with
/// the same window, id and callback message, so that `nwg` keeps managing it afterwards.
#[derive(Clone, Copy)]
struct TrayIconData {
    window: HWND,
    icon: HICON,
}

impl TrayIconData {
    /// Adds the icon unless it is already in the tray. Returns whether it is in the tray now.
    fn ensure_added(self) -> bool {
        let mut data = self.notify_icon_data();

        // Only succeeds if the icon is already in the tray, in which case it is left as is.
        data.uFlags = NIF_ICON;
        if unsafe { Shell_NotifyIconW(NIM_MODIFY, &raw const data) } != FALSE {
            return true;
        }

        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
        unsafe { Shell_NotifyIconW(NIM_ADD, &raw const data) != FALSE }
    }

    fn notify_icon_data(self) -> NOTIFYICONDATAW {
        // An all zero `NOTIFYICONDATAW` is valid, it only holds integers, pointers and arrays.
        let mut data: NOTIFYICONDATAW = unsafe { mem::zeroed() };
        data.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.window;
        data.uCallbackMessage = NWG_TRAY_MESSAGE;
        data.hIcon = self.icon;
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        for (dst, src) in data.szTip.iter_mut().zip(APP_NAME.encode_utf16()) {
            *dst = src;
        }
        data
    }
}
