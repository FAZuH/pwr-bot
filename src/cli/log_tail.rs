//! Follow the stable latest-log symlink across daily rotation.

use std::fs;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use tokio::time::sleep;

use crate::cli::service;

pub const LATEST_LOG: &str = "pwr-bot.latest.log";
const READ_INTERVAL: Duration = Duration::from_millis(200);
const FILE_WAIT: Duration = Duration::from_secs(15);
const ROTATION_TAIL_LINES: usize = 32;

pub fn log_file_path_from(registration: Option<&service::Registration>) -> PathBuf {
    let logs = registration
        .map(|reg| &reg.logs_path)
        .filter(|path| !path.as_os_str().is_empty())
        .cloned()
        .unwrap_or_else(service::logs_path);
    logs.join(LATEST_LOG)
}

pub fn log_file_path() -> PathBuf {
    let registration = service::read_registration(&service::registration_path())
        .ok()
        .flatten();
    log_file_path_from(registration.as_ref())
}

/// Byte offset where the last `lines` lines start. A trailing newline is not
/// treated as a line of its own.
pub fn start_offset_for_lines<R: Read + Seek>(reader: &mut R, lines: usize) -> Result<u64> {
    let len = reader.seek(SeekFrom::End(0))?;
    if lines == 0 {
        return Ok(len);
    }
    let mut pos = len;
    if len > 0 {
        reader.seek(SeekFrom::Start(len - 1))?;
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte)?;
        if byte[0] == b'\n' {
            pos = len - 1;
        }
    }
    let mut remaining = lines;
    let mut byte = [0u8; 1];
    while pos > 0 && remaining > 0 {
        pos -= 1;
        reader.seek(SeekFrom::Start(pos))?;
        reader.read_exact(&mut byte)?;
        if byte[0] == b'\n' {
            remaining -= 1;
        }
    }
    Ok(if remaining == 0 { pos + 1 } else { 0 })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identity {
    dev: u64,
    ino: u64,
}

impl Identity {
    fn of(path: &Path) -> io::Result<Self> {
        let meta = fs::metadata(path)?;
        Ok(Self {
            dev: meta.dev(),
            ino: meta.ino(),
        })
    }
}

struct Follower {
    path: PathBuf,
    file: File,
    identity: Identity,
    partial: Vec<u8>,
}

impl Follower {
    fn open(path: &Path, lines: Option<usize>) -> Result<Self> {
        Self::open_with(path, lines, Vec::new())
    }

    fn open_with(path: &Path, lines: Option<usize>, partial: Vec<u8>) -> Result<Self> {
        let mut file =
            File::open(path).with_context(|| format!("failed to open log `{}`", path.display()))?;
        let identity = Identity::of(path)
            .with_context(|| format!("failed to stat log `{}`", path.display()))?;
        let start = match lines {
            Some(count) => start_offset_for_lines(&mut file, count)?,
            None => file.seek(SeekFrom::End(0))?,
        };
        file.seek(SeekFrom::Start(start))?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            identity,
            partial,
        })
    }

    fn current_identity(&self) -> bool {
        Identity::of(&self.path).is_ok_and(|current| current == self.identity)
    }

    fn maybe_reopen(&mut self) {
        if self.current_identity() {
            return;
        }
        let carried = std::mem::take(&mut self.partial);
        match Follower::open_with(&self.path, Some(ROTATION_TAIL_LINES), carried.clone()) {
            Ok(reopened) => *self = reopened,
            Err(_) => self.partial = carried,
        }
    }

    fn read_available(&mut self) -> Result<Vec<String>> {
        let mut bytes = Vec::new();
        self.file.read_to_end(&mut bytes)?;
        self.partial.extend_from_slice(&bytes);
        let mut lines = Vec::new();
        while let Some(index) = self.partial.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=index).collect();
            lines.push(String::from_utf8_lossy(&line[..line.len() - 1]).to_string());
        }
        Ok(lines)
    }
}

