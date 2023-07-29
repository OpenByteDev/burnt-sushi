#![allow(dead_code)]

use std::{
    fs::File,
    io::{self, Write, BufWriter},
    path::PathBuf, time::Instant,
};

use super::SimpleLog;

#[derive(Debug)]
pub struct FileLog {
    path: PathBuf,
    file: Option<BufWriter<File>>,
    last_written: Option<Instant>
}

impl FileLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            file: None,
            last_written: None
        }
    }

    fn open_file(&mut self) -> io::Result<&mut BufWriter<File>> {
        if let Some(ref mut file) = self.file {
            return Ok(file);
        }

        let file = File::options().create(true).write(true).open(&self.path)?;
        let writer = BufWriter::new(file);
        Ok(self.file.insert(writer))
    }
}

impl SimpleLog for FileLog {
    fn log(&mut self, message: &str) {
        let file = self.open_file().unwrap();
        writeln!(file, "{}", message).unwrap();
        file.flush().unwrap();
    }
}
