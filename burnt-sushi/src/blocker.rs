use std::{mem, net::SocketAddrV4};

use anyhow::Context;
use dll_syringe::{
    error::SyringeError,
    process::{OwnedProcessModule, Process},
    Syringe,
};
use log::{debug, error, info, warn};
use serde::Deserialize;
use tokio::{runtime, task::LocalSet};

use crate::{
    args::ARGS,
    resolver::{resolve_blocker, resolve_filter_config},
    rpc,
    spotify_process_scanner::{SpotifyInfo, SpotifyProcessScanner, SpotifyState},
    DEFAULT_BLOCKER_FILE_NAME,
};

pub struct SpotifyAdBlocker {
    scanner: SpotifyProcessScanner,
    spotify_state: tokio::sync::watch::Receiver<SpotifyState>,
    state: SpotifyHookState,
}

#[allow(clippy::large_enum_variant)]
enum SpotifyHookState {
    Hooked(HookState),
    Unhooked,
}

struct HookState {
    syringe: Syringe,
    payload: OwnedProcessModule,
    rpc_task: async_thread::JoinHandle<()>,
}

impl SpotifyAdBlocker {
    pub fn new() -> Self {
        let (scanner, spotify_state) = SpotifyProcessScanner::new();
        Self {
            scanner,
            spotify_state,
            state: SpotifyHookState::Unhooked,
        }
    }

    pub async fn run(&mut self) {
        tokio::select! {
            _ = self.scanner.run() => {
                unreachable!("Spotify scanner should never stop on its own");
            }
            _ = async {
                info!("Looking for Spotify...");
                while self.spotify_state.changed().await.is_ok() {
                    let state = self.spotify_state.borrow();
                    match &*state {
                        SpotifyState::Running(spotify) => {
                            self.state.hook_spotify(spotify.try_clone().unwrap()).await.unwrap();
                        },
                        SpotifyState::Stopped => {
                            self.state.unhook_spotify().await;
                            if ARGS.shutdown_with_spotify {
                                info!("Shutting down due to spotify exit...");
                                break;
                            }
                            info!("Looking for Spotify...");
                        }
                    }
                }
            } => {}
        }
    }

    pub async fn stop(&mut self) {
        if matches!(self.state, SpotifyHookState::Hooked(_)) {
            self.state.unhook_spotify().await;
        }
    }
}

impl SpotifyHookState {
    async fn hook_spotify(&mut self, spotify: SpotifyInfo) -> anyhow::Result<()> {
        if let SpotifyHookState::Hooked(_) = self {
            self.unhook_spotify().await;
        }

        match spotify.process.pid().ok() {
            Some(pid) => info!("Found Spotify (PID={pid})"),
            None => info!("Found Spotify")
        }
        let syringe = Syringe::for_process(spotify.process);

        while let Some(prev_payload) = syringe
            .process()
            .find_module_by_name(DEFAULT_BLOCKER_FILE_NAME)
            .context("Failed to inspect modules of Spotify process.")?
        {
            warn!("Found previously injected blocker");

            debug!("Stopping RPC of previous blocker");
            let stop_rpc =
                unsafe { syringe.get_payload_procedure::<fn()>(prev_payload, "stop_rpc") }
                    .context("Failed to access spotify process.")?
                    .context("Failed to find stop_rpc in blocker module.")?;
            match stop_rpc.call() {
                Ok(_) => {
                    debug!("Stopped RPC of previous blocker");
                }
                Err(e) => {
                    error!("Failed to stop RPC of previous blocker: {}", e);
                }
            }

            info!("Ejecting previous blocker...");
            match syringe.eject(prev_payload) {
                Ok(_) => info!("Ejected previous blocker"),
                Err(_) => error!("Failed to eject previous blocker")
            };
        }

        info!("Loading filter config...");
        let filter_config = resolve_filter_config(ARGS.filters.as_ref().map(|p| p.as_ref()))
            .await
            .context("Failed to resolve filter config.")?;

        info!("Preparing blocker...");
        let payload_path = resolve_blocker(ARGS.blocker.as_ref().map(|p| p.as_ref()))
            .await
            .context("Failed to resolve blocker.")?;

        info!("Injecting blocker...");
        let payload = syringe.inject(payload_path)
            .context("Failed to inject blocker.")?;

        debug!("Starting RPC...");
        let start_rpc =
            unsafe { syringe.get_payload_procedure::<fn() -> SocketAddrV4>(payload, "start_rpc") }
                .context("Failed to access spotify process.")?
                .context("Failed to find start_rpc in blocker module.")?;

        let rpc_socket_addr = start_rpc.call().unwrap();

        let rpc_task = async_thread::spawn(move || {
            let rt = runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let localset = LocalSet::new();
            localset.block_on(&rt, async move {
                rpc::run(rpc_socket_addr, filter_config).await.unwrap();
            });
        });

        info!("Blocker up and running!");
        *self = SpotifyHookState::Hooked(HookState {
            payload: payload.try_to_owned().unwrap(),
            syringe,
            rpc_task,
        });

        Ok(())
    }

    async fn unhook_spotify(&mut self) {
        let state = mem::replace(self, SpotifyHookState::Unhooked);
        let state = match state {
            SpotifyHookState::Hooked(state) => state,
            _ => return,
        };

        info!("Unhooking Spotify...");

        let result: Result<(), SyringeError> = async {
            let stop_rpc = unsafe {
                state
                    .syringe
                    .get_payload_procedure::<fn()>(state.payload.borrowed(), "stop_rpc")
            }?
            .unwrap();

            debug!("Stopping RPC...");
            stop_rpc.call()?;
            state.rpc_task.join().await.unwrap();
            debug!("Stopped RPC");

            if state.payload.process().is_alive() {
                info!("Ejecting blocker...");
                state.syringe.eject(state.payload.borrowed())?;
                info!("Ejected blocker");
            }

            Ok(())
        }
        .await;

        match result {
            Ok(_)
            | Err(SyringeError::ProcessInaccessible)
            | Err(SyringeError::ModuleInaccessible) => {}
            _ => todo!("{:#?}", result),
        };

        *self = SpotifyHookState::Unhooked;
    }
}

#[derive(Deserialize, Debug)]
pub struct FilterConfig {
    pub allowlist: Vec<String>,
    pub denylist: Vec<String>,
}
