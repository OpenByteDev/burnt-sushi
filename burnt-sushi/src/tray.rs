use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

use native_windows_derive as nwd;
use native_windows_gui as nwg;

use nwd::NwgUi;
use nwg::NativeUi;
use winapi::um::{
    processthreadsapi::GetCurrentThreadId,
    winuser::{PostThreadMessageW, WM_QUIT},
};

use crate::{
    logger::{self, Console},
    APP_NAME,
};

static INITIALIZED: AtomicBool = AtomicBool::new(false);

pub struct SystemTrayManager {
    ui_thread: Option<thread::JoinHandle<()>>,
    ui_thread_exit: tokio::sync::watch::Receiver<bool>,
    ui_thread_id: u32,
}

impl SystemTrayManager {
    pub async fn build_and_run() -> Result<Self, nwg::NwgError> {
        if !INITIALIZED.swap(true, Ordering::SeqCst) {
            nwg::init()?;
        }

        let (start_tx, start_rx) = tokio::sync::oneshot::channel();
        let (exit_tx, exit_rx) = tokio::sync::watch::channel(false);

        let ui_thread = thread::spawn(move || {
            let _tray_icon = match SystemTrayIcon::build_ui(SystemTrayIcon::default()) {
                Ok(tray_icon) => tray_icon,
                Err(err) => {
                    start_tx.send(Err(err)).unwrap();
                    exit_tx.send(true).unwrap();
                    return;
                }
            };

            let thread_id = unsafe { GetCurrentThreadId() };
            start_tx.send(Ok(thread_id)).unwrap();

            nwg::dispatch_thread_events();

            exit_tx.send(true).unwrap();
        });

        Ok(Self {
            ui_thread: Some(ui_thread),
            ui_thread_id: start_rx.await.unwrap()?,
            ui_thread_exit: exit_rx,
        })
    }

    pub async fn wait_for_exit(&mut self) {
        if self.ui_thread.is_none() || *self.ui_thread_exit.borrow() {
            return;
        }

        self.ui_thread_exit.changed().await.unwrap();

        if let Some(ui_thread) = self.ui_thread.take() {
            ui_thread.join().unwrap();
        }
    }

    pub async fn exit(mut self) {
        unsafe { PostThreadMessageW(self.ui_thread_id, WM_QUIT, 0, 0) };
        self.wait_for_exit().await
    }
}

#[derive(NwgUi, Default)]
pub struct SystemTrayIcon {
    #[nwg_control]
    window: nwg::MessageWindow,

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
}
