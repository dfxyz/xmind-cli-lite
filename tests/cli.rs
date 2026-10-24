use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50,
    0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "xmind-cli-lite-test-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xmind-cli-lite"))
}

fn run(command: &mut Command, stdin: Option<&str>) -> Output {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().expect("spawn binary");
    if let Some(stdin) = stdin {
        let mut pipe = child.stdin.take().expect("stdin");
        pipe.write_all(stdin.as_bytes()).expect("write stdin");
    }
    child.wait_with_output().expect("wait binary")
}

fn json_out(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout 不是 JSON: {e}\nstatus={:?}\nstdout={}\nstderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn assert_ok(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "命令失败: status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    json_out(output)
}

fn find_topic<'a>(topic: &'a Value, title: &str) -> Option<&'a Value> {
    if topic.get("title").and_then(Value::as_str) == Some(title) {
        return Some(topic);
    }
    for kind in ["attached", "summary", "callout", "detached"] {
        if let Some(arr) = topic
            .get("children")
            .and_then(|c| c.get(kind))
            .and_then(Value::as_array)
        {
            for child in arr {
                if let Some(found) = find_topic(child, title) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn validate(file: &Path) -> Value {
    let output = run(cmd().arg("validate").arg(file).arg("--json"), None);
    assert_eq!(
        output.status.code(),
        Some(0),
        "validate 失败: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value = json_out(&output);
    assert_eq!(value["ok"], true);
    value
}

fn zip_map(path: &Path) -> Vec<(String, Vec<u8>)> {
    let file = fs::File::open(path).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        entries.push((name, data));
    }
    entries
}

fn write_zip(path: &Path, entries: &[(String, Vec<u8>)], options: SimpleFileOptions) {
    let file = fs::File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    for (name, data) in entries {
        if name.ends_with('/') {
            zip.add_directory(name, options).unwrap();
        } else {
            zip.start_file(name, options).unwrap();
            zip.write_all(data).unwrap();
        }
    }
    zip.finish().unwrap();
}

fn find_entry<'a>(entries: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
    &entries
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("entry {name} missing"))
        .1
}

fn create_draft(path: &Path, image: Option<&Path>) {
    let image_line = image
        .map(|p| format!("![pic]({})\n", p.to_string_lossy().replace('\\', "/")))
        .unwrap_or_default();
    let text = format!(
        "# Root\n\n## Plan\n\n- item\n  > note **bold**\n\n### Detail\n\n{image_line}\n[Site](https://example.com)\n"
    );
    fs::write(path, text).unwrap();
}

fn create_file(dir: &Temp, name: &str, extra: &[&str]) -> PathBuf {
    let draft = dir.path().join("draft.md");
    create_draft(&draft, Some(&dir.path().join("pic.png")));
    fs::write(dir.path().join("pic.png"), PNG).unwrap();
    let out = dir.path().join(name);
    let mut command = cmd();
    command.arg("create").arg(&draft).arg("-o").arg(&out);
    for arg in extra {
        command.arg(arg);
    }
    assert_ok(&run(&mut command, None));
    out
}

#[test]
fn create_read_batch_validate_roundtrip() {
    let dir = Temp::new();
    let file = create_file(&dir, "out.xmind", &[]);

    let value = validate(&file);
    assert_eq!(value["errors"], json!([]));

    let output = run(
        cmd().arg("read").arg(&file).arg("--format").arg("json"),
        None,
    );
    let sheets = assert_ok(&output);
    let sheets = sheets.as_array().unwrap();
    assert_eq!(sheets.len(), 1);
    let root = &sheets[0]["rootTopic"];
    assert_eq!(root["path"], json!([]));
    assert_eq!(root["title"], "Root");
    assert_eq!(root["style"]["properties"]["fo:color"], "#0A0A0A");
    assert_eq!(root["style"]["properties"]["line-color"], "#087736FF");
    let detail = find_topic(root, "Detail").unwrap();
    assert_eq!(detail["path"], json!(["Plan", "Detail"]));
    assert_eq!(detail["image"]["width"], 1);
    let site = find_topic(root, "Site").unwrap();
    assert_eq!(site["href"], "https://example.com");
    let item = find_topic(root, "item").unwrap();
    assert!(
        item["notes"]["plain"]["content"]
            .as_str()
            .unwrap()
            .contains("note bold")
    );

    let ops = json!([
        {"op":"add","parent":{"path":["Plan"]},"title":"New","index":0},
        {"op":"boundary","parent":{"path":["Plan"]},"range":[0,1],"title":"Group"},
        {"op":"summary","parent":{"path":["Plan"]},"range":[0,1],"text":"Both"},
        {"op":"relationship","from":{"path":["Plan","Detail"]},"to":{"path":["Plan","New"]},"label":"dep"},
        {"op":"floating","title":"Parking","position":{"x":10,"y":20}},
        {"op":"position","topic":{"path":["Parking"]},"x":30,"y":40}
    ])
    .to_string();
    let output = run(
        cmd().arg("batch").arg(&file).arg("--input").arg("-"),
        Some(&ops),
    );
    let value = assert_ok(&output);
    assert_eq!(value["ops"], 6);
    assert_eq!(value["changes"].as_array().unwrap().len(), 6);
    validate(&file);

    let output = run(
        cmd().arg("read").arg(&file).arg("--format").arg("outline"),
        None,
    );
    let outline = String::from_utf8(output.stdout).unwrap();
    for needle in ["Floating topics", "boundary:", "summary:", "relationship:"] {
        assert!(
            outline.contains(needle),
            "outline 缺少 {needle}:\n{outline}"
        );
    }

    let output = run(cmd().arg("describe").arg(&file), None);
    let desc = assert_ok(&output);
    assert_eq!(desc["sheets"][0]["features"]["boundaries"], 1);
    assert_eq!(desc["sheets"][0]["features"]["summaries"], 1);
    assert_eq!(desc["sheets"][0]["features"]["relationships"], 1);
    assert_eq!(desc["sheets"][0]["features"]["floating"], 1);
}

#[test]
fn image_and_link_commands() {
    let dir = Temp::new();
    let file = create_file(&dir, "out.xmind", &[]);
    let png = dir.path().join("pic.png");

    let output = run(
        cmd()
            .arg("image")
            .arg(&file)
            .arg("--topic")
            .arg("Detail")
            .arg("--input")
            .arg(&png)
            .arg("--width")
            .arg("64"),
        None,
    );
    assert_ok(&output);
    validate(&file);
    let entries = zip_map(&file);
    let content: Value = serde_json::from_slice(find_entry(&entries, "content.json")).unwrap();
    let detail = find_topic(&content[0]["rootTopic"], "Detail").unwrap();
    assert_eq!(detail["image"]["width"], 64);
    let resource = entries
        .iter()
        .find(|(name, _)| name.starts_with("resources/"))
        .expect("resource entry");
    let manifest: Value = serde_json::from_slice(find_entry(&entries, "manifest.json")).unwrap();
    assert!(manifest["file-entries"].get(&resource.0).is_some());

    let target = dir.path().join("target.txt");
    fs::write(&target, "x").unwrap();
    let output = run(
        cmd()
            .arg("link")
            .arg(&file)
            .arg("--topic")
            .arg("Plan")
            .arg("--file")
            .arg(&target),
        None,
    );
    assert_ok(&output);
    let content = run(
        cmd().arg("read").arg(&file).arg("--format").arg("json"),
        None,
    );
    let sheets = assert_ok(&content);
    let plan = find_topic(&sheets[0]["rootTopic"], "Plan").unwrap();
    assert!(plan["href"].as_str().unwrap().starts_with("file:///"));

    let output = run(
        cmd()
            .arg("link")
            .arg(&file)
            .arg("--topic")
            .arg("Plan")
            .arg("--remove"),
        None,
    );
    assert_ok(&output);
    let content = run(
        cmd().arg("read").arg(&file).arg("--format").arg("json"),
        None,
    );
    let sheets = assert_ok(&content);
    let plan = find_topic(&sheets[0]["rootTopic"], "Plan").unwrap();
    assert!(plan.get("href").is_none());
    validate(&file);
}

#[test]
fn exit_codes_and_dry_run() {
    let dir = Temp::new();
    let draft = dir.path().join("draft.md");
    fs::write(&draft, "# Root\n").unwrap();
    let out = dir.path().join("out.xmind");
    assert_ok(&run(
        cmd().arg("create").arg(&draft).arg("-o").arg(&out),
        None,
    ));

    let output = run(cmd().arg("create").arg(&draft).arg("-o").arg(&out), None);
    assert_eq!(output.status.code(), Some(1));
    let error = json_out(&output);
    assert_eq!(error["ok"], false);
    assert_eq!(error["error"]["code"], "io");

    let not_zip = dir.path().join("bad.xmind");
    fs::write(&not_zip, "hello").unwrap();
    let output = run(cmd().arg("validate").arg(&not_zip).arg("--json"), None);
    assert_eq!(output.status.code(), Some(3));
    let report = json_out(&output);
    assert_eq!(report["ok"], false);
    assert!(!report["errors"].as_array().unwrap().is_empty());

    let output = run(
        cmd().arg("batch").arg(&out).arg("--input").arg("-"),
        Some("[{\"op\":\"theme\"}]"),
    );
    assert_eq!(output.status.code(), Some(2));

    let dry = dir.path().join("dry.xmind");
    let output = run(
        cmd()
            .arg("create")
            .arg(&draft)
            .arg("-o")
            .arg(&dry)
            .arg("--dry-run"),
        None,
    );
    let value = assert_ok(&output);
    assert_eq!(value["dry_run"], true);
    assert!(!dry.exists());

    let output = run(
        cmd()
            .arg("create")
            .arg("--stdin")
            .arg("-o")
            .arg(dir.path().join("stdin.xmind"))
            .arg("--title")
            .arg("Root"),
        Some("## A\n"),
    );
    assert_ok(&output);
    let output = run(
        cmd()
            .arg("create")
            .arg("--stdin")
            .arg("-o")
            .arg(dir.path().join("stdin2.xmind")),
        Some("## A\n"),
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn skeleton_extension_and_multiple_sheets() {
    let dir = Temp::new();
    let file = create_file(&dir, "logic.xmind", &["--skeleton", "logic-right"]);
    let entries = zip_map(&file);
    let content: Value = serde_json::from_slice(find_entry(&entries, "content.json")).unwrap();
    assert_eq!(
        content[0]["rootTopic"]["structureClass"],
        "org.xmind.ui.logic.right"
    );
    assert_eq!(
        content[0]["extensions"][0]["content"]["centralTopic"],
        "org.xmind.ui.logic.right"
    );

    let base = create_file(&dir, "base.xmind", &[]);
    let mut entries = zip_map(&base);
    let mut sheets: Vec<Value> =
        serde_json::from_slice(find_entry(&entries, "content.json")).unwrap();
    let mut second = sheets[0].clone();
    let mut mapping = HashMap::new();
    collect_topic_ids(&second["rootTopic"], &mut mapping);
    for new_id in mapping.values_mut() {
        *new_id = uuid::Uuid::new_v4().to_string();
    }
    remap(&mut second, &mapping);
    second["id"] = json!(uuid::Uuid::new_v4().to_string());
    second["revisionId"] = json!(uuid::Uuid::new_v4().to_string());
    second["title"] = json!("Second");
    sheets.push(second);
    let updated = serde_json::to_vec(&Value::Array(sheets)).unwrap();
    for entry in &mut entries {
        if entry.0 == "content.json" {
            entry.1 = updated.clone();
        }
    }
    let multi = dir.path().join("multi.xmind");
    write_zip(
        &multi,
        &entries,
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    );
    validate(&multi);

    let ops = json!([
        {"op":"update","topic":{"path":["Plan","item"]},"title":"Second changed"}
    ])
    .to_string();
    assert_ok(&run(
        cmd()
            .arg("batch")
            .arg(&multi)
            .arg("--sheet")
            .arg("Second")
            .arg("--input")
            .arg("-"),
        Some(&ops),
    ));
    validate(&multi);

    let read_sheet = |sheet: &str| -> Value {
        let output = run(
            cmd()
                .arg("read")
                .arg(&multi)
                .arg("--sheet")
                .arg(sheet)
                .arg("--format")
                .arg("json"),
            None,
        );
        assert_ok(&output)
    };
    let first = read_sheet("Sheet 1");
    let second = read_sheet("Second");
    assert!(find_topic(&first[0]["rootTopic"], "Second changed").is_none());
    assert!(find_topic(&second[0]["rootTopic"], "Second changed").is_some());
}

fn collect_topic_ids(topic: &Value, out: &mut HashMap<String, String>) {
    if let Some(id) = topic.get("id").and_then(Value::as_str) {
        out.insert(id.to_string(), String::new());
    }
    for kind in ["attached", "summary", "callout", "detached"] {
        if let Some(arr) = topic
            .get("children")
            .and_then(|c| c.get(kind))
            .and_then(Value::as_array)
        {
            for child in arr {
                collect_topic_ids(child, out);
            }
        }
    }
}

fn remap(value: &mut Value, mapping: &HashMap<String, String>) {
    match value {
        Value::Object(obj) => {
            for key in ["id", "topicId", "end1Id", "end2Id"] {
                if let Some(old) = obj.get(key).and_then(Value::as_str)
                    && let Some(new) = mapping.get(old)
                {
                    obj.insert(key.to_string(), json!(new));
                }
            }
            if let Some(order) = obj
                .get_mut("arrangeableLayerOrder")
                .and_then(Value::as_array_mut)
            {
                for item in order {
                    if let Some(old) = item.as_str()
                        && let Some(new) = mapping.get(old)
                    {
                        *item = json!(new);
                    }
                }
            }
            for child in obj.values_mut() {
                remap(child, mapping);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                remap(item, mapping);
            }
        }
        _ => {}
    }
}

#[test]
fn reference_templates_read_validate_roundtrip() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("reference/templates");
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("xmind") {
            continue;
        }
        let before = zip_map(&path);
        let copy = Temp::new();
        let target = copy.path().join("copy.xmind");
        fs::copy(&path, &target).unwrap();
        let output = run(
            cmd().arg("batch").arg(&target).arg("--input").arg("-"),
            Some("[]"),
        );
        assert!(
            output.status.success(),
            "{} 重写失败: {}",
            path.display(),
            String::from_utf8_lossy(&output.stdout)
        );
        validate(&target);
        let after = zip_map(&target);
        let names = |entries: &[(String, Vec<u8>)]| {
            let mut names: Vec<String> = entries.iter().map(|(n, _)| n.clone()).collect();
            names.sort();
            names
        };
        assert_eq!(names(&before), names(&after), "{} 条目丢失", path.display());
    }
}
