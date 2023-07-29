use std::{fmt::Debug, any::Any};

pub trait SimpleLog: Any + Debug + Send + Sync {
    fn log(&mut self, message: &str);
}
