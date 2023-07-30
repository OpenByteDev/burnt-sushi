#![allow(dead_code)]

use std::{
    fs::{self, File},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    time::Instant,
};

use anyhow::Context;

use super::SimpleLog;

#[derive(Debug)]
pub struct FileLog {
    path: PathBuf,
    file: Option<BufWriter<File>>,
    last_written: Option<Instant>,
}

impl FileLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            file: None,
            last_written: None,
        }
    }

    fn open_file(&mut self) -> anyhow::Result<&mut BufWriter<File>> {
        if let Some(ref mut file) = self.file {
            return Ok(file);
        }

        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).context("Failed to create parent directories for log file.")?;
        }
        let mut file = File::options()
            .create(true)
            .append(true)
            .open(&self.path)
            .context("Failed to open or create log file.")?;

        if file.metadata().unwrap().len() > 10 * 1024 * 1024 /* 10mb */ {
            file = File::options()
                .write(true)
                .read(true)
                .open(&self.path)
                .context("Failed to open or create log file.")?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)
                .context("Failed to read log file.")?;

            let mut truncated_contents = String::new();
            for (index, _) in contents.match_indices('\n') {
                let succeeding = &contents[(index + 1)..];
                if succeeding.len() > 1 * 1024 * 1024 /* 1mb */ {
                    continue;
                }
                truncated_contents.push_str(succeeding);
                break;
            }
            file.seek(SeekFrom::Start(0))
                .context("Failed to seek in log file.")?;
            file.set_len(0).context("Failed to clear log file.")?;
            file.write_all(truncated_contents.as_bytes())
                .context("Failed to write log file.")?;
        }
        let writer = BufWriter::new(file);
        Ok(self.file.insert(writer))
    }
}

impl SimpleLog for FileLog {
    fn log(&mut self, message: &str) {
        let file = self
            .open_file()
            .context("Failed to prepare log file.")
            .unwrap();
        writeln!(file, "{}", message).unwrap();
        file.flush().unwrap();
    }
}
