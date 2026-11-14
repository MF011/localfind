use localfind_core::{db, scan, Engine};
use std::path::PathBuf;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    anyhow::ensure!(
        a.len() >= 3,
        "Usage: localfind-core DATABASE status|add|scan|search|process [ARG]"
    );
    let e = Engine::new(PathBuf::from(&a[1]))?;
    match a[2].as_str() {
        "status" => println!("{}", serde_json::to_string_pretty(&db::status(&e)?)?),
        "add" => db::add_root(
            &e.connect()?,
            a.get(3).ok_or_else(|| anyhow::anyhow!("Missing folder"))?,
        )?,
        "scan" => scan::scan(&e)?,
        "search" => println!(
            "{}",
            serde_json::to_string_pretty(&db::search(
                &e.connect()?,
                &db::Query {
                    text: a.get(3).cloned().unwrap_or_default(),
                    ..Default::default()
                }
            )?)?
        ),
        "process" => {
            let worker = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../workers/extract.py");
            let python = std::env::var("LOCALFIND_PYTHON").unwrap_or_else(|_| {
                if cfg!(windows) {
                    "python".into()
                } else {
                    "python3".into()
                }
            });
            while localfind_core::jobs::process_one(&e, &worker, &PathBuf::from(python.clone()))? {}
        }
        _ => anyhow::bail!("Unknown command"),
    };
    Ok(())
}
