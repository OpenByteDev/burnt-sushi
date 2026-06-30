#![allow(dead_code, non_camel_case_types)]

use std::ffi::c_void;

#[repr(C)]
pub struct _cef_string_utf16_t {
    pub str_: *mut u16,
    pub length: usize,
}

#[repr(C)]
pub struct _cef_request_t {
    _base: [usize; 5],
    _is_read_only: *mut c_void,
    pub get_url: unsafe extern "system" fn(self_: *mut _cef_request_t) -> *mut _cef_string_utf16_t,
}
