use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::md;
use crate::model;
use crate::ops;
use crate::render;
use crate::util::{self, AppError, Result};
use crate::validate::{self, Report};
use crate::xmind;
use crate::{
    BatchArgs, CreateArgs, DescribeArgs, ImageArgs, LinkArgs, ReadArgs, SheetsArgs, ValidateArgs,
};

pub fn create(args: CreateArgs) -> Result<i32> {
    let cwd = current_dir()?;
    let (text, base) = match (args.stdin, args.draft.as_deref()) {
        (true, Some(_)) => {
            return Err(AppError::usage("--stdin 与 Markdown 文件不能同时使用"));
        }
        (true, None) => (util::read_stdin()?, cwd),
        (false, Some(path)) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| AppError::io(format!("读取 {} 失败: {e}", path.display())))?;
            let parent = path.parent().unwrap_or_else(|| Path::new(""));
            let parent = if parent.is_absolute() {
                parent.to_path_buf()
            } else {
                cwd.join(parent)
            };
            (text, parent)
        }
        (false, None) => {
            return Err(AppError::usage("create 需要 Markdown 文件或 --stdin"));
        }
    };

    let md::Parsed {
        mut root,
        resources,
    } = md::parse(&text, &base, args.title.as_deref())?;
    let class = model::structure_class(&args.skeleton)?;
    root["structureClass"] = Value::String(class.clone());
    let sheet_title = args.sheet.clone().unwrap_or_else(|| "Sheet 1".to_string());
    let sheet = model::new_sheet(&sheet_title, root, &class);
    let sheets = vec![sheet.clone()];

    let mut entries: xmind::Entries = vec![
        (
            "content.json".to_string(),
            serde_json::to_vec(&Value::Array(sheets))?,
        ),
        (
            "metadata.json".to_string(),
            serde_json::to_vec(&model::metadata_json())?,
        ),
    ];
    for (name, bytes) in resources {
        xmind::add_resource(&mut entries, &name, bytes);
    }
    xmind::refresh_manifest(&mut entries);

    let report = if args.no_validate {
        Report::default()
    } else {
        validate::validate_entries(&entries)
    };
    if !report.errors.is_empty() {
        return Err(AppError::validation(&report.errors));
    }

    let topics = xmind::sheet_topic_count(&sheet);
    let mut output = json!({
        "ok": true,
        "file": args.out.display().to_string(),
        "sheet": { "id": xmind::sheet_id(&sheet), "title": xmind::sheet_title(&sheet) },
        "topics": topics,
        "warnings": report.warnings,
    });
    if args.dry_run {
        output["dry_run"] = Value::Bool(true);
        println!("{}", serde_json::to_string(&output)?);
        return Ok(0);
    }
    xmind::write_new(&entries, &args.out, args.force)?;
    println!("{}", serde_json::to_string(&output)?);
    eprintln!("已写入 {}（{} 个主题）", args.out.display(), topics);
    Ok(0)
}

pub fn read(args: ReadArgs) -> Result<i32> {
    let entries = xmind::read_entries(&args.file)?;
    let sheets = xmind::content_sheets(&entries)?;
    let selected = select(&sheets, args.sheet.as_deref())?;
    match args.format {
        crate::Format::Json => {
            let mut selected = selected.into_iter().cloned().collect::<Vec<_>>();
            for sheet in &mut selected {
                render::annotate_paths(sheet);
            }
            println!("{}", serde_json::to_string_pretty(&Value::Array(selected))?);
        }
        crate::Format::Outline => {
            print!("{}", render::outline(&selected, args.depth));
        }
    }
    Ok(0)
}

pub fn describe(args: DescribeArgs) -> Result<i32> {
    let entries = xmind::read_entries(&args.file)?;
    let sheets = xmind::content_sheets(&entries)?;
    let selected = select(&sheets, args.sheet.as_deref())?;
    let info = selected
        .iter()
        .map(|sheet| render::describe_sheet(sheet))
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": true,
            "file": args.file.display().to_string(),
            "sheets": info,
        }))?
    );
    Ok(0)
}

pub fn sheets(args: SheetsArgs) -> Result<i32> {
    let entries = xmind::read_entries(&args.file)?;
    let sheets = xmind::content_sheets(&entries)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": true,
            "file": args.file.display().to_string(),
            "sheets": render::sheets_info(&sheets),
        }))?
    );
    Ok(0)
}

pub fn validate(args: ValidateArgs) -> Result<i32> {
    let mut report = match validate::validate_file(&args.file) {
        Ok(report) => report,
        Err(error) if args.quiet => return Ok(error.exit),
        Err(error) => return Err(error),
    };
    if args.strict {
        report = report.strict();
    }
    let code = if report.errors.is_empty() { 0 } else { 3 };
    if args.quiet {
        return Ok(code);
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": report.errors.is_empty(),
                "file": args.file.display().to_string(),
                "errors": report.errors,
                "warnings": report.warnings,
            }))?
        );
    } else {
        for error in &report.errors {
            println!("错误: {error}");
        }
        for warning in &report.warnings {
            println!("警告: {warning}");
        }
        if report.errors.is_empty() {
            println!("OK");
        }
    }
    Ok(code)
}

