use super::SimpleLog;

#[derive(Debug)]
#[allow(unused)]
pub struct NoopLog;

impl SimpleLog for NoopLog {
    fn log(&mut self, _message: &str) {}
}
