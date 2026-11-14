use localfind_core::{
    db::{self, Query},
    Engine,
};
use std::time::Instant;
fn main() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let e = Engine::new(dir.path().join("index.sqlite"))?;
    let mut c = e.connect()?;
    c.execute("INSERT INTO roots(path) VALUES('/fixtures')", [])?;
    let start = Instant::now();
    let tx = c.transaction()?;
    for i in 0..100000 {
        let name = format!("Отчёт AB-{i:06}.txt");
        let path = format!("/fixtures/Отдел {}/{}", i % 100, name);
        tx.execute("INSERT INTO files(root_id,path,name,norm_path,norm_name,ext,size,modified,generation,state) VALUES(1,?,?,?,?, 'txt',1024,0,'bench','ready')",rusqlite::params![path,name,path.to_lowercase(),name.to_lowercase()])?;
        if i % 10 == 0 {
            tx.execute("INSERT INTO blocks(file_id,location,source,text) VALUES(?,'Страница 1','text',?)",rusqlite::params![i+1,format!("Договор поставки AB-{i:06}. Mixed language русский текст. Invoice reference {i}.")])?;
        }
    }
    tx.commit()?;
    let seconds = start.elapsed().as_secs_f64();
    for (kind, mode, terms) in [
        (
            "names",
            "names",
            vec!["AB-050001", "отчёт", "отдел 42", "99999", "ёт"],
        ),
        (
            "documents",
            "text",
            vec![
                "\"русский текст\"",
                "AB-050000",
                "invoice",
                "договор поставки",
            ],
        ),
    ] {
        let mut times = Vec::new();
        for n in 0..100 {
            let q = Query {
                text: terms[n % terms.len()].into(),
                mode: mode.into(),
                ..Default::default()
            };
            let start = Instant::now();
            let _ = db::search(&c, &q)?;
            if n >= 10 {
                times.push(start.elapsed().as_secs_f64() * 1000.0)
            }
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{kind}: p50_ms={:.2}, p95_ms={:.2}, samples={}",
            times[times.len() / 2],
            times[times.len() * 95 / 100],
            times.len()
        );
    }
    println!(
        "synthetic_insert_seconds={seconds:.2}, files_per_second={:.0}, db_bytes={}",
        100000.0 / seconds,
        db::status(&e)?.bytes
    );
    Ok(())
}
