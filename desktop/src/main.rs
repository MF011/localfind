#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use localfind_core::{db, scan, Engine};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::atomic::Ordering};
use tauri::Manager;

fn handle(e: &Engine, op: &str, args: Value) -> Result<Value, String> {
    let action = || -> Result<Value, Box<dyn std::error::Error>> {
        let mut c = e.connect()?;
        let id = args.get("id").and_then(Value::as_i64).unwrap_or(-1);
        let path = args.get("path").and_then(Value::as_str).unwrap_or("");
        match op {
            "status" => Ok(serde_json::to_value(db::status(e)?)?),
            "roots" => Ok(serde_json::to_value(db::roots(&c)?)?),
            "drives" => Ok(json!(scan::local_drives())),
            "all_drives" => {
                db::set_setting(&c, "all_disks", "true")?;
                scan::discover_drives(e, scan::local_drives())?;
                e.resume();
                Ok(Value::Null)
            }
            "add_root" => {
                let _guard = e
                    .locations
                    .lock()
                    .map_err(|_| "Не удалось изменить расположения")?;
                db::add_root(&c, path)?;
                e.resume();
                Ok(Value::Null)
            }
            "remove_root" => {
                let _guard = e
                    .locations
                    .lock()
                    .map_err(|_| "Не удалось изменить расположения")?;
                db::set_setting(&c, "all_disks", "false")?;
                db::remove_root(&c, id)?;
                Ok(Value::Null)
            }
            "exclude" => {
                let _guard = e
                    .locations
                    .lock()
                    .map_err(|_| "Не удалось изменить расположения")?;
                db::exclude(&mut c, path)?;
                Ok(Value::Null)
            }
            "exclusions" => Ok(json!(db::excluded(&c)?)),
            "unexclude" => {
                let _guard = e
                    .locations
                    .lock()
                    .map_err(|_| "Не удалось изменить расположения")?;
                c.execute("DELETE FROM exclusions WHERE path=?", [path])?;
                e.dirty.store(true, Ordering::Relaxed);
                Ok(Value::Null)
            }
            "search" => Ok(serde_json::to_value(db::search(
                &c,
                &serde_json::from_value(args.clone())?,
            )?)?),
            "preview" => Ok(serde_json::to_value(db::preview(&c, id)?)?),
            "pause" => {
                e.pause(true);
                Ok(Value::Null)
            }
            "resume" => {
                e.resume();
                Ok(Value::Null)
            }
            "cancel" => {
                e.cancelled.store(true, Ordering::Relaxed);
                Ok(Value::Null)
            }
            "retry" => {
                c.execute(
                    "UPDATE files SET state='pending',error=NULL WHERE state='error' AND (?=-1 OR id=?)",
                    [id,id],
                )?;
                e.resume();
                Ok(Value::Null)
            }
            "rebuild" => {
                let tx = c.transaction()?;
                tx.execute("DELETE FROM files", [])?;
                tx.commit()?;
                e.resume();
                Ok(Value::Null)
            }
            "errors" => {
                let rows=c.prepare("SELECT id,path,error FROM files WHERE state='error' ORDER BY id LIMIT 200")?.query_map([],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"path":r.get::<_,String>(1)?,"error":r.get::<_,String>(2)?})))?.collect::<rusqlite_bridge::Result<Vec<_>>>()?;
                Ok(json!(rows))
            }
            "settings" => {
                let mut v = serde_json::Map::new();
                for (key, default) in [
                    ("theme", "light"),
                    ("all_disks", "false"),
                    ("ocr", "true"),
                    ("workers", "auto"),
                    ("tesseract", "tesseract"),
                    ("tessdata", ""),
                ] {
                    v.insert(key.into(), json!(db::setting(&c, key, default)));
                }
                Ok(Value::Object(v))
            }
            "set_settings" => {
                let tx = c.transaction()?;
                let mut reset = false;
                for key in [
                    "theme",
                    "ocr",
                    "workers",
                    "tesseract",
                    "tessdata",
                    "all_disks",
                ] {
                    if let Some(value) = args.get(key).and_then(Value::as_str) {
                        if key == "workers"
                            && value != "auto"
                            && !value.parse::<usize>().is_ok_and(|n| (1..=128).contains(&n))
                        {
                            return Err("Неверное число обработчиков".into());
                        }
                        if matches!(key, "tesseract" | "tessdata")
                            && value != db::setting(&tx, key, "")
                        {
                            reset = true;
                        }
                        db::set_setting(&tx, key, value)?;
                    }
                }
                if reset {
                    tx.execute("DELETE FROM blocks WHERE source='ocr'", [])?;
                    tx.execute("UPDATE files SET state='pending' WHERE ext IN ('pdf','png','jpg','jpeg','tif','tiff','bmp')",[])?;
                }
                tx.commit()?;
                Ok(Value::Null)
            }
            "open" | "reveal" => {
                let file: String =
                    c.query_row("SELECT path FROM files WHERE id=?", [id], |r| r.get(0))?;
                if !std::path::Path::new(&file).is_file() {
                    return Err("Файл недоступен; проверьте подключение диска".into());
                }
                if op == "open" {
                    open::that(&file)?;
                } else {
                    #[cfg(windows)]
                    {
                        std::process::Command::new("explorer.exe")
                            .arg(format!("/select,{}", file.trim_start_matches("\\\\?\\")))
                            .spawn()?;
                    }
                    #[cfg(not(windows))]
                    {
                        open::that(
                            std::path::Path::new(&file)
                                .parent()
                                .ok_or("Нет родительской папки")?,
                        )?;
                    }
                }
                Ok(Value::Null)
            }
            _ => Err("Неизвестная операция".into()),
        }
    };
    action().map_err(|err| err.to_string())
}
#[tauri::command]
async fn api(
    engine: tauri::State<'_, Engine>,
    op: String,
    args: Option<Value>,
) -> Result<Value, String> {
    let e = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || handle(&e, &op, args.unwrap_or(json!({}))))
        .await
        .map_err(|err| err.to_string())?
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data = app.path().app_local_data_dir()?;
            let e = Engine::new(data.join("index.sqlite"))?;
            let worker = if cfg!(debug_assertions) {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../workers/extract.py")
            } else {
                app.path().resource_dir()?.join("workers/extract.py")
            };
            let python = std::env::var_os("LOCALFIND_PYTHON")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "python" } else { "python3" }));
            localfind_core::jobs::invalidate_version(&e, &worker, &python)?;
            scan::start(e.clone(), worker, python);
            app.manage(e);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![api])
        .run(tauri::generate_context!())
        .expect("Не удалось запустить приложение");
}
