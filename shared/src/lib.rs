#![feature(variant_count)]

use core::{cmp, fmt, hash};
use std::mem;

#[allow(dead_code)]
mod spotify_ad_guard_capnp {
    include!(concat!(env!("OUT_DIR"), "\\spotify_ad_guard_capnp.rs"));
}

pub mod rpc {
    pub use super::spotify_ad_guard_capnp::*;
}

impl hash::Hash for rpc::blocker_service::FilterHook {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state)
    }
}

impl cmp::Eq for rpc::blocker_service::FilterHook {}

impl fmt::Display for rpc::blocker_service::FilterHook {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            rpc::blocker_service::FilterHook::GetAddrInfo => {
                write!(f, "getaddrinfo")
            }
            rpc::blocker_service::FilterHook::CefUrlRequestCreate => {
                write!(f, "cef_urlrequest_create")
            }
        }
    }
}

impl enum_map::Enum for rpc::blocker_service::FilterHook {
    const LENGTH: usize = mem::variant_count::<Self>();

    fn from_usize(value: usize) -> Self {
        match value {
            0 => rpc::blocker_service::FilterHook::GetAddrInfo,
            1 => rpc::blocker_service::FilterHook::CefUrlRequestCreate,
            _ => unreachable!(),
        }
    }

    fn into_usize(self) -> usize {
        match self {
            rpc::blocker_service::FilterHook::GetAddrInfo => 0,
            rpc::blocker_service::FilterHook::CefUrlRequestCreate => 1,
        }
    }
}

impl<T> enum_map::EnumArray<T> for rpc::blocker_service::FilterHook {
    type Array = [T; mem::variant_count::<Self>()];
}
