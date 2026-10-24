use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::model;
use crate::util::{AppError, Result};

pub type Entries = Vec<(String, Vec<u8>)>;

pub fn read_entries(path: &Path) -> Result<Entries> {
    let file =
        File::open(path).map_err(|e| AppError::io(format!("打开 {} 失败: {e}", path.display())))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| AppError::invalid(format!("{} 不是可读的 zip/xmind: {e}", path.display())))?;
    let mut entries = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| AppError::invalid(format!("读取 zip 条目失败: {e}")))?;
        let name = entry.name()?.replace('\\', "/");
        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .map_err(|e| AppError::io(format!("读取 zip 条目 {name} 失败: {e}")))?;
        entries.push((name, data));
    }
    Ok(entries)
}

pub fn content_sheets(entries: &Entries) -> Result<Vec<Value>> {
    let raw =
        get_entry(entries, "content.json").ok_or_else(|| AppError::invalid("缺少 content.json"))?;
    let value: Value = serde_json::from_slice(raw)
        .map_err(|e| AppError::invalid(format!("content.json 不是合法 JSON: {e}")))?;
    match value {
        Value::Array(sheets) if sheets.is_empty() => {
            Err(AppError::invalid("content.json 的 sheet 数组为空"))
        }
        Value::Array(sheets) => {
            if let Some(index) = sheets.iter().position(|sheet| !sheet.is_object()) {
                return Err(AppError::invalid(format!("sheet[{index}] 不是对象")));
            }
            Ok(sheets)
        }
        _ => Err(AppError::invalid("content.json 顶层必须是 sheet 数组")),
    }
}

pub fn set_content(entries: &mut Entries, sheets: &[Value]) -> Result<()> {
    let bytes = serde_json::to_vec(&Value::Array(sheets.to_vec()))?;
    set_entry(entries, "content.json", bytes);
    Ok(())
}

pub fn get_entry<'a>(entries: &'a Entries, name: &str) -> Option<&'a [u8]> {
    entries
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, d)| d.as_slice())
}

pub fn set_entry(entries: &mut Entries, name: &str, data: Vec<u8>) {
    if let Some(slot) = entries.iter_mut().find(|(n, _)| n == name) {
        slot.1 = data;
    } else {
        entries.push((name.to_string(), data));
    }
}

pub fn add_resource(entries: &mut Entries, name: &str, data: Vec<u8>) {
    let full = format!("resources/{name}");
    if !entries.iter().any(|(n, _)| n == &full) {
        entries.push((full, data));
    }
}

pub fn resource_names(entries: &Entries) -> Vec<String> {
    entries
        .iter()
        .filter(|(n, _)| n.starts_with("resources/") && !n.ends_with('/'))
        .map(|(n, _)| n.clone())
        .collect()
}

pub fn refresh_manifest(entries: &mut Entries) {
    let mut manifest = get_entry(entries, "manifest.json")
        .and_then(|raw| serde_json::from_slice::<Value>(raw).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({ "file-entries": {} }));
    let obj = manifest.as_object_mut().expect("object");
    if !obj.get("file-entries").is_some_and(Value::is_object) {
        obj.insert("file-entries".into(), serde_json::json!({}));
    }
    let file_entries = obj
        .get_mut("file-entries")
        .and_then(Value::as_object_mut)
        .expect("file-entries");
    for name in ["content.json", "metadata.json"] {
        file_entries
            .entry(name.to_string())
            .or_insert_with(|| serde_json::json!({}));
    }
    for name in resource_names(entries) {
        file_entries
            .entry(name)
            .or_insert_with(|| serde_json::json!({}));
    }
    match serde_json::to_vec(&manifest) {
        Ok(bytes) => set_entry(entries, "manifest.json", bytes),
        Err(_) => set_entry(entries, "manifest.json", b"{\"file-entries\":{}}".to_vec()),
    }
}

pub fn write_new(entries: &Entries, out: &Path, force: bool) -> Result<()> {
    if out.exists() {
        if !force {
            return Err(AppError::io(format!(
                "{} 已存在，使用 --force 覆盖",
                out.display()
            )));
        }
        if out.is_dir() {
            return Err(AppError::io(format!("{} 是目录", out.display())));
        }
    }
    let tmp = temp_path(out);
    write_zip(entries, &tmp)?;
    commit(&tmp, out)
}

pub fn write_replace(entries: &Entries, out: &Path) -> Result<()> {
    let tmp = temp_path(out);
    write_zip(entries, &tmp)?;
    commit(&tmp, out)
}

// shortcut: 用“旧文件改名 -> 新文件改名”模拟替换；崩溃窗口仍在，但不会直接丢旧文件
fn commit(tmp: &Path, out: &Path) -> Result<()> {
    if !out.exists() {
        return fs::rename(tmp, out).map_err(|e| {
            let _ = fs::remove_file(tmp);
            AppError::io(format!("写入 {} 失败: {e}", out.display()))
        });
    }
    let tmp_old = temp_path(out);
    let old = tmp_old.with_file_name(format!(
        "{}.old",
        tmp_old.file_name().unwrap_or_default().to_string_lossy()
    ));
    if let Err(e) = fs::rename(out, &old) {
        let _ = fs::remove_file(tmp);
        return Err(AppError::io(format!("替换 {} 失败: {e}", out.display())));
    }
    match fs::rename(tmp, out) {
        Ok(()) => {
            let _ = fs::remove_file(&old);
            Ok(())
        }
        Err(e) => {
            let _ = fs::rename(&old, out);
            let _ = fs::remove_file(tmp);
            Err(AppError::io(format!("写入 {} 失败: {e}", out.display())))
        }
    }
}

fn write_zip(entries: &Entries, path: &Path) -> Result<()> {
    let file = File::create(path)
        .map_err(|e| AppError::io(format!("创建临时文件 {} 失败: {e}", path.display())))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let result = (|| -> Result<()> {
        for (name, data) in entries {
            if name.ends_with('/') {
                zip.add_directory(name, options)?;
            } else {
                zip.start_file(name, options)?;
                zip.write_all(data)?;
            }
        }
        zip.finish()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn temp_path(out: &Path) -> PathBuf {
    let name = out
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "out".to_string());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    out.with_file_name(format!(".{name}.tmp-{}-{nanos}", std::process::id()))
}

pub fn select_sheet(sheets: &[Value], query: Option<&str>) -> Result<usize> {
    if sheets.is_empty() {
        return Err(AppError::invalid("content.json 的 sheet 数组为空"));
    }
    let Some(query) = query else {
        return Ok(0);
    };
    if let Some(i) = sheets
        .iter()
        .position(|s| s.get("id").and_then(Value::as_str) == Some(query))
    {
        return Ok(i);
    }
    let matches: Vec<usize> = sheets
        .iter()
        .enumerate()
        .filter(|(_, s)| s.get("title").and_then(Value::as_str) == Some(query))
        .map(|(i, _)| i)
        .collect();
    match matches.as_slice() {
        [] => Err(AppError::usage(format!("找不到 sheet `{query}`"))),
        [i] => Ok(*i),
        _ => Err(AppError::usage(format!(
            "sheet 标题 `{query}` 不唯一，请改用 id"
        ))),
    }
}

pub fn sheet_id(sheet: &Value) -> String {
    sheet
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub fn sheet_title(sheet: &Value) -> String {
    sheet
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub fn sheet_topic_count(sheet: &Value) -> usize {
    sheet.get("rootTopic").map(model::count_topics).unwrap_or(0)
}
