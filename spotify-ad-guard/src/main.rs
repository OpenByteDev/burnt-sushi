#![feature(io_safety, once_cell)]
#![warn(unsafe_op_in_unsafe_fn)]

use std::{
    fs, mem,
    net::SocketAddrV4,
    path::{Path, PathBuf},
};

use dll_syringe::{
    error::SyringeError,
    process::{OwnedProcessModule, Process},
    Syringe,
};
use log::{info, warn};
use spotify_process_scanner::{SpotifyInfo, SpotifyProcessScanner};
use tokio::{runtime, task::LocalSet};

use crate::spotify_process_scanner::SpotifyState;

mod rpc;
mod spotify_process_scanner;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("Spotify Ad Guard v{}", env!("CARGO_PKG_VERSION"));

    setup_logging();
    App::new().run().await;
}

struct App {
    payload: PayloadInfo,
    scanner: SpotifyProcessScanner,
    spotify_state: tokio::sync::watch::Receiver<SpotifyState>,
    filter_config: PathBuf,
    state: AppState,
}

struct PayloadInfo {
    path: PathBuf,
}

#[allow(clippy::large_enum_variant)]
enum AppState {
    Hooked(HookState),
    Unhooked,
}

struct HookState {
    syringe: Syringe,
    payload: OwnedProcessModule,
    rpc_task: async_thread::JoinHandle<()>,
}

fn setup_logging() {
    simple_logger::init_with_level(log::Level::Info).unwrap();
}

fn prepare_payload() -> PayloadInfo {
    info!("Start preparing injection payload");

    let payload_path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("inject_payload_x86.dll");

    let payload_bytes = include_bytes!(concat!(env!("OUT_DIR"), "\\inject_payload_x86.dll"));

    info!("Write injection payload to '{}'", payload_path.display());
    fs::write(&payload_path, payload_bytes).unwrap();

    PayloadInfo { path: payload_path }
}

fn prepare_config() -> PathBuf {
    let path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("filter.toml");

    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "\\filter.toml"));

    fs::write(&path, bytes).unwrap();

    path
}

impl App {
    fn new() -> Self {
        let payload = prepare_payload();
        let filter_config = prepare_config();

        let (scanner, spotify_state) = SpotifyProcessScanner::new();
        Self {
            payload,
            scanner,
            spotify_state,
            filter_config,
            state: AppState::Unhooked,
        }
    }

    async fn run(mut self) {
        println!("Looking for Spotify...");
        tokio::select! {
            _ = self.scanner.run() => {}
            _ = async {
                while self.spotify_state.changed().await.is_ok() {
                    let state = self.spotify_state.borrow();
                    match *state {
                        SpotifyState::Running(ref spotify) => {
                            self.state.hook_spotify(spotify.try_clone().unwrap(), &self.payload, &self.filter_config).await;
                        },
                        SpotifyState::Stopped => {
                            self.state.unhook_spotify().await;
                        }
                    }
                }
            } => {}
        }
    }
}

impl AppState {
    async fn hook_spotify(
        &mut self,
        spotify: SpotifyInfo,
        payload: &PayloadInfo,
        filter_config: &Path,
    ) {
        if let AppState::Hooked(_) = self {
            self.unhook_spotify().await;
        }

        println!("Found Spotify (PID={})", spotify.process.pid().unwrap());
        let syringe = Syringe::for_process(spotify.process);

        while let Some(prev_payload) = syringe
            .process()
            .find_module_by_name("inject_payload_x86.dll") // TODO:
            .unwrap()
        {
            info!("Found previously injected blocker");

            info!("Stopping RPC of previous blocker");
            let stop_rpc = syringe
                .get_procedure::<(), ()>(prev_payload, "stop_rpc")
                .unwrap()
                .unwrap();
            stop_rpc.call(&()).unwrap();

            info!("Ejecting previous blocker...");
            syringe.eject(prev_payload).unwrap();

            info!("Ejectied previous blocker");
        }

        println!("Injecting blocker...");
        let payload = syringe.inject(&payload.path).unwrap();

        println!("Starting RPC...");
        let start_rpc = syringe
            .get_procedure::<(), SocketAddrV4>(payload, "start_rpc")
            .unwrap()
            .unwrap();

        let rpc_socket_addr = start_rpc.call(&()).unwrap();

        let filter_config = filter_config.to_path_buf();
        let rpc_task = async_thread::spawn(move || {
            let rt = runtime::Builder::new_current_thread().enable_all().build().unwrap();
            let localset = LocalSet::new();
            localset.block_on(&rt, async move {
                rpc::run(rpc_socket_addr, filter_config).await.unwrap();
            });
        });

        println!("Blocker up and running");
        *self = AppState::Hooked(HookState {
            payload: payload.try_to_owned().unwrap(),
            syringe,
            rpc_task,
        });

        println!("Hooked!");
    }

    async fn unhook_spotify(&mut self) {
        let state = mem::replace(self, AppState::Unhooked);
        let state = match state {
            AppState::Hooked(state) => state,
            _ => return,
        };

        println!("Unhooking Spotify...");

        let result: Result<(), SyringeError> = async {
            let stop_rpc = state
                .syringe
                .get_procedure::<(), ()>(state.payload.borrowed(), "stop_rpc")?
                .unwrap();

            println!("Stopping RPC...");
            stop_rpc.call(&())?;
            state.rpc_task.join().await.unwrap();
            println!("Stopped RPC");

            if state.payload.process().is_alive() {
                println!("Ejecting blocker...");
                state.syringe.eject(state.payload.borrowed())?;
                println!("Ejected blocker");
            }

            Ok(())
        }
        .await;

        match result {
            Ok(_) | Err(SyringeError::ProcessInaccessible) => {}
            _ => todo!("{:#?}", result),
        };

        *self = AppState::Unhooked;

        println!("Unhooked!");
    }
}
