use crate::{db, Engine};
use anyhow::Result;
use rusqlite::params;
use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;

pub fn local_drives() -> Vec<String> {
    #[cfg(windows)]
    {
        unsafe {
            use windows_sys::Win32::Storage::FileSystem::*;
            let mask = GetLogicalDrives();
            (0..26)
                .filter_map(|n| {
                    if mask & (1 << n) == 0 {
                        return None;
                    }
                    let drive = format!("{}:\\", (b'A' + n) as char);
                    let wide: Vec<u16> = drive.encode_utf16().chain(Some(0)).collect();
                    // DRIVE_FIXED=3, DRIVE_REMOVABLE=2. Network/optical are opt-in.
                    match GetDriveTypeW(wide.as_ptr()) {
                        2 | 3 => Some(drive),
                        _ => None,
                    }
                })
                .collect()
        }
    }
    #[cfg(not(windows))]
    {
        vec![]
    }
}
pub fn discover_drives(e: &Engine, drives: Vec<String>) -> Result<()> {
    let _guard = e
        .locations
        .lock()
        .map_err(|_| anyhow::anyhow!("Не удалось изменить расположения"))?;
    let c = e.connect()?;
    if db::setting(&c, "all_disks", "false") != "true" {
        return Ok(());
    }
    let excluded = db::excluded(&c)?;
    for drive in drives {
        let Ok(path) = std::fs::canonicalize(&drive) else {
            continue;
        };
        if excluded.iter().any(|p| path.starts_with(p)) {
            continue;
        }
        let roots = db::roots(&c)?;
        if roots.iter().any(|r| path.starts_with(&r.path)) {
            continue;
        }
        if db::add_root(&c, &drive).is_ok() {
            e.dirty.store(true, Ordering::Relaxed);
        }
    }
    Ok(())
}
pub fn placeholder_or_link(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Reparse points, offline files, recall-on-open/data-access placeholders.
        std::fs::symlink_metadata(path)
            .map(|m| m.file_attributes() & (0x400 | 0x1000 | 0x40000 | 0x400000) != 0)
            .unwrap_or(true)
    }
    #[cfg(not(windows))]
    {
        std::fs::symlink_metadata(path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(true)
    }
}
pub fn supported(ext: &str) -> bool {
    matches!(
        ext,
        "pdf"
            | "docx"
            | "xlsx"
            | "pptx"
            | "txt"
            | "md"
            | "markdown"
            | "csv"
            | "png"
            | "jpg"
            | "jpeg"
            | "tif"
            | "tiff"
            | "bmp"
    )
}
pub fn is_image(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "tif" | "tiff" | "bmp")
}
pub fn modified(m: &std::fs::Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
pub fn scan(e: &Engine) -> Result<()> {
    if e.scanning.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    struct Reset<'a>(&'a Engine);
    impl Drop for Reset<'_> {
        fn drop(&mut self) {
            self.0.scanning.store(false, Ordering::SeqCst);
        }
    }
    let _reset = Reset(e);
    let mut c = e.connect()?;
    let excludes: Vec<PathBuf> = db::excluded(&c)?
        .into_iter()
        .map(PathBuf::from)
        .chain(e.db.parent().map(Path::to_path_buf))
        .collect();
    for root in db::roots(&c)? {
        if e.cancelled.load(Ordering::Relaxed) {
            break;
        }
        if !Path::new(&root.path).is_dir() || std::fs::read_dir(&root.path).is_err() {
            c.execute("UPDATE roots SET online=0,error='Расположение недоступно; прежние результаты сохранены' WHERE id=?",[root.id])?;
            continue;
        }
        c.execute("UPDATE roots SET online=1,error=NULL WHERE id=?", [root.id])?;
        let generation = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_nanos()
            .to_string();
        let mut complete = true;
        let mut batch = Vec::new();
        let walker = WalkDir::new(&root.path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                !excludes.iter().any(|p| entry.path().starts_with(p))
                    && (entry.depth() == 0 || !placeholder_or_link(entry.path()))
            });
        for entry in walker {
            while e.paused.load(Ordering::Relaxed) && !e.cancelled.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
            }
            if e.cancelled.load(Ordering::Relaxed) {
                complete = false;
                break;
            }
            let entry = match entry {
                Ok(v) => v,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let m = match entry.metadata() {
                Ok(v) => v,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            batch.push((
                entry.path().to_string_lossy().into_owned(),
                m.len() as i64,
                modified(&m),
            ));
            if batch.len() >= 256 {
                if !write_batch(&mut c, root.id, &generation, &batch)? {
                    complete = false;
                    break;
                }
                batch.clear();
            }
        }
        if !batch.is_empty() && !write_batch(&mut c, root.id, &generation, &batch)? {
            complete = false;
        }
        if complete {
            c.execute(
                "DELETE FROM files WHERE root_id=? AND generation!=?",
                params![root.id, generation],
            )?;
        } else {
            c.execute("UPDATE roots SET error='Обход неполон: часть файлов недоступна или обход прерван. Старые записи сохранены.' WHERE id=?",[root.id])?;
        }
    }
    Ok(())
}
fn write_batch(
    c: &mut rusqlite::Connection,
    root: i64,
    generation: &str,
    batch: &[(String, i64, i64)],
) -> Result<bool> {
    let tx = c.transaction()?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM roots WHERE id=?)",
        [root],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(false);
    }
    let exclusions = db::excluded(&tx)?;
    for (path, size, modified) in batch {
        if exclusions.iter().any(|p| Path::new(path).starts_with(p)) {
            continue;
        }
        let name = Path::new(path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let ext = Path::new(path)
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let state = if supported(&ext) {
            "pending"
        } else {
            "unsupported"
        };
        // Invalidate extracted content immediately when the file changes.
        tx.execute("DELETE FROM blocks WHERE file_id IN (SELECT id FROM files WHERE path=? AND (size!=? OR modified!=?))",params![path,size,modified])?;
        tx.execute("INSERT INTO files(root_id,path,name,norm_path,norm_name,ext,size,modified,generation,state) VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(path) DO UPDATE SET generation=excluded.generation,size=excluded.size,modified=excluded.modified,state=CASE WHEN files.size!=excluded.size OR files.modified!=excluded.modified THEN excluded.state ELSE files.state END,error=CASE WHEN files.size!=excluded.size OR files.modified!=excluded.modified THEN NULL ELSE files.error END",params![root,path,name,path.to_lowercase(),name.to_lowercase(),ext,size,modified,generation,state])?;
    }
    tx.commit()?;
    Ok(true)
}
/// Reserve 1 GiB for the desktop; bound worst-case worker memory at 1.5 GiB each.
pub fn worker_capacity() -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    #[cfg(windows)]
    let available = unsafe {
        use windows_sys::Win32::System::SystemInformation::*;
        let mut state: MEMORYSTATUSEX = std::mem::zeroed();
        state.dwLength = std::mem::size_of_val(&state) as u32;
        if GlobalMemoryStatusEx(&mut state) != 0 {
            state.ullAvailPhys
        } else {
            2560 * 1024 * 1024
        }
    };
    #[cfg(not(windows))]
    let available = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("MemAvailable:"))
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|value| value.parse::<u64>().ok())
        })
        .unwrap_or(2560 * 1024)
        * 1024;
    let memory_slots =
        (available.saturating_sub(1024 * 1024 * 1024) / (1536 * 1024 * 1024)).max(1) as usize;
    cpus.min(memory_slots)
}
pub fn start(e: Engine, worker: PathBuf, python: PathBuf) {
    let events = e.clone();
    std::thread::spawn(move || {
        use notify::{RecursiveMode, Watcher};
        let signal = events.dirty.clone();
        let data_dir = events.db.parent().unwrap().to_path_buf();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
                Ok(ev)
                    if !matches!(ev.kind, notify::EventKind::Access(_))
                        && ev.paths.iter().any(|p| !p.starts_with(&data_dir)) =>
                {
                    signal.store(true, Ordering::Relaxed)
                }
                Err(_) => signal.store(true, Ordering::Relaxed),
                _ => {}
            })
            .ok();
        let mut watched = std::collections::HashSet::new();
        loop {
            let _ = discover_drives(&events, local_drives());
            if let Ok(c) = events.connect() {
                if let Ok(roots) = db::roots(&c) {
                    let current: std::collections::HashSet<String> =
                        roots.into_iter().map(|r| r.path).collect();
                    if let Some(w) = watcher.as_mut() {
                        for p in watched.difference(&current) {
                            let _ = w.unwatch(Path::new(p));
                        }
                        watched.retain(|p| current.contains(p));
                        for p in current.difference(&watched).cloned().collect::<Vec<_>>() {
                            if w.watch(Path::new(&p), RecursiveMode::Recursive).is_ok() {
                                watched.insert(p);
                            }
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    });
    let scanner = e.clone();
    std::thread::spawn(move || {
        crate::low_priority();
        let mut last = std::time::Instant::now() - Duration::from_secs(300);
        loop {
            if !scanner.paused.load(Ordering::Relaxed)
                && !scanner.cancelled.load(Ordering::Relaxed)
                && (scanner.dirty.swap(false, Ordering::Relaxed)
                    || last.elapsed() > Duration::from_secs(300))
            {
                if scan(&scanner).is_err() {
                    scanner.dirty.store(true, Ordering::Relaxed);
                }
                last = std::time::Instant::now();
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
    // Parallel extraction processes are bounded by CPU count and user setting.
    let count = worker_capacity();
    for slot in 0..count {
        let jobs = e.clone();
        let worker = worker.clone();
        let python = python.clone();
        std::thread::spawn(move || {
            crate::low_priority();
            loop {
                if !jobs.paused.load(Ordering::Relaxed)
                    && !jobs.cancelled.load(Ordering::Relaxed)
                    && !jobs.scanning.load(Ordering::Relaxed)
                {
                    let allowed = jobs
                        .connect()
                        .map(|c| {
                            db::setting(&c, "workers", "auto")
                                .parse::<usize>()
                                .unwrap_or(count)
                                .clamp(1, count)
                        })
                        .unwrap_or(1);
                    if slot < allowed
                        && matches!(crate::jobs::process_one(&jobs, &worker, &python), Ok(true))
                    {
                        continue;
                    }
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        });
    }
}
