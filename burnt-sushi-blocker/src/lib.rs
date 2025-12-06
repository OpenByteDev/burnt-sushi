#![feature(once_cell_try)]

use std::{
    cell::{OnceCell, RefCell},
    net::{Ipv4Addr, SocketAddrV4},
    rc::Rc,
    sync::{LazyLock, Mutex},
    thread,
};

use capnp::capability::Promise;
use capnp_rpc::{RpcSystem, pry, rpc_twoparty_capnp, twoparty};
use dll_syringe::payload_utils::payload_procedure;
use futures::{AsyncReadExt, FutureExt};
use hooks::LogParams;
use regex::RegexSet;
use tokio::select;

mod cef;
mod filters;
mod hooks;
mod utils;
pub use filters::*;

static RPC_STATE: LazyLock<Mutex<Option<RpcState>>> = LazyLock::new(|| Mutex::new(None));

struct RpcState {
    rpc_thread: thread::JoinHandle<()>,
    rpc_disconnector: tokio::sync::watch::Sender<()>,
    socket_addr: SocketAddrV4,
}

#[payload_procedure]
fn start_rpc() -> SocketAddrV4 {
    let mut state = RPC_STATE.lock().unwrap();
    if let Some(state) = state.as_ref() {
        return state.socket_addr;
    }

    let (end_point_tx, end_point_rx) = tokio::sync::oneshot::channel();
    let (disconnect_tx, disconnect_rx) = tokio::sync::watch::channel(());

    let rpc_thread = thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
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

#[payload_procedure]
fn stop_rpc() {
    let mut state = RPC_STATE.lock().unwrap();
    if let Some(state) = state.take() {
        hooks::disable().unwrap();
        state.rpc_disconnector.send(()).unwrap();
        state.rpc_thread.join().unwrap();
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
            builder.set_url(url);
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
            req.get().set_message(message);
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

struct ServerImpl {
    logger: LoggerManager,
    filters: Filters,
}

impl ServerImpl {
    fn new() -> Self {
        Self {
            logger: LoggerManager::new(),
            filters: Filters::empty(),
        }
    }
}

impl shared::rpc::blocker_service::Server for ServerImpl {
    fn register_logger(
        self: Rc<Self>,
        params: shared::rpc::blocker_service::RegisterLoggerParams,
        mut _results: shared::rpc::blocker_service::RegisterLoggerResults,
    ) -> impl futures::Future<Output = Result<(), capnp::Error>> + 'static {
        self.logger
            .add_logger(pry!(pry!(params.get()).get_logger()));

        Promise::ok(())
    }

    fn set_ruleset(
        self: Rc<Self>,
        params: shared::rpc::blocker_service::SetRulesetParams,
        mut _results: shared::rpc::blocker_service::SetRulesetResults,
    ) -> impl futures::Future<Output = Result<(), capnp::Error>> + 'static {
        pry!((move || {
            let hook = params.get()?.get_hook()?;
            let raw_ruleset = params.get()?.get_ruleset()?;
            let whitelist = raw_ruleset.get_whitelist()?;
            let blacklist = raw_ruleset.get_blacklist()?;

            let whitelist = RegexSet::new(
                whitelist
                    .iter()
                    .map(|pattern| pattern.map(|p| String::from_utf8_lossy(p.as_bytes())))
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .map_err(|e| capnp::Error::failed(e.to_string()))?;
            let blacklist = RegexSet::new(
                blacklist
                    .iter()
                    .map(|pattern| pattern.map(|p| String::from_utf8_lossy(p.as_bytes())))
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .map_err(|e| capnp::Error::failed(e.to_string()))?;
            let ruleset = FilterRuleset {
                whitelist,
                blacklist,
            };
            self.filters.replace_ruleset(hook, ruleset);

            Ok::<(), capnp::Error>(())
        })());

        Promise::ok(())
    }

    fn enable_filtering(
        self: Rc<Self>,
        _params: shared::rpc::blocker_service::EnableFilteringParams,
        mut _results: shared::rpc::blocker_service::EnableFilteringResults,
    ) -> impl futures::Future<Output = Result<(), capnp::Error>> + 'static {
        match hooks::enable(self.filters.clone(), self.logger.log_sender()) {
            Ok(()) => Promise::ok(()),
            Err(e) => Promise::err(capnp::Error::failed(e.to_string())),
        }
    }

    fn disable_filtering(
        self: Rc<Self>,
        _params: shared::rpc::blocker_service::DisableFilteringParams,
        mut _results: shared::rpc::blocker_service::DisableFilteringResults,
    ) -> impl futures::Future<Output = Result<(), capnp::Error>> + 'static {
        match hooks::disable() {
            Ok(()) => Promise::ok(()),
            Err(e) => Promise::err(capnp::Error::failed(e.to_string())),
        }
    }
}
