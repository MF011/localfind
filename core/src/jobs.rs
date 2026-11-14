use crate::{db, scan, Engine};
use anyhow::{Context, Result};
use rusqlite::params;
use serde::Deserialize;
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
const PROCESSOR: &str = "extract-1:rus+eng:psm3:pdf20";
const MAX_OUTPUT: u64 = 16 * 1024 * 1024;
#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    blocks: Vec<db::Block>,
    #[serde(default)]
    needs_ocr: bool,
    error: Option<String>,
}
struct Task {
    id: i64,
    path: String,
    size: i64,
    modified: i64,
    stage: String,
}
fn claim(e: &Engine) -> Result<Option<Task>> {
    let mut c = e.connect()?;
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let ocr = db::setting(&tx, "ocr", "true") == "true";
    let mut stmt=tx.prepare("SELECT f.id,f.path,f.size,f.modified,f.state FROM files f JOIN roots r ON r.id=f.root_id WHERE r.online=1 AND (f.state='pending' OR (f.state='ocr_pending' AND ?)) ORDER BY CASE f.state WHEN 'pending' THEN 0 ELSE 1 END,f.id LIMIT 1")?;
    let task = stmt
        .query_map([ocr], |r| {
            Ok(Task {
                id: r.get(0)?,
                path: r.get(1)?,
                size: r.get(2)?,
                modified: r.get(3)?,
                stage: if r.get::<_, String>(4)? == "ocr_pending" {
                    "ocr".into()
                } else {
                    "text".into()
                },
            })
        })?
        .next()
        .transpose()?;
    drop(stmt);
    if let Some(t) = &task {
        tx.execute(
            "UPDATE files SET state='processing',error=NULL WHERE id=?",
            [t.id],
        )?;
    }
    tx.commit()?;
    Ok(task)
}
fn fingerprint(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    anyhow::ensure!(
        f.metadata()?.len() <= 100 * 1024 * 1024,
        "Файл превышает лимит 100 МБ"
    );
    let mut bytes = 0u64;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = f.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        anyhow::ensure!(
            bytes <= 100 * 1024 * 1024,
            "Файл вырос за пределы 100 МБ во время чтения"
        );
        hasher.update(&buffer[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}
struct ScratchDirectory(std::path::PathBuf);
impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn run(e: &Engine, t: &Task, worker: &Path, python: &Path) -> Result<Option<(Response, String)>> {
    let path = Path::new(&t.path);
    anyhow::ensure!(
        !scan::placeholder_or_link(path),
        "Облачная заглушка или ссылка пропущена"
    );
    anyhow::ensure!(t.size <= 100 * 1024 * 1024, "Файл превышает лимит 100 МБ");
    let hash = fingerprint(path).context("Не удалось прочитать файл")?;
    let c = e.connect()?;
    let request = serde_json::json!({"path":t.path,"stage":t.stage,"tesseract":db::setting(&c,"tesseract","tesseract"),"tessdata":db::setting(&c,"tessdata","")});
    let mut cmd = Command::new(python);
    let scratch =
        e.db.parent()
            .context("Нет папки данных")?
            .join("scratch")
            .join(t.id.to_string());
    std::fs::create_dir_all(&scratch)?;
    let _scratch_cleanup = ScratchDirectory(scratch.clone());
    cmd.env("TMP", &scratch)
        .env("TEMP", &scratch)
        .env("TMPDIR", &scratch);
    cmd.arg(worker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000 | 0x00000040);
    }
    // Process group makes cancellation terminate OCR subprocesses too.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .context("Не найден Python или обработчик документов")?;
    #[cfg(windows)]
    let job = match crate::limits::WorkerJob::attach(&child) {
        Ok(job) => job,
        Err(err) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(err);
        }
    };
    let mut input = child.stdin.take().context("Нет входного канала")?;
    if let Err(err) = input.write_all(serde_json::to_string(&request)?.as_bytes()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(err.into());
    }
    drop(input);
    let stdout = child.stdout.take().context("Нет выходного канала")?;
    let overflow = Arc::new(AtomicBool::new(false));
    let signal = overflow.clone();
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        let _ = stdout.take(MAX_OUTPUT + 1).read_to_end(&mut data);
        if data.len() as u64 > MAX_OUTPUT {
            signal.store(true, Ordering::Relaxed);
        }
        data
    });
    let start = Instant::now();
    let mut interrupted = false;
    let mut timed_out = false;
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        interrupted = e.paused.load(Ordering::Relaxed) || e.cancelled.load(Ordering::Relaxed);
        timed_out = start.elapsed() > Duration::from_secs(180) || overflow.load(Ordering::Relaxed);
        // Removing a root may delete this task while a worker is running.
        let exists: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM files WHERE id=?)",
            [t.id],
            |r| r.get(0),
        )?;
        if interrupted || timed_out || !exists {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            #[cfg(windows)]
            job.terminate();
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let output = reader.join().unwrap_or_default();
    if interrupted {
        return Ok(None);
    }
    anyhow::ensure!(
        !timed_out && output.len() as u64 <= MAX_OUTPUT,
        "Обработка превысила лимит времени или объёма"
    );
    let response: Response =
        serde_json::from_slice(&output).context("Некорректный ответ обработчика")?;
    let meta = std::fs::metadata(path).context("Файл стал недоступен")?;
    anyhow::ensure!(
        scan::modified(&meta) == t.modified
            && meta.len() as i64 == t.size
            && fingerprint(path)? == hash,
        "Файл изменился во время чтения; повторите обработку"
    );
    Ok(Some((response, hash)))
}
pub fn process_one(e: &Engine, worker: &Path, python: &Path) -> Result<bool> {
    let Some(t) = claim(e)? else { return Ok(false) };
    let result = run(e, &t, worker, python);
    let mut c = e.connect()?;
    let tx = c.transaction()?;
    let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM files WHERE id=? AND size=? AND modified=? AND state='processing')",params![t.id,t.size,t.modified],|r|r.get(0))?;
    if !valid {
        return Ok(true);
    }
    match result {
        Ok(Some((response, hash))) if response.error.is_none() => {
            if t.stage == "text" {
                tx.execute("DELETE FROM blocks WHERE file_id=?", [t.id])?;
            } else {
                tx.execute(
                    "DELETE FROM blocks WHERE file_id=? AND source='ocr'",
                    [t.id],
                )?;
            }
            for b in response.blocks {
                tx.execute(
                    "INSERT INTO blocks(file_id,location,source,text,boxes) VALUES(?,?,?,?,?)",
                    params![
                        t.id,
                        b.location,
                        b.source,
                        b.text,
                        serde_json::to_string(&b.boxes)?
                    ],
                )?;
            }
            let state = if response.needs_ocr && t.stage == "text" {
                "ocr_pending"
            } else {
                "ready"
            };
            tx.execute(
                "UPDATE files SET state=?,fingerprint=?,processor=?,error=NULL WHERE id=?",
                params![
                    state,
                    hash,
                    db::setting(&tx, "processor_key", PROCESSOR),
                    t.id
                ],
            )?;
        }
        Ok(None) => {
            tx.execute(
                "UPDATE files SET state=? WHERE id=?",
                params![
                    if t.stage == "ocr" {
                        "ocr_pending"
                    } else {
                        "pending"
                    },
                    t.id
                ],
            )?;
        }
        other => {
            let error = match other {
                Ok(Some((r, _))) => r.error.unwrap_or_else(|| "Ошибка обработки".into()),
                Err(err) => err.to_string(),
                _ => unreachable!(),
            };
            tx.execute(
                "UPDATE files SET state='error',error=? WHERE id=?",
                params![error, t.id],
            )?;
        }
    }
    tx.commit()?;
    Ok(true)
}
pub fn invalidate_version(e: &Engine, worker: &Path, python: &Path) -> Result<()> {
    let mut c = e.connect()?;
    let config = serde_json::json!({"operation":"signature","tesseract":db::setting(&c,"tesseract","tesseract"),"tessdata":db::setting(&c,"tessdata","")});
    let mut cmd = Command::new(python);
    cmd.arg(worker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000 | 0x40);
    }
    let mut signature = PROCESSOR.to_owned();
    if let Ok(mut child) = cmd.spawn() {
        if let Some(mut input) = child.stdin.take() {
            let _ = input.write_all(serde_json::to_string(&config)?.as_bytes());
        }
        let start = Instant::now();
        while child.try_wait()?.is_none() && start.elapsed() < Duration::from_secs(12) {
            std::thread::sleep(Duration::from_millis(50));
        }
        if start.elapsed() >= Duration::from_secs(12) {
            let _ = child.kill();
        }
        if let Ok(output) = child.wait_with_output() {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                if let Some(version) = value.get("version").and_then(|v| v.as_str()) {
                    signature = format!("{PROCESSOR}:{version}");
                }
            }
        }
    }
    let tx = c.transaction()?;
    tx.execute("DELETE FROM blocks WHERE file_id IN (SELECT id FROM files WHERE processor IS NOT NULL AND processor!=?)",[&signature])?;
    tx.execute("UPDATE files SET state='pending' WHERE processor IS NOT NULL AND processor!=? AND state!='unsupported'",[&signature])?;
    db::set_setting(&tx, "processor_key", &signature)?;
    tx.commit()?;
    Ok(())
}