/// Prints new log lines and blocks. Reopens by name when the file is rotated.
pub async fn follow(path: &Path, lines: Option<usize>) -> Result<()> {
    let deadline = tokio::time::Instant::now() + FILE_WAIT;
    let mut follower = loop {
        match Follower::open(path, lines) {
            Ok(follower) => break follower,
            Err(e) if tokio::time::Instant::now() < deadline => {
                log::debug!("log not ready yet: {e:#}");
                sleep(READ_INTERVAL).await;
            }
            Err(e) => return Err(e),
        }
    };

    loop {
        for line in follower.read_available()? {
            println!("{line}");
        }
        follower.maybe_reopen();
        sleep(READ_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::io::Write;

    use super::*;

    #[test]
    fn start_offset_selects_the_last_lines() {
        let data = b"a\nb\nc\n".to_vec();
        let mut cursor = Cursor::new(data);
        assert_eq!(start_offset_for_lines(&mut cursor, 1).expect("one line"), 4);
        assert_eq!(
            start_offset_for_lines(&mut cursor, 2).expect("two lines"),
            2
        );
        assert_eq!(
            start_offset_for_lines(&mut cursor, 10).expect("all lines"),
            0
        );
        assert_eq!(start_offset_for_lines(&mut cursor, 0).expect("no lines"), 6);
    }

    #[test]
    fn buffers_a_partial_line_until_the_newline_arrives() {
        let dir = tempfile::tempdir().expect("temp dir");
        let log = dir.path().join("pwr-bot.latest.log");
        fs::write(&log, "first\npartial").expect("write log");
        let mut follower = Follower::open(&log, Some(2)).expect("open log");
        assert_eq!(
            follower.read_available().expect("read"),
            vec!["first".to_string()]
        );

        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&log)
            .expect("open for append");
        file.write_all(b"-done\n").expect("append");
        assert_eq!(
            follower.read_available().expect("read complete line"),
            vec!["partial-done".to_string()]
        );
    }

    #[test]
    fn reopens_when_the_symlink_is_rotated() {
        let dir = tempfile::tempdir().expect("temp dir");
        let latest = dir.path().join("pwr-bot.latest.log");
        let old = dir.path().join("pwr-bot.2026-11-17.log");
        fs::write(&old, "old line\n").expect("write old log");
        std::os::unix::fs::symlink(&old, &latest).expect("symlink old log");

        let mut follower = Follower::open(&latest, None).expect("open latest");
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&old)
            .expect("open old for append");
        file.write_all(b"new line\n").expect("append to old");
        assert_eq!(
            follower.read_available().expect("read old"),
            vec!["new line".to_string()]
        );

        let rotated = dir.path().join("pwr-bot.2026-11-18.log");
        fs::write(&rotated, "rotated line\n").expect("write rotated log");
        fs::remove_file(&latest).expect("remove symlink");
        std::os::unix::fs::symlink(&rotated, &latest).expect("symlink rotated log");

        assert!(!follower.current_identity());
        follower.maybe_reopen();
        assert_eq!(
            follower.read_available().expect("read rotated"),
            vec!["rotated line".to_string()]
        );
    }

    #[test]
    fn rotation_carries_partial_line_and_keeps_first_lines() {
        let dir = tempfile::tempdir().expect("temp dir");
        let latest = dir.path().join("pwr-bot.latest.log");
        let old = dir.path().join("pwr-bot.2026-11-17.log");
        fs::write(&old, "old line\nhalf").expect("write old log");
        std::os::unix::fs::symlink(&old, &latest).expect("symlink old log");

        let mut follower = Follower::open(&latest, Some(2)).expect("open latest");
        assert_eq!(
            follower.read_available().expect("read old"),
            vec!["old line".to_string()]
        );
        assert_eq!(follower.partial, b"half");

        let rotated = dir.path().join("pwr-bot.2026-11-18.log");
        fs::write(&rotated, "a line\nb line\n").expect("write rotated log");
        fs::remove_file(&latest).expect("remove symlink");
        std::os::unix::fs::symlink(&rotated, &latest).expect("symlink rotated log");

        follower.maybe_reopen();
        assert_eq!(
            follower.read_available().expect("read rotated"),
            vec!["halfa line".to_string(), "b line".to_string()]
        );
    }

    #[test]
    fn tail_offset_works_without_a_trailing_newline() {
        let data = b"a\nb\nc".to_vec();
        let mut cursor = Cursor::new(data);
        assert_eq!(
            start_offset_for_lines(&mut cursor, 1).expect("last line"),
            4
        );
        assert_eq!(
            start_offset_for_lines(&mut cursor, 2).expect("last two lines"),
            2
        );
    }

    #[test]
    fn log_path_comes_from_the_lease() {
        let dir = tempfile::tempdir().expect("temp dir");
        let reg = service::Registration {
            id: "owner".to_string(),
            version: "0.4.3".to_string(),
            pid: 42,
            logs_path: dir.path().to_path_buf(),
        };
        assert_eq!(log_file_path_from(Some(&reg)), dir.path().join(LATEST_LOG));
        let empty = service::Registration {
            id: "old".to_string(),
            logs_path: PathBuf::new(),
            ..reg
        };
        assert_eq!(
            log_file_path_from(Some(&empty)),
            service::logs_path().join(LATEST_LOG)
        );
    }
}
