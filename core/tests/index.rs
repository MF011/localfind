use localfind_core::{
    db::{self, Query},
    scan, Engine,
};
use std::fs;
fn setup() -> (tempfile::TempDir, Engine, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("Документы");
    fs::create_dir(&root).unwrap();
    let e = Engine::new(d.path().join("data/index.sqlite")).unwrap();
    db::add_root(&e.connect().unwrap(), root.to_str().unwrap()).unwrap();
    (d, e, root)
}
fn find(e: &Engine, s: &str) -> Vec<db::Hit> {
    db::search(
        &e.connect().unwrap(),
        &Query {
            text: s.into(),
            ..Default::default()
        },
    )
    .unwrap()
}
#[test]
fn unicode_substrings_filters_and_moves() {
    let (_d, e, r) = setup();
    fs::write(r.join("Счёт AB-102.txt"), "hello").unwrap();
    scan::scan(&e).unwrap();
    assert_eq!(find(&e, "ЧЁТ").len(), 1);
    assert_eq!(find(&e, "AB-10").len(), 1);
    assert_eq!(find(&e, "документы")[0].source, "path");
    assert_eq!(find(&e, "ёт").len(), 1);
    assert!(db::search(
        &e.connect().unwrap(),
        &Query {
            text: "счёт".into(),
            max_size: Some(1),
            ..Default::default()
        }
    )
    .unwrap()
    .is_empty());
    fs::rename(r.join("Счёт AB-102.txt"), r.join("Новый.txt")).unwrap();
    scan::scan(&e).unwrap();
    assert!(find(&e, "счёт").is_empty());
    assert_eq!(find(&e, "новый").len(), 1);
    fs::remove_file(r.join("Новый.txt")).unwrap();
    scan::scan(&e).unwrap();
    assert!(find(&e, "новый").is_empty());
}
#[test]
fn offline_roots_preserve_results_and_removal_purges_text() {
    let (d, e, r) = setup();
    fs::write(r.join("контракт.txt"), "hello").unwrap();
    scan::scan(&e).unwrap();
    let id = find(&e, "контракт")[0].id;
    let c = e.connect().unwrap();
    c.execute(
        "INSERT INTO blocks(file_id,location,source,text) VALUES(?,'p1','text','секретная фраза')",
        [id],
    )
    .unwrap();
    assert_eq!(find(&e, "\"секретная фраза\"").len(), 1);
    fs::rename(&r, d.path().join("offline")).unwrap();
    scan::scan(&e).unwrap();
    assert!(!find(&e, "контракт")[0].online);
    db::remove_root(&c, db::roots(&c).unwrap()[0].id).unwrap();
    assert!(find(&e, "секретная").is_empty());
    assert!(find(&e, "контракт").is_empty());
}
#[test]
fn interrupted_scan_does_not_delete_and_recovery_requeues() {
    let (_d, e, r) = setup();
    fs::write(r.join("a.txt"), "a").unwrap();
    scan::scan(&e).unwrap();
    e.cancelled
        .store(true, std::sync::atomic::Ordering::Relaxed);
    scan::scan(&e).unwrap();
    assert_eq!(find(&e, "a.txt").len(), 1);
    e.connect()
        .unwrap()
        .execute("UPDATE files SET state='processing'", [])
        .unwrap();
    let database = e.db.clone();
    assert!(Engine::new(database.clone()).is_err());
    drop(e);
    let recovered = Engine::new(database).unwrap();
    assert_eq!(db::status(&recovered).unwrap().pending, 1);
}
#[test]
fn exclusion_purges_and_prevents_reappearance() {
    let (_d, e, r) = setup();
    let child = r.join("private");
    fs::create_dir(&child).unwrap();
    fs::write(child.join("secret.txt"), "secret").unwrap();
    scan::scan(&e).unwrap();
    assert_eq!(find(&e, "secret").len(), 1);
    db::exclude(&mut e.connect().unwrap(), child.to_str().unwrap()).unwrap();
    scan::scan(&e).unwrap();
    assert!(find(&e, "secret").is_empty());
}
#[test]
fn malformed_queries_are_literals() {
    let (_d, e, r) = setup();
    fs::write(r.join("AB-102.txt"), "ok").unwrap();
    scan::scan(&e).unwrap();
    for q in ["\"", "*", "OR", "a\"b", "\"AB-102\""] {
        let _ = find(&e, q);
    }
}
#[cfg(unix)]
#[test]
fn symlink_cycles_not_followed() {
    let (_d, e, r) = setup();
    std::os::unix::fs::symlink(&r, r.join("loop")).unwrap();
    fs::write(r.join("one.txt"), "x").unwrap();
    scan::scan(&e).unwrap();
    assert_eq!(db::status(&e).unwrap().total, 1);
}
#[test]
fn extraction_search_cache_and_file_change() {
    let (_d, e, r) = setup();
    let p = r.join("Контракт.txt");
    fs::write(
        &p,
        "Русский договор AB-102. English invoice. <script>never execute</script>",
    )
    .unwrap();
    scan::scan(&e).unwrap();
    let worker = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../workers/extract.py");
    let python = std::path::PathBuf::from(if cfg!(windows) { "python" } else { "python3" });
    assert!(localfind_core::jobs::process_one(&e, &worker, &python).unwrap());
    assert!(!localfind_core::jobs::process_one(&e, &worker, &python).unwrap());
    assert_eq!(find(&e, "\"Русский договор\"")[0].source, "text");
    assert_eq!(find(&e, "AB-102").len(), 1);
    assert_eq!(find(&e, "invoice").len(), 1);
    scan::scan(&e).unwrap();
    assert!(!localfind_core::jobs::process_one(&e, &worker, &python).unwrap());
    fs::write(&p, "Новое содержимое документа").unwrap();
    scan::scan(&e).unwrap();
    assert!(find(&e, "invoice").is_empty());
    localfind_core::jobs::process_one(&e, &worker, &python).unwrap();
    assert_eq!(find(&e, "Новое содержимое").len(), 1);
}
#[test]
fn pause_kills_worker_and_preserves_retryable_task() {
    let (d, e, r) = setup();
    fs::write(r.join("slow.txt"), "sample").unwrap();
    scan::scan(&e).unwrap();
    let script = d.path().join("slow.py");
    fs::write(&script, "import time, os, pathlib\np=pathlib.Path(os.environ['TMP']) / 'private.tmp'\np.write_bytes(b'synthetic private fixture')\ntime.sleep(20)\n").unwrap();
    let job = e.clone();
    let handle = std::thread::spawn(move || {
        localfind_core::jobs::process_one(
            &job,
            &script,
            std::path::Path::new(if cfg!(windows) { "python" } else { "python3" }),
        )
        .unwrap()
    });
    let scratch = e.db.parent().unwrap().join("scratch/1/private.tmp");
    for _ in 0..150 {
        if scratch.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        scratch.exists(),
        "Worker must create a temporary file before cancellation"
    );
    e.pause(true);
    assert!(handle.join().unwrap());
    assert_eq!(db::status(&e).unwrap().pending, 1);
    assert_eq!(db::status(&e).unwrap().errors, 0);
    assert_eq!(
        fs::read_dir(e.db.parent().unwrap().join("scratch"))
            .unwrap()
            .count(),
        0
    );
}
#[test]
fn removed_ids_are_never_reused_by_new_files() {
    let (_d, e, r) = setup();
    fs::write(r.join("a.txt"), "a").unwrap();
    scan::scan(&e).unwrap();
    let first = find(&e, "a.txt")[0].id;
    fs::remove_file(r.join("a.txt")).unwrap();
    scan::scan(&e).unwrap();
    fs::write(r.join("b.txt"), "b").unwrap();
    scan::scan(&e).unwrap();
    assert!(find(&e, "b.txt")[0].id > first);
}
#[test]
fn all_disks_discovers_new_roots_and_merges_without_losing_content() {
    let (d, e, r) = setup();
    fs::write(r.join("contract.txt"), "fixture").unwrap();
    scan::scan(&e).unwrap();
    let c = e.connect().unwrap();
    let id = find(&e, "contract")[0].id;
    c.execute(
        "INSERT INTO blocks(file_id,location,source,text) VALUES(?,'','text','сохранённый текст')",
        [id],
    )
    .unwrap();
    db::set_setting(&c, "all_disks", "true").unwrap();
    let other = d.path().join("new-drive");
    fs::create_dir(&other).unwrap();
    scan::discover_drives(&e, vec![other.to_string_lossy().into_owned()]).unwrap();
    assert_eq!(db::roots(&c).unwrap().len(), 2);
    // Selecting a parent retains previously extracted content and avoids duplicate roots.
    db::add_root(&c, d.path().to_str().unwrap()).unwrap();
    assert_eq!(db::roots(&c).unwrap().len(), 1);
    assert_eq!(find(&e, "сохранённый").len(), 1);
    db::set_setting(&c, "all_disks", "false").unwrap();
    db::remove_root(&c, db::roots(&c).unwrap()[0].id).unwrap();
    scan::discover_drives(&e, vec![other.to_string_lossy().into_owned()]).unwrap();
    assert!(db::roots(&c).unwrap().is_empty());
}
