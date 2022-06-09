use ::capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::{AsyncReadExt, FutureExt};
use log::{debug, info};
use tokio::net::ToSocketAddrs;

use crate::FilterConfig;

struct LoggerImpl;

impl shared::rpc::blocker_service::logger::Server for LoggerImpl {
    fn log_request(
        &mut self,
        params: shared::rpc::blocker_service::logger::LogRequestParams,
        mut _results: shared::rpc::blocker_service::logger::LogRequestResults,
    ) -> Promise<(), ::capnp::Error> {
        let request = pry!(pry!(params.get()).get_request());

        let block_sign = if request.get_blocked() { '-' } else { '+' };
        let hook_name = pry!(request.get_hook());
        let url = pry!(request.get_url());

        debug!("[{}] ({}) {}", block_sign, hook_name, url);

        Promise::ok(())
    }

    fn log_message(
        &mut self,
        params: shared::rpc::blocker_service::logger::LogMessageParams,
        mut _results: shared::rpc::blocker_service::logger::LogMessageResults,
    ) -> Promise<(), ::capnp::Error> {
        let message = pry!(pry!(params.get()).get_message());
        info!("{}", message);

        Promise::ok(())
    }
}

pub async fn run(
    socket_addr: impl ToSocketAddrs,
    filter_config: FilterConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::task::LocalSet::new()
        .run_until(async move {
            let stream = tokio::net::TcpStream::connect(socket_addr).await?;
            info!("Connected to {}", stream.peer_addr()?);

            stream.set_nodelay(true)?;
            let (reader, writer) =
                tokio_util::compat::TokioAsyncReadCompatExt::compat(stream).split();
            let rpc_network = Box::new(twoparty::VatNetwork::new(
                reader,
                writer,
                rpc_twoparty_capnp::Side::Client,
                Default::default(),
            ));
            let mut rpc_system = RpcSystem::new(rpc_network, None);
            let client: shared::rpc::blocker_service::Client =
                rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);

            let rpc = tokio::task::spawn_local(Box::pin(rpc_system.map(|_| ())));

            let mut register_logger_request = client.register_logger_request();
            register_logger_request
                .get()
                .set_logger(capnp_rpc::new_client(LoggerImpl));
            register_logger_request.send().promise.await?;

            {
                let mut set_ruleset_request = client.set_ruleset_request();
                set_ruleset_request
                    .get()
                    .set_hook(shared::rpc::blocker_service::FilterHook::GetAddrInfo);
                let mut ruleset = set_ruleset_request.get().init_ruleset();
                let mut whitelist = ruleset
                    .reborrow()
                    .init_whitelist(filter_config.allowlist.len() as _);
                for (i, url) in filter_config.allowlist.iter().enumerate() {
                    whitelist.set(i as _, url);
                }
                let mut _blacklist = ruleset.reborrow().init_blacklist(0);
                set_ruleset_request.send().promise.await?;
            }

            {
                let mut set_ruleset_request = client.set_ruleset_request();
                set_ruleset_request
                    .get()
                    .set_hook(shared::rpc::blocker_service::FilterHook::CefUrlRequestCreate);
                let mut ruleset = set_ruleset_request.get().init_ruleset();
                let mut blacklist = ruleset
                    .reborrow()
                    .init_blacklist(filter_config.denylist.len() as _);
                for (i, url) in filter_config.denylist.iter().enumerate() {
                    blacklist.set(i as _, url);
                }
                let mut _whitelist = ruleset.reborrow().init_whitelist(0);
                set_ruleset_request.send().promise.await?;
            }

            let enable_filtering_request = client.enable_filtering_request();
            enable_filtering_request.send().promise.await?;

            rpc.await.map_err(|e| e.into())
        })
        .await
}
