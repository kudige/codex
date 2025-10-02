use std::fs::OpenOptions;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;

pub(crate) struct TranscriptLog {
    writer: BufWriter<std::fs::File>,
}

impl TranscriptLog {
    pub(crate) fn create(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?;

        Ok(Self {
            writer: BufWriter::new(file),
        })
    }

    pub(crate) fn write_line(&mut self, line: &str) {
        if let Err(err) = writeln!(self.writer, "{line}") {
            eprintln!("Failed to write transcript log line: {err}");
            return;
        }
        if let Err(err) = self.writer.flush() {
            eprintln!("Failed to flush transcript log: {err}");
        }
    }
}
