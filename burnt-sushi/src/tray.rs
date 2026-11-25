use std::cell::RefCell;

use log::error;
use tokio::sync::oneshot;
use tray_icon::{
    Icon, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem},
};

use crate::{
    APP_NAME,
    console::Console,
    logger,
    message_loop::{MessageLoop, QuitHandle},
};

const SHOW_CONSOLE_ID: &str = "show-console";
const EXIT_ID: &str = "exit";

thread_local! {
    static TRAY_STATE: RefCell<Option<TrayState>> = const { RefCell::new(None) };
}

pub struct Tray {
    quit: QuitHandle,
    thread: Option<async_thread::JoinHandle<()>>,
}

impl Tray {
    pub async fn spawn(exit_tx: oneshot::Sender<()>) -> anyhow::Result<Self> {
        let (started_tx, started_rx) = oneshot::channel();

        let thread = async_thread::spawn(move || {
            let message_loop = MessageLoop::new();
            if let Err(err) = TrayState::install(exit_tx) {
                let _ = started_tx.send(Err(err));
                return;
            }
            let _ = started_tx.send(Ok(message_loop.quit_handle()));

            message_loop.run();

            TrayState::uninstall();
        });

        let quit = started_rx.await.unwrap()?;

        Ok(Self {
            quit,
            thread: Some(thread),
        })
    }

    pub async fn exit(mut self) {
        self.quit.quit();
        if let Some(thread) = self.thread.take() {
            thread.join().await.unwrap();
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.quit.quit();
        }
    }
}

struct TrayState {
    _icon: TrayIcon,
    show_console: MenuItem,
    exit_tx: Option<oneshot::Sender<()>>,
}

impl TrayState {
    fn install(exit_tx: oneshot::Sender<()>) -> anyhow::Result<()> {
        anyhow::ensure!(
            TRAY_STATE.with_borrow(Option::is_none),
            "A tray already exists on this thread"
        );

        let show_console = MenuItem::with_id(SHOW_CONSOLE_ID, "Show Console", true, None);
        let exit = MenuItem::with_id(EXIT_ID, "Exit", true, None);
        let menu = Menu::with_items(&[&show_console, &exit])?;

        let icon = TrayIconBuilder::new()
            .with_icon(Icon::from_resource_name("TRAYICON", None)?)
            .with_tooltip(APP_NAME)
            .with_menu(Box::new(menu))
            .build()?;

        TRAY_STATE.set(Some(Self {
            _icon: icon,
            show_console,
            exit_tx: Some(exit_tx),
        }));
        TrayIconEvent::set_event_handler(Some(on_tray_icon_event));
        MenuEvent::set_event_handler(Some(on_menu_event));

        Ok(())
    }

    fn uninstall() {
        drop(TRAY_STATE.take());
    }
}

fn on_tray_icon_event(event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        let has_console = logger::global::get().has_console();
        TRAY_STATE.with_borrow(|state| {
            if let Some(state) = state {
                state.show_console.set_enabled(!has_console);
            }
        });
    }
}

fn on_menu_event(event: MenuEvent) {
    match event.id.as_ref() {
        SHOW_CONSOLE_ID => show_console(),
        EXIT_ID => request_exit(),
        _ => {}
    }
}

fn show_console() {
    let mut logger = logger::global::get();
    if logger.has_console() {
        return;
    }

    match Console::piped() {
        Ok(console) => logger.console.attach(console),
        Err(e) => {
            drop(logger);
            error!("Failed to open console: {e:#}");
        }
    }
}

fn request_exit() {
    TRAY_STATE.with_borrow_mut(|state| {
        if let Some(exit_tx) = state.as_mut().and_then(|state| state.exit_tx.take()) {
            let _ = exit_tx.send(());
        }
    });
}
