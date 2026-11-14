use anyhow::{bail, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub fn initialize(c: &Connection) -> Result<()> {
    let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > 1 {
        bail!("База создана более новой версией приложения");
    }
    c.execute_batch("PRAGMA journal_mode=WAL;
    CREATE TABLE IF NOT EXISTS roots(id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, online INTEGER NOT NULL DEFAULT 1, error TEXT);
    CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS exclusions(path TEXT PRIMARY KEY);
    CREATE TABLE IF NOT EXISTS files(id INTEGER PRIMARY KEY AUTOINCREMENT, root_id INTEGER NOT NULL REFERENCES roots ON DELETE CASCADE,
      path TEXT NOT NULL UNIQUE, name TEXT NOT NULL, norm_path TEXT NOT NULL, norm_name TEXT NOT NULL,
      ext TEXT NOT NULL, size INTEGER NOT NULL, modified INTEGER NOT NULL, generation TEXT NOT NULL,
      state TEXT NOT NULL DEFAULT 'pending', error TEXT, fingerprint TEXT, processor TEXT);
    CREATE INDEX IF NOT EXISTS file_root ON files(root_id);
    CREATE INDEX IF NOT EXISTS file_name_order ON files(norm_name,path);
    CREATE INDEX IF NOT EXISTS file_queue ON files(state);
    CREATE VIRTUAL TABLE IF NOT EXISTS names USING fts5(name,path, tokenize='trigram');
    CREATE TABLE IF NOT EXISTS blocks(id INTEGER PRIMARY KEY,file_id INTEGER NOT NULL REFERENCES files ON DELETE CASCADE,
      location TEXT NOT NULL, source TEXT NOT NULL, text TEXT NOT NULL, boxes TEXT NOT NULL DEFAULT '[]');
    CREATE INDEX IF NOT EXISTS block_file ON blocks(file_id);
    CREATE VIRTUAL TABLE IF NOT EXISTS contents USING fts5(text, tokenize='unicode61');
    CREATE TRIGGER IF NOT EXISTS file_insert AFTER INSERT ON files BEGIN
      INSERT INTO names(rowid,name,path) VALUES(new.id,new.norm_name,new.norm_path); END;
    CREATE TRIGGER IF NOT EXISTS file_delete AFTER DELETE ON files BEGIN DELETE FROM names WHERE rowid=old.id; END;
    CREATE TRIGGER IF NOT EXISTS file_update AFTER UPDATE OF norm_name,norm_path ON files BEGIN
      DELETE FROM names WHERE rowid=old.id;
      INSERT INTO names(rowid,name,path) VALUES(new.id,new.norm_name,new.norm_path); END;
    CREATE TRIGGER IF NOT EXISTS block_insert AFTER INSERT ON blocks BEGIN INSERT INTO contents(rowid,text) VALUES(new.id,new.text); END;
    CREATE TRIGGER IF NOT EXISTS block_delete AFTER DELETE ON blocks BEGIN DELETE FROM contents WHERE rowid=old.id; END;
    PRAGMA user_version=1;")?;
    Ok(())
}
#[derive(Serialize, Debug)]
pub struct Root {
    pub id: i64,
    pub path: String,
    pub online: bool,
    pub error: Option<String>,
}
pub fn roots(c: &Connection) -> Result<Vec<Root>> {
    Ok(
        c.prepare("SELECT id,path,online,error FROM roots ORDER BY path")?
            .query_map([], |r| {
                Ok(Root {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    online: r.get(2)?,
                    error: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?,
    )
}
pub fn add_root(c: &Connection, path: &str) -> Result<()> {
    let p = std::fs::canonicalize(path)?;
    if !p.is_dir() {
        bail!("Выберите папку или диск");
    }
    let tx = c.unchecked_transaction()?;
    let current = roots(&tx)?;
    for root in &current {
        if p.starts_with(&root.path) {
            bail!("Папка уже входит в добавленное расположение");
        }
    }
    tx.execute(
        "INSERT INTO roots(path) VALUES(?)",
        [p.to_string_lossy().as_ref()],
    )?;
    let new_id = tx.last_insert_rowid();
    for root in current {
        if Path::new(&root.path).starts_with(&p) {
            tx.execute(
                "UPDATE files SET root_id=? WHERE root_id=?",
                params![new_id, root.id],
            )?;
            tx.execute("DELETE FROM roots WHERE id=?", [root.id])?;
        }
    }
    tx.commit()?;
    Ok(())
}
pub fn remove_root(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM roots WHERE id=?", [id])?;
    Ok(())
}
pub fn excluded(c: &Connection) -> Result<Vec<String>> {
    Ok(c.prepare("SELECT path FROM exclusions ORDER BY path")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}
pub fn exclude(c: &mut Connection, path: &str) -> Result<()> {
    let p = std::fs::canonicalize(path)?.to_string_lossy().into_owned();
    let tx = c.transaction()?;
    tx.execute("INSERT OR IGNORE INTO exclusions VALUES(?)", [&p])?;
    let ids: Vec<(i64, String)> = tx
        .prepare("SELECT id,path FROM files")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, file) in ids {
        if Path::new(&file).starts_with(&p) {
            tx.execute("DELETE FROM files WHERE id=?", [id])?;
        }
    }
    tx.commit()?;
    Ok(())
}
#[derive(Default, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct Query {
    pub text: String,
    pub folder: String,
    pub extension: String,
    pub min_size: Option<i64>,
    pub max_size: Option<i64>,
    pub after: Option<i64>,
    pub before: Option<i64>,
    pub mode: String,
}
#[derive(Serialize, Debug)]
pub struct Hit {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub size: i64,
    pub modified: i64,
    pub source: String,
    pub location: String,
    pub snippet: String,
    pub online: bool,
}
// Treat punctuation as text, never expose the FTS query language to ordinary input.
pub fn phrase_query(input: &str) -> String {
    let mut parts = Vec::new();
    let mut buffer = String::new();
    let mut quoted = false;
    for ch in input.chars() {
        if ch == '"' {
            if !buffer.trim().is_empty() {
                parts.push(format!("\"{}\"", buffer.trim().replace('"', "\"\"")));
                buffer.clear();
            }
            quoted = !quoted;
        } else if ch.is_whitespace() && !quoted {
            if !buffer.is_empty() {
                parts.push(format!("\"{}\"", buffer.replace('"', "\"\"")));
                buffer.clear();
            }
        } else {
            buffer.push(ch);
        }
    }
    if !buffer.trim().is_empty() {
        parts.push(format!("\"{}\"", buffer.trim().replace('"', "\"\"")));
    }
    parts.join(" AND ")
}
pub fn search(c: &Connection, q: &Query) -> Result<Vec<Hit>> {
    let needle = q.text.trim().trim_matches('"').to_lowercase();
    if needle.is_empty() {
        return Ok(vec![]);
    }
    let folder = q.folder.trim_end_matches(['/', '\\']);
    let prefix = format!("{}{}", folder, std::path::MAIN_SEPARATOR);
    let ext = q.extension.trim_start_matches('.').to_lowercase();
    let filter="(:folder='' OR f.path=:folder OR substr(f.path,1,length(:folder)+1)=:prefix) AND (:ext='' OR f.ext=:ext) AND (:min IS NULL OR f.size>=:min) AND (:max IS NULL OR f.size<=:max) AND (:after IS NULL OR f.modified>=:after) AND (:before IS NULL OR f.modified<=:before)";
    let trigram = format!("\"{}\"", needle.replace('"', "\"\""));
    let tokens = phrase_query(&q.text);
    let run = |sql: &str| -> Result<Vec<Hit>> {
        let mut stmt = c.prepare(sql)?;
        for (name, value) in [
            (":needle", needle.as_str()),
            (":trigram", trigram.as_str()),
            (":tokens", tokens.as_str()),
            (":folder", folder),
            (":prefix", prefix.as_str()),
            (":ext", ext.as_str()),
        ] {
            if let Some(i) = stmt.parameter_index(name)? {
                stmt.raw_bind_parameter(i, value)?;
            }
        }
        for (name, value) in [
            (":min", q.min_size),
            (":max", q.max_size),
            (":after", q.after),
            (":before", q.before),
        ] {
            if let Some(i) = stmt.parameter_index(name)? {
                stmt.raw_bind_parameter(i, value)?;
            }
        }
        let hits = stmt
            .raw_query()
            .mapped(|r| {
                Ok(Hit {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    path: r.get(2)?,
                    size: r.get(3)?,
                    modified: r.get(4)?,
                    source: r.get(5)?,
                    location: r.get(6)?,
                    snippet: r.get(7)?,
                    online: r.get(8)?,
                })
            })
            .collect::<rusqlite::Result<_>>()?;
        Ok(hits)
    };
    let mut hits = Vec::new();
    if q.mode != "text" {
        let broad = needle.chars().count() < 3
            || c.query_row(
                "SELECT count(*) FROM (SELECT rowid FROM names WHERE names MATCH ? LIMIT 1001)",
                [&trigram],
                |r| r.get::<_, i64>(0),
            )? > 1000;
        if broad {
            // Ordered B-tree scans stop as soon as the result window is full.
            // Exact, name-substring and path-only phases are disjoint.
            for predicate in [
                "f.norm_name=:needle",
                "f.norm_name!=:needle AND instr(f.norm_name,:needle)>0",
                "instr(f.norm_name,:needle)=0 AND instr(f.norm_path,:needle)>0",
            ] {
                let limit = 200 - hits.len();
                if limit == 0 {
                    break;
                }
                let sql=format!("SELECT f.id,f.name,f.path,f.size,f.modified,CASE WHEN instr(f.norm_name,:needle)>0 THEN 'name' ELSE 'path' END,'',f.path,r.online FROM files f INDEXED BY file_name_order JOIN roots r ON r.id=f.root_id WHERE {predicate} AND {filter} ORDER BY f.norm_name,f.path LIMIT {limit}");
                hits.extend(run(&sql)?);
            }
        } else {
            let sql=format!("SELECT f.id,f.name,f.path,f.size,f.modified,CASE WHEN instr(f.norm_name,:needle)>0 THEN 'name' ELSE 'path' END,'',f.path,r.online FROM names JOIN files f ON f.id=names.rowid JOIN roots r ON r.id=f.root_id WHERE names MATCH :trigram AND {filter} ORDER BY CASE WHEN f.norm_name=:needle THEN 0 WHEN instr(f.norm_name,:needle)>0 THEN 1 ELSE 2 END,f.norm_name,f.path LIMIT 200");
            hits = run(&sql)?;
        }
    }
    if q.mode != "names"
        && hits.len() < 200
        && !tokens.is_empty()
        && q.text.chars().any(char::is_alphanumeric)
    {
        let sql=format!("WITH matches AS (SELECT b.file_id,b.source,b.location,snippet(contents,0,'⟦','⟧',' … ',28) AS snippet FROM contents JOIN blocks b ON b.id=contents.rowid WHERE contents MATCH :tokens), ranked AS (SELECT *,ROW_NUMBER() OVER(PARTITION BY file_id ORDER BY location) AS rn FROM matches) SELECT f.id,f.name,f.path,f.size,f.modified,m.source,m.location,m.snippet,r.online FROM ranked m JOIN files f ON f.id=m.file_id JOIN roots r ON r.id=f.root_id WHERE m.rn=1 AND {filter} ORDER BY f.norm_name,f.path LIMIT 200");
        let mut seen: std::collections::HashSet<i64> = hits.iter().map(|h| h.id).collect();
        for h in run(&sql)? {
            if seen.insert(h.id) {
                hits.push(h);
            }
            if hits.len() == 200 {
                break;
            }
        }
    }
    Ok(hits)
}
#[derive(Serialize, Deserialize, Debug)]
pub struct Block {
    pub location: String,
    pub source: String,
    pub text: String,
    #[serde(default)]
    pub boxes: serde_json::Value,
}
pub fn preview(c: &Connection, id: i64) -> Result<Vec<Block>> {
    Ok(c.prepare("SELECT location,source,substr(text,1,16000),boxes FROM blocks WHERE file_id=? ORDER BY id LIMIT 40")?.query_map([id],|r|Ok(Block{location:r.get(0)?,source:r.get(1)?,text:r.get(2)?,boxes:serde_json::from_str(&r.get::<_,String>(3)?).unwrap_or_default()}))?.collect::<rusqlite::Result<_>>()?)
}
#[derive(Serialize)]
pub struct Status {
    pub total: i64,
    pub pending: i64,
    pub processing: i64,
    pub ready: i64,
    pub errors: i64,
    pub unsupported: i64,
    pub offline: i64,
    pub bytes: u64,
    pub paused: bool,
    pub scanning: bool,
    pub cancelled: bool,
}
pub fn status(e: &crate::Engine) -> Result<Status> {
    use std::sync::atomic::Ordering;
    let c = e.connect()?;
    let count = |state: &str| -> Result<i64> {
        Ok(
            c.query_row("SELECT count(*) FROM files WHERE state=?", [state], |r| {
                r.get(0)
            })?,
        )
    };
    let bytes = [
        e.db.clone(),
        e.db.with_extension("sqlite-wal"),
        e.db.with_extension("sqlite-shm"),
    ]
    .iter()
    .filter_map(|p| std::fs::metadata(p).ok())
    .map(|m| m.len())
    .sum();
    Ok(Status {
        total: c.query_row("SELECT count(*) FROM files", [], |r| r.get(0))?,
        pending: count("pending")? + count("ocr_pending")?,
        processing: count("processing")?,
        ready: count("ready")?,
        errors: count("error")?,
        unsupported: count("unsupported")?,
        offline: c.query_row("SELECT count(*) FROM roots WHERE online=0", [], |r| {
            r.get(0)
        })?,
        bytes,
        paused: e.paused.load(Ordering::Relaxed),
        scanning: e.scanning.load(Ordering::Relaxed),
        cancelled: e.cancelled.load(Ordering::Relaxed),
    })
}
pub fn setting(c: &Connection, key: &str, default: &str) -> String {
    c.query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
        r.get(0)
    })
    .unwrap_or(default.into())
}
pub fn set_setting(c: &Connection, key: &str, value: &str) -> Result<()> {
    c.execute(
        "INSERT INTO settings VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}
