pub mod db;
pub mod jobs;
mod limits;
pub mod scan;
use anyhow::Result;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Clone)]
pub struct Engine {
    pub db: PathBuf,
    pub locations: Arc<std::sync::Mutex<()>>,
    _lock: Arc<std::fs::File>,
    pub paused: Arc<AtomicBool>,
    pub cancelled: Arc<AtomicBool>,
    pub scanning: Arc<AtomicBool>,
    pub dirty: Arc<AtomicBool>,
}
impl Engine {
    pub fn new(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(parent) = path.parent() {
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))?;
        lock.try_lock()
            .map_err(|_| anyhow::anyhow!("Этот индекс уже открыт другим экземпляром приложения"))?;
        // The process lock proves that no live worker owns these crash remnants.
        if let Some(parent) = path.parent() {
            let scratch = parent.join("scratch");
            if scratch.exists() {
                std::fs::remove_dir_all(scratch)?;
            }
        }
        let engine = Self {
            db: path,
            locations: Arc::new(std::sync::Mutex::new(())),
            _lock: Arc::new(lock),
            paused: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            scanning: Arc::new(AtomicBool::new(false)),
            dirty: Arc::new(AtomicBool::new(true)),
        };
        db::initialize(&engine.connect()?)?;
        engine.connect()?.execute(
            "UPDATE files SET state='pending' WHERE state='processing'",
            [],
        )?;
        Ok(engine)
    }
    pub fn connect(&self) -> Result<rusqlite::Connection> {
        let c = rusqlite::Connection::open(&self.db)?;
        c.busy_timeout(std::time::Duration::from_secs(5))?;
        c.execute_batch("PRAGMA foreign_keys=ON;")?;
        Ok(c)
    }
    pub fn pause(&self, value: bool) {
        self.paused.store(value, Ordering::Relaxed);
        // Running processors are killed; their task is returned to pending.
    }
    pub fn resume(&self) {
        self.cancelled.store(false, Ordering::Relaxed);
        self.pause(false);
        self.dirty.store(true, Ordering::Relaxed);
    }
}
pub fn low_priority() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Threading::*;
        SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_IDLE);
    }
    #[cfg(target_os = "linux")]
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 19);
    }
}
