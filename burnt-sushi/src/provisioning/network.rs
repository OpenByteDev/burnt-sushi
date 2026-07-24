use windows_sys::Win32::NetworkManagement::WindowsConnectionManager::{
    WCM_CONNECTION_COST_DATA, WCM_CONNECTION_COST_FIXED, WCM_CONNECTION_COST_OVERDATALIMIT,
    WCM_CONNECTION_COST_ROAMING, WCM_CONNECTION_COST_VARIABLE, WCM_PROFILE_INFO,
    WCM_PROFILE_INFO_LIST, WcmFreeMemory, WcmGetProfileList, WcmQueryProperty,
    wcm_intf_property_connection_cost,
};

const METERED_COST_MASK: u32 = (WCM_CONNECTION_COST_FIXED
    | WCM_CONNECTION_COST_VARIABLE
    | WCM_CONNECTION_COST_ROAMING
    | WCM_CONNECTION_COST_OVERDATALIMIT) as u32;

/// Owns memory allocated by a `Wcm*` API and frees it via `WcmFreeMemory` on drop.
struct WcmAlloc<T>(*mut T);

impl<T> Drop for WcmAlloc<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WcmFreeMemory(self.0.cast()) };
        }
    }
}

/// Whether any currently known network profile is metered (fixed, variable, roaming or
/// over its data limit), as opposed to unrestricted/unknown.
pub fn is_metered_connection() -> bool {
    let mut profile_list_ptr: *mut WCM_PROFILE_INFO_LIST = std::ptr::null_mut();
    if unsafe { WcmGetProfileList(std::ptr::null(), &raw mut profile_list_ptr) } != 0 {
        return false;
    }
    let profile_list = WcmAlloc(profile_list_ptr);
    if profile_list.0.is_null() {
        return false;
    }

    let count = unsafe { (*profile_list.0).dwNumberOfItems } as usize;
    let profiles =
        unsafe { std::slice::from_raw_parts((*profile_list.0).ProfileInfo.as_ptr(), count) };

    profiles.iter().any(|profile| {
        query_connection_cost(profile).is_some_and(|cost| cost & METERED_COST_MASK != 0)
    })
}

fn query_connection_cost(profile: &WCM_PROFILE_INFO) -> Option<u32> {
    let mut data_size = 0u32;
    let mut data_ptr: *mut u8 = std::ptr::null_mut();
    let result = unsafe {
        WcmQueryProperty(
            &raw const profile.AdapterGUID,
            profile.strProfileName.as_ptr(),
            wcm_intf_property_connection_cost,
            std::ptr::null(),
            &raw mut data_size,
            &raw mut data_ptr,
        )
    };
    let data = WcmAlloc(data_ptr.cast::<WCM_CONNECTION_COST_DATA>());

    if result != 0
        || data.0.is_null()
        || (data_size as usize) < size_of::<WCM_CONNECTION_COST_DATA>()
    {
        return None;
    }

    Some(unsafe { data.0.read_unaligned() }.ConnectionCost)
}
