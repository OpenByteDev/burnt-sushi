use std::{ffi::CStr, mem, panic::AssertUnwindSafe, ptr, slice, sync::Arc, sync::OnceLock};

use cef::*;
use detour::static_detour;
use dll_syringe::process::OwnedProcessModule;
use enum_map::EnumMap;
use winapi::{
    shared::{minwindef::INT, ntdef::PCSTR, ws2def::ADDRINFOA},
    um::winsock2::WSAHOST_NOT_FOUND,
};

use crate::{cef, utils::panic_info_to_string, FilterRuleset};

type GetAddrInfoFn =
    unsafe extern "system" fn(PCSTR, PCSTR, *const ADDRINFOA, *const *const ADDRINFOA) -> INT;
static_detour! {
    static GetAddrInfoHook: unsafe extern "system" fn(PCSTR, PCSTR, *const ADDRINFOA, *const *const ADDRINFOA) -> INT;
}
type CefUrlRequestCreateFn = unsafe extern "C" fn(
    *mut cef::_cef_request_t,
    *mut cef::_cef_urlrequest_client_t,
    *mut cef::_cef_request_context_t,
) -> *mut cef::cef_urlrequest_t;
type CefStringUserfreeUtf16FreeFn = unsafe extern "C" fn(cef::cef_string_userfree_utf16_t);
static_detour! {
    static CefUrlRequestCreateHook: unsafe extern "C" fn(*mut _cef_request_t, *mut _cef_urlrequest_client_t, *mut _cef_request_context_t) -> *mut cef_urlrequest_t;
}

pub enum LogParams {
    Message(String),
    Request {
        url: String,
        blocked: bool,
        hook: shared::rpc::blocker_service::FilterHook,
    },
}

pub fn enable(
    filters: Arc<EnumMap<shared::rpc::blocker_service::FilterHook, FilterRuleset>>,
    log_tx: tokio::sync::mpsc::UnboundedSender<LogParams>,
) -> Result<(), Box<dyn std::error::Error>> {
    static GET_ADDR_INFO_HOOK: OnceLock<()> = OnceLock::new();
    static CEF_URL_REQUEST_CREATE_HOOK: OnceLock<()> = OnceLock::new();

    GET_ADDR_INFO_HOOK
        .get_or_try_init(|| init_get_addr_info_hook(filters.clone(), log_tx.clone()))?;
    CEF_URL_REQUEST_CREATE_HOOK
        .get_or_try_init(|| init_cef_urlrequest_create_hook(filters, log_tx))?;

    unsafe { GetAddrInfoHook.enable() }?;
    unsafe { CefUrlRequestCreateHook.enable() }?;

    Ok(())
}

pub fn disable() -> Result<(), Box<dyn std::error::Error>> {
    if GetAddrInfoHook.is_enabled() {
        unsafe { GetAddrInfoHook.disable() }?;
    }
    if CefUrlRequestCreateHook.is_enabled() {
        unsafe { CefUrlRequestCreateHook.disable()? };
    }
    Ok(())
}

fn init_get_addr_info_hook(
    filters: Arc<EnumMap<shared::rpc::blocker_service::FilterHook, FilterRuleset>>,
    log_tx: tokio::sync::mpsc::UnboundedSender<LogParams>,
) -> Result<(), Box<dyn std::error::Error>> {
    let ws2 =
        OwnedProcessModule::find_local_by_name("WS2_32.dll")?.ok_or("WS2_32.dll not found")?;
    let getaddrinfo = ws2.get_local_procedure_address("getaddrinfo")?;
    let getaddrinfo = unsafe { mem::transmute::<_, GetAddrInfoFn>(getaddrinfo) };
    unsafe {
        GetAddrInfoHook.initialize(
            getaddrinfo,
            move |node_name, service_name, hints, result| {
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    let url = CStr::from_ptr(node_name).to_str().unwrap(); // TODO:
                    let block =
                        !filters[shared::rpc::blocker_service::FilterHook::GetAddrInfo].check(url);

                    let _ = log_tx.send(LogParams::Request {
                        hook: shared::rpc::blocker_service::FilterHook::GetAddrInfo,
                        blocked: block,
                        url: url.to_string(),
                    });

                    block
                }));

                let block = match res {
                    Ok(block) => block,
                    Err(e) => {
                        let _ = log_tx.send(LogParams::Message(panic_info_to_string(e)));
                        false
                    }
                };

                if block {
                    WSAHOST_NOT_FOUND as _
                } else {
                    GetAddrInfoHook.call(node_name, service_name, hints, result)
                }
            },
        )
    }?;

    Ok(())
}

fn init_cef_urlrequest_create_hook(
    filters: Arc<EnumMap<shared::rpc::blocker_service::FilterHook, FilterRuleset>>,
    log_tx: tokio::sync::mpsc::UnboundedSender<LogParams>,
) -> Result<(), Box<dyn std::error::Error>> {
    let libcef =
        OwnedProcessModule::find_local_by_name("libcef.dll")?.ok_or("libcef.dll not found")?;
    let cef_urlrequest_create = libcef.get_local_procedure_address("cef_urlrequest_create")?;
    let cef_urlrequest_create =
        unsafe { mem::transmute::<_, CefUrlRequestCreateFn>(cef_urlrequest_create) };
    let cef_string_userfree_utf16_free =
        libcef.get_local_procedure_address("cef_string_userfree_utf16_free")?;
    let cef_string_userfree_utf16_free = unsafe {
        mem::transmute::<_, CefStringUserfreeUtf16FreeFn>(cef_string_userfree_utf16_free)
    };

    unsafe {
        CefUrlRequestCreateHook.initialize(
            cef_urlrequest_create,
            move |request, client, request_context| -> *mut cef::cef_urlrequest_t {
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    if request.is_null() {
                        return false;
                    }

                    let cef_url = ((*request).get_url)(request);
                    if cef_url.is_null() {
                        return false;
                    }

                    let wide_url = slice::from_raw_parts((*cef_url).str_, (*cef_url).length as _);
                    let url = String::from_utf16_lossy(wide_url);
                    cef_string_userfree_utf16_free(cef_url);

                    let block = !filters
                        [shared::rpc::blocker_service::FilterHook::CefUrlRequestCreate]
                        .check(&url);

                    let _ = log_tx.send(LogParams::Request {
                        hook: shared::rpc::blocker_service::FilterHook::CefUrlRequestCreate,
                        blocked: block,
                        url,
                    });

                    block
                }));

                let block = match res {
                    Ok(block) => block,
                    Err(e) => {
                        let _ = log_tx.send(LogParams::Message(panic_info_to_string(e)));
                        false
                    }
                };

                if block {
                    ptr::null_mut()
                } else {
                    CefUrlRequestCreateHook.call(request, client, request_context)
                }
            },
        )
    }?;

    Ok(())
}
