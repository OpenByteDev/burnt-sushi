use std::any::Any;

pub fn panic_info_to_string(info: Box<dyn Any + Send>) -> String {
    if let Some(s) = info.downcast_ref::<&str>() {
        s.to_string()
    } else if let Ok(s) = info.downcast::<String>() {
        *s
    } else {
        "Unknown panic".to_string()
    }
}
