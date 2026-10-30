//! Counts the requests the blocker refused during this run of the app.
//!
//! The counters live here rather than in the blocker so they survive Spotify restarting,
//! which tears down the injected module and its RPC connection.

use std::sync::atomic::{AtomicU64, Ordering};

use shared::rpc::blocker_service::FilterHook;

static BLOCKED_HOSTS: AtomicU64 = AtomicU64::new(0);
static BLOCKED_URLS: AtomicU64 = AtomicU64::new(0);

/// Requests refused since startup, split by the hook that caught them.
#[derive(Debug, Clone, Copy)]
pub struct BlockedCounts {
    /// Hostnames `getaddrinfo` refused to resolve because they are not on the allowlist.
    pub hosts: u64,
    /// Urls `cef_urlrequest_create` refused because they match the denylist.
    pub urls: u64,
}

impl BlockedCounts {
    #[must_use]
    pub fn total(self) -> u64 {
        self.hosts + self.urls
    }
}

pub fn record_blocked(hook: FilterHook) {
    match hook {
        FilterHook::GetAddrInfo => &BLOCKED_HOSTS,
        FilterHook::CefUrlRequestCreate => &BLOCKED_URLS,
    }
    .fetch_add(1, Ordering::Relaxed);
}

#[must_use]
pub fn blocked_counts() -> BlockedCounts {
    BlockedCounts {
        hosts: BLOCKED_HOSTS.load(Ordering::Relaxed),
        urls: BLOCKED_URLS.load(Ordering::Relaxed),
    }
}
