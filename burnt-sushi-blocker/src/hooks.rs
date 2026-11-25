use std::{ffi::c_void, panic::AssertUnwindSafe, ptr, slice, sync::OnceLock};

use cef::{_cef_request_t, _cef_string_utf16_t};
use dll_syringe::process::OwnedProcessModule;
use retour::static_detour;
use shared::rpc::blocker_service::FilterHook;

use crate::{cef, filters::Filters, utils::panic_info_to_string};

type CefUrlRequestCreateFn =
    unsafe extern "C" fn(*mut _cef_request_t, *mut c_void, *mut c_void) -> *mut c_void;
type CefStringUserfreeUtf16FreeFn = unsafe extern "C" fn(*mut _cef_string_utf16_t);
static_detour! {
    static CefUrlRequestCreateHook: unsafe extern "C" fn(*mut _cef_request_t, *mut c_void, *mut c_void) -> *mut c_void;
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
    filters: Filters,
    log_tx: tokio::sync::mpsc::UnboundedSender<LogParams>,
) -> Result<(), Box<dyn std::error::Error>> {
    static CEF_URL_REQUEST_CREATE_HOOK: OnceLock<()> = OnceLock::new();

    CEF_URL_REQUEST_CREATE_HOOK
        .get_or_try_init(|| init_cef_urlrequest_create_hook(filters, log_tx))?;

    unsafe { CefUrlRequestCreateHook.enable() }?;

    Ok(())
}

pub fn disable() -> Result<(), Box<dyn std::error::Error>> {
    if CefUrlRequestCreateHook.is_enabled() {
        unsafe { CefUrlRequestCreateHook.disable()? };
    }
    Ok(())
}

fn init_cef_urlrequest_create_hook(
    filters: Filters,
    log_tx: tokio::sync::mpsc::UnboundedSender<LogParams>,
) -> Result<(), Box<dyn std::error::Error>> {
    let libcef =
        OwnedProcessModule::find_local_by_name("libcef.dll")?.ok_or("libcef.dll not found")?;
    let cef_urlrequest_create =
        unsafe { libcef.get_local_procedure::<CefUrlRequestCreateFn>("cef_urlrequest_create")? };
    let cef_string_userfree_utf16_free = unsafe {
        libcef
            .get_local_procedure::<CefStringUserfreeUtf16FreeFn>("cef_string_userfree_utf16_free")?
    };

    unsafe {
        CefUrlRequestCreateHook.initialize(
            cef_urlrequest_create,
            move |request, client, request_context| -> *mut c_void {
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

                    let block = !filters.check(FilterHook::CefUrlRequestCreate, &url);

                    let _ = log_tx.send(LogParams::Request {
                        hook: FilterHook::CefUrlRequestCreate,
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
