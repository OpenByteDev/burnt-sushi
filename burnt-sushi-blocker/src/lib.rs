#![feature(once_cell, io_safety)]

use std::{
    cell::RefCell,
    lazy::{OnceCell, SyncLazy},
    net::{Ipv4Addr, SocketAddrV4},
    sync::{Arc, Mutex},
    thread,
};

use capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};
use enum_map::EnumMap;
use futures::{AsyncReadExt, FutureExt};
use hooks::LogParams;
use regex::Regex;
use tokio::select;

mod cef;
mod hooks;
mod utils;

static RPC_STATE: SyncLazy<Mutex<Option<RpcState>>> = SyncLazy::new(|| Mutex::new(None));

struct RpcState {
    rpc_thread: thread::JoinHandle<()>,
    rpc_disconnector: tokio::sync::watch::Sender<()>,
    socket_addr: SocketAddrV4,
}

dll_syringe::payload_procedure! {
    fn start_rpc() -> SocketAddrV4 {
        let mut state = RPC_STATE.lock().unwrap();
        if let Some(state) = state.as_ref() {
            return state.socket_addr;
        }

        let (end_point_tx, end_point_rx) = tokio::sync::oneshot::channel();
        let (disconnect_tx, disconnect_rx) = tokio::sync::watch::channel(());

        let rpc_thread = thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
                .block_on(tokio::task::LocalSet::new().run_until(run_rpc(end_point_tx, disconnect_rx)))
                .unwrap()
        });

        let socket_addr = end_point_rx.blocking_recv().unwrap();

        *state = Some(RpcState {
            rpc_thread,
            rpc_disconnector: disconnect_tx,
            socket_addr,
        });

        socket_addr
    }
}

dll_syringe::payload_procedure! {
    fn stop_rpc() {
        let mut state = RPC_STATE.lock().unwrap();
        if let Some(state) = state.take() {
            state.rpc_disconnector.send(()).unwrap();
            state.rpc_thread.join().unwrap();
        }
    }
}

async fn run_rpc(
    end_point: tokio::sync::oneshot::Sender<SocketAddrV4>,
    mut disconnect_signal: tokio::sync::watch::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).await?;
    end_point
        .send(SocketAddrV4::new(
            Ipv4Addr::LOCALHOST,
            listener.local_addr()?.port(),
        ))
        .unwrap();
    let client: shared::rpc::blocker_service::Client = capnp_rpc::new_client(ServerImpl::new());

    loop {
        select! {
            res = listener.accept() => {
                let stream = match res {
                    Ok((stream, _)) => stream,
                    Err(e) => return Err(e.into()),
                };

                stream.set_nodelay(true)?;
                let (reader, writer) =
                    tokio_util::compat::TokioAsyncReadCompatExt::compat(stream).split();
                let network = twoparty::VatNetwork::new(
                    reader,
                    writer,
                    rpc_twoparty_capnp::Side::Server,
                    Default::default(),
                );

                let rpc_system = RpcSystem::new(Box::new(network), Some(client.clone().client));

                let disconnector = rpc_system.get_disconnector();
                let mut disconnect_signal = disconnect_signal.clone();
                tokio::task::spawn_local(async move {
                    disconnect_signal.changed().await.unwrap();
                    let _ = hooks::disable();
                    disconnector.await.unwrap();
                });

                tokio::task::spawn_local(Box::pin(rpc_system.map(|_| ())));
            },
            _ = disconnect_signal.changed() => {
                return Ok(());
            }
        }
    }
}

#[derive(Clone)]
struct LoggerManager {
    loggers: RefCell<Vec<shared::rpc::blocker_service::logger::Client>>,
    log_tx: OnceCell<tokio::sync::mpsc::UnboundedSender<LogParams>>,
}

impl LoggerManager {
    fn new() -> Self {
        Self {
            loggers: RefCell::new(Vec::new()),
            log_tx: OnceCell::new(),
        }
    }

    fn add_logger(&self, logger: shared::rpc::blocker_service::logger::Client) {
        self.loggers.borrow_mut().push(logger);
    }

    #[allow(clippy::await_holding_refcell_ref)] // Ref is dropped before await
    async fn log_request(
        &self,
        hook: shared::rpc::blocker_service::FilterHook,
        blocked: bool,
        url: &str,
    ) {
        let loggers = self.loggers.borrow();
        let futures = futures::future::join_all(loggers.iter().map(|logger| {
            let mut req = logger.log_request_request();
            let mut builder = req.get().init_request();
            builder.set_hook(hook);
            builder.set_blocked(blocked);
            builder.set_url(url.as_ref());
            req.send().promise
        }));
        drop(loggers);
        futures.await;
    }

