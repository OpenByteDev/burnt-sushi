use std::{any::Any, fmt::Debug};

pub trait SimpleLog: Any + Debug + Send + Sync {
    fn log(&mut self, message: &str);
}