pub fn batch(args: BatchArgs) -> Result<i32> {
    let cwd = current_dir()?;
    let mut entries = xmind::read_entries(&args.file)?;
    let mut sheets = xmind::content_sheets(&entries)?;
    let index = xmind::select_sheet(&sheets, args.sheet.as_deref())?;
    let ops = read_ops(args.input.as_deref())?;
    let mut ctx = ops::Ctx::new(&cwd);
    let changes = ops::apply(&mut sheets[index], &ops, &mut ctx)?;

    xmind::set_content(&mut entries, &sheets)?;
    for (name, bytes) in ctx.resources {
        xmind::add_resource(&mut entries, &name, bytes);
    }
    xmind::refresh_manifest(&mut entries);

    let report = if args.no_validate {
        Report::default()
    } else {
        validate::validate_entries(&entries)
    };
    if !report.errors.is_empty() {
        return Err(AppError::validation(&report.errors));
    }

    let target = args.out.clone().unwrap_or_else(|| args.file.clone());
    let sheet = &sheets[index];
    let ops_count = ops.as_array().map(Vec::len).unwrap_or(0);
    let mut output = json!({
        "ok": true,
        "file": target.display().to_string(),
        "sheet": { "id": xmind::sheet_id(sheet), "title": xmind::sheet_title(sheet) },
        "topics": xmind::sheet_topic_count(sheet),
        "ops": ops_count,
        "changes": changes,
        "warnings": report.warnings,
    });
    if args.dry_run {
        output["dry_run"] = Value::Bool(true);
        println!("{}", serde_json::to_string(&output)?);
        return Ok(0);
    }
    if args.backup {
        backup(&args.file)?;
    }
    if target == args.file {
        xmind::write_replace(&entries, &target)?;
    } else {
        xmind::write_new(&entries, &target, args.force)?;
    }
    println!("{}", serde_json::to_string(&output)?);
    eprintln!("已写入 {}（{} 个主题）", target.display(), output["topics"]);
    Ok(0)
}

pub fn link(args: LinkArgs) -> Result<i32> {
    let topic = json!({ "topic": args.topic });
    let op = match (args.url, args.target_file, args.remove) {
        (Some(url), None, false) => json!({ "op": "href", "topic": topic, "href": url }),
        (None, Some(file), false) => json!({ "op": "href", "topic": topic, "file": file }),
        (None, None, true) => json!({ "op": "href", "topic": topic, "href": null }),
        _ => {
            return Err(AppError::usage(
                "link 需要 --url、--file、--remove 三者之一",
            ));
        }
    };
    mutate_one(&args.xmind, args.sheet.as_deref(), op)
}

pub fn image(args: ImageArgs) -> Result<i32> {
    if args.remove && (args.input.is_some() || args.width.is_some()) {
        return Err(AppError::usage(
            "image --remove 不能与 --input/--width 同用",
        ));
    }
    let topic = json!({ "topic": args.topic });
    let op = if args.remove {
        json!({ "op": "image", "topic": topic, "remove": true })
    } else if let Some(input) = args.input {
        let mut op = json!({ "op": "image", "topic": topic, "input": input });
        if let Some(width) = args.width {
            op["width"] = json!(width);
        }
        op
    } else {
        return Err(AppError::usage("image 需要 --input 或 --remove"));
    };
    mutate_one(&args.xmind, args.sheet.as_deref(), op)
}

fn mutate_one(file: &Path, sheet_query: Option<&str>, op: Value) -> Result<i32> {
    let cwd = current_dir()?;
    let mut entries = xmind::read_entries(file)?;
    let mut sheets = xmind::content_sheets(&entries)?;
    let index = xmind::select_sheet(&sheets, sheet_query)?;
    let mut ctx = ops::Ctx::new(&cwd);
    let changes = ops::apply(&mut sheets[index], &Value::Array(vec![op]), &mut ctx)?;
    xmind::set_content(&mut entries, &sheets)?;
    for (name, bytes) in ctx.resources {
        xmind::add_resource(&mut entries, &name, bytes);
    }
    xmind::refresh_manifest(&mut entries);
    let report = validate::validate_entries(&entries);
    if !report.errors.is_empty() {
        return Err(AppError::validation(&report.errors));
    }
    let sheet = &sheets[index];
    let output = json!({
        "ok": true,
        "file": file.display().to_string(),
        "sheet": { "id": xmind::sheet_id(sheet), "title": xmind::sheet_title(sheet) },
        "topics": xmind::sheet_topic_count(sheet),
        "changes": changes,
        "warnings": report.warnings,
    });
    xmind::write_replace(&entries, file)?;
    println!("{}", serde_json::to_string(&output)?);
    eprintln!("已写入 {}（{} 个主题）", file.display(), output["topics"]);
    Ok(0)
}

fn select<'a>(sheets: &'a [Value], query: Option<&str>) -> Result<Vec<&'a Value>> {
    match query {
        Some(query) => {
            let index = xmind::select_sheet(sheets, Some(query))?;
            Ok(vec![&sheets[index]])
        }
        None => Ok(sheets.iter().collect()),
    }
}

fn read_ops(input: Option<&Path>) -> Result<Value> {
    let text = match input {
        None => util::read_stdin()?,
        Some(path) if path == Path::new("-") => util::read_stdin()?,
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| AppError::io(format!("读取 ops {} 失败: {e}", path.display())))?,
    };
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| AppError::usage(format!("ops 不是合法 JSON: {e}")))?;
    if !value.is_array() {
        return Err(AppError::usage("ops 必须是 JSON 数组"));
    }
    Ok(value)
}

fn backup(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(".bak");
    let backup = PathBuf::from(name);
    std::fs::copy(path, &backup)
        .map_err(|e| AppError::io(format!("备份到 {} 失败: {e}", backup.display())))?;
    Ok(())
}

fn current_dir() -> Result<PathBuf> {
    std::env::current_dir().map_err(|e| AppError::io(format!("读取当前目录失败: {e}")))
}