    #[allow(clippy::await_holding_refcell_ref)] // Ref is dropped before await
    async fn log_message(&self, message: &str) {
        let loggers = self.loggers.borrow();
        let futures = futures::future::join_all(loggers.iter().map(|logger| {
            let mut req = logger.log_message_request();
            req.get().set_message(message.as_ref());
            req.send().promise
        }));
        drop(loggers);
        futures.await;
    }

    fn log_sender(&self) -> tokio::sync::mpsc::UnboundedSender<LogParams> {
        self.log_tx.get_or_init(|| self.spawn_log_channel()).clone()
    }

    fn spawn_log_channel(&self) -> tokio::sync::mpsc::UnboundedSender<LogParams> {
        let this = self.clone();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::task::spawn_local(async move {
            loop {
                while let Some(m) = rx.recv().await {
                    match m {
                        LogParams::Request { hook, blocked, url } => {
                            this.log_request(hook, blocked, &url).await;
                        }
                        LogParams::Message(message) => {
                            this.log_message(&message).await;
                        }
                    }
                }
            }
        });

        tx
    }
}

#[derive(Debug, Clone, Default)]
pub struct FilterRuleset {
    whitelist: Vec<Regex>,
    blacklist: Vec<Regex>,
}

impl FilterRuleset {
    fn check(&self, request: &str) -> bool {
        (self.whitelist.is_empty() || self.whitelist.iter().any(|regex| regex.is_match(request)))
            && !self.blacklist.iter().any(|regex| regex.is_match(request))
    }
}

struct ServerImpl {
    logger: LoggerManager,
    filters: Arc<EnumMap<shared::rpc::blocker_service::FilterHook, FilterRuleset>>,
}

impl ServerImpl {
    fn new() -> Self {
        Self {
            logger: LoggerManager::new(),
            filters: Arc::new(EnumMap::default()),
        }
    }
}

impl shared::rpc::blocker_service::Server for ServerImpl {
    fn register_logger(
        &mut self,
        params: shared::rpc::blocker_service::RegisterLoggerParams,
        mut _results: shared::rpc::blocker_service::RegisterLoggerResults,
    ) -> Promise<(), ::capnp::Error> {
        self.logger
            .add_logger(pry!(pry!(params.get()).get_logger()));

        Promise::ok(())
    }

    fn set_ruleset(
        &mut self,
        params: shared::rpc::blocker_service::SetRulesetParams,
        mut _results: shared::rpc::blocker_service::SetRulesetResults,
    ) -> Promise<(), ::capnp::Error> {
        pry!((move || {
            let hook = params.get()?.get_hook()?;
            let raw_ruleset = params.get()?.get_ruleset()?;
            let whitelist = raw_ruleset.get_whitelist()?;
            let blacklist = raw_ruleset.get_blacklist()?;

            let ruleset = &mut Arc::get_mut(&mut self.filters).ok_or_else(|| {
                ::capnp::Error::failed("cannot modify filters while in use".to_string())
            })?[hook];
            for i in 0..whitelist.len() {
                ruleset.whitelist.push(
                    Regex::new(whitelist.get(i)?)
                        .map_err(|e| capnp::Error::failed(e.to_string()))?,
                );
            }
            for i in 0..blacklist.len() {
                ruleset.blacklist.push(
                    Regex::new(blacklist.get(i)?)
                        .map_err(|e| capnp::Error::failed(e.to_string()))?,
                );
            }

            Ok::<(), capnp::Error>(())
        })());

        Promise::ok(())
    }

    fn enable_filtering(
        &mut self,
        _params: shared::rpc::blocker_service::EnableFilteringParams,
        mut _results: shared::rpc::blocker_service::EnableFilteringResults,
    ) -> Promise<(), ::capnp::Error> {
        match hooks::enable(self.filters.clone(), self.logger.log_sender()) {
            Ok(()) => Promise::ok(()),
            Err(e) => Promise::err(capnp::Error::failed(e.to_string())),
        }
    }

    fn disable_filtering(
        &mut self,
        _params: shared::rpc::blocker_service::DisableFilteringParams,
        mut _results: shared::rpc::blocker_service::DisableFilteringResults,
    ) -> Promise<(), ::capnp::Error> {
        match hooks::disable() {
            Ok(()) => Promise::ok(()),
            Err(e) => Promise::err(capnp::Error::failed(e.to_string())),
        }
    }
}
