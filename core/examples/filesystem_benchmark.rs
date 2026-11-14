use localfind_core::{db, scan, Engine};
use std::{fs, sync::atomic::Ordering, time::Instant};
fn main() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("fixtures");
    fs::create_dir(&root)?;
    for i in 0..10000 {
        fs::write(root.join(format!("Договор-{i:05}.bin")), [0u8; 128])?;
    }
    let e = Engine::new(temp.path().join("data/index.sqlite"))?;
    db::add_root(&e.connect()?, root.to_str().unwrap())?;
    let start = Instant::now();
    scan::scan(&e)?;
    let seconds = start.elapsed().as_secs_f64();
    println!(
        "initial_scan_files=10000 seconds={seconds:.3} files_per_second={:.0}",
        10000.0 / seconds
    );
    // Ready index avoids initial scan; wait until the watcher has had time to attach.
    e.dirty.store(false, Ordering::Relaxed);
    scan::start(e.clone(), "unused".into(), "unused".into());
    std::thread::sleep(std::time::Duration::from_secs(1));
    let start = Instant::now();
    fs::write(root.join("Новый-incremental.bin"), [0u8; 128])?;
    loop {
        if !db::search(
            &e.connect()?,
            &db::Query {
                text: "Новый-incremental".into(),
                mode: "names".into(),
                ..Default::default()
            },
        )?
        .is_empty()
        {
            break;
        }
        anyhow::ensure!(
            start.elapsed().as_secs() < 30,
            "Incremental update exceeded 30 seconds"
        );
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    println!(
        "incremental_latency_ms={:.1}",
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}
