use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

use crate::model;
use crate::util::{AppError, Result};
use crate::xmind::{self, Entries};

#[derive(Default, Clone)]
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl Report {
    pub fn error(&mut self, message: impl Into<String>) {
        self.errors.push(message.into());
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    pub fn strict(mut self) -> Self {
        self.errors.append(&mut self.warnings);
        self
    }
}

pub fn validate_file(path: &Path) -> Result<Report> {
    if !path.exists() {
        return Err(AppError::io(format!("文件不存在: {}", path.display())));
    }
    match xmind::read_entries(path) {
        Ok(entries) => Ok(validate_entries(&entries)),
        Err(e) if e.code == "io" => Err(e),
        Err(e) => {
            let mut report = Report::default();
            report.error(e.message);
            Ok(report)
        }
    }
}

pub fn validate_entries(entries: &Entries) -> Report {
    let mut report = Report::default();
    check_manifest(entries, &mut report);
    if xmind::get_entry(entries, "metadata.json").is_none() {
        report.error("缺少 metadata.json");
    }
    let Some(raw) = xmind::get_entry(entries, "content.json") else {
        report.error("缺少 content.json");
        return report;
    };
    let parsed: Value = match serde_json::from_slice(raw) {
        Ok(value) => value,
        Err(e) => {
            report.error(format!("content.json 不是合法 JSON: {e}"));
            return report;
        }
    };
    let sheets = match parsed {
        Value::Array(sheets) if sheets.is_empty() => {
            report.error("content.json 的 sheet 数组为空");
            return report;
        }
        Value::Array(sheets) => sheets,
        _ => {
            report.error("content.json 顶层必须是 sheet 数组");
            return report;
        }
    };

    let mut global_sheet_ids = HashSet::new();
    let mut global_topic_ids = HashSet::new();
    for (index, sheet) in sheets.iter().enumerate() {
        let Some(obj) = sheet.as_object() else {
            report.error(format!("sheet[{index}] 不是对象"));
            continue;
        };
        let title = obj
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let label = if title.is_empty() {
            format!("sheet[{index}]")
        } else {
            format!("sheet[{index}] `{title}`")
        };
        match obj.get("id").and_then(Value::as_str) {
            Some(id) => {
                if !global_sheet_ids.insert(id.to_string()) {
                    report.error(format!("{label}: sheet id `{id}` 重复"));
                }
            }
            None => report.error(format!("{label}: 缺少 sheet id")),
        }
        if let Some(t) = obj.get("title")
            && !t.is_string()
        {
            report.error(format!("{label}: sheet title 必须是字符串"));
        }
        let Some(root) = obj.get("rootTopic") else {
            report.error(format!("{label}: 缺少 rootTopic"));
            continue;
        };
        if !root.is_object() {
            report.error(format!("{label}: rootTopic 不是对象"));
            continue;
        }

        let mut ctx = WalkCtx {
            entries,
            label: label.clone(),
            ids: HashSet::new(),
            boundary_ids: HashSet::new(),
            global_topic_ids: &mut global_topic_ids,
            classes: HashSet::new(),
            stats: Stats::default(),
        };
        walk_structure(root, 0, &mut ctx, &mut report);
        if ctx.stats.max_depth > 8 {
            report.warn(format!("{label}: 主题深度 {} 超过 8", ctx.stats.max_depth));
        }
        if ctx.stats.count > 1500 {
            report.warn(format!("{label}: 节点数 {} 超过 1500", ctx.stats.count));
        }
        for class in ctx.classes.iter() {
            report.warn(format!("{label}: 未知 structureClass `{class}`"));
        }
        if !ctx.stats.root_has_class {
            report.warn(format!("{label}: 根主题缺少 structureClass"));
        }
        walk_refs(root, &ctx.ids, &mut report, &label);
        let mut ref_ids = ctx.ids.clone();
        ref_ids.extend(ctx.boundary_ids.iter().cloned());
        if let Some(zones) = obj.get("zones").and_then(Value::as_array) {
            for zone in zones {
                if let Some(zone_id) = zone.get("id").and_then(Value::as_str) {
                    ref_ids.insert(zone_id.to_string());
                }
            }
        }
        check_relationships(sheet, &ref_ids, &mut report, &label);
    }
    report
}

struct Stats {
    count: usize,
    max_depth: usize,
    root_has_class: bool,
    root: bool,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            count: 0,
            max_depth: 0,
            root_has_class: false,
            root: true,
        }
    }
}

struct WalkCtx<'a> {
    entries: &'a Entries,
    label: String,
    ids: HashSet<String>,
    boundary_ids: HashSet<String>,
    global_topic_ids: &'a mut HashSet<String>,
    classes: HashSet<String>,
    stats: Stats,
}

fn walk_structure(topic: &Value, depth: usize, ctx: &mut WalkCtx, report: &mut Report) {
    let Some(obj) = topic.as_object() else {
        report.error(format!("{}: topic 不是对象", ctx.label));
        return;
    };
    ctx.stats.count += 1;
    ctx.stats.max_depth = ctx.stats.max_depth.max(depth);
    if ctx.stats.root {
        ctx.stats.root_has_class = obj.get("structureClass").is_some();
        ctx.stats.root = false;
    }

    let id = match obj.get("id").and_then(Value::as_str) {
        Some(id) => id,
        None => {
            report.error(format!("{}: topic 缺少 id", ctx.label));
            "<missing>"
        }
    };
    if !ctx.ids.insert(id.to_string()) {
        report.error(format!("{}: topic id `{id}` 重复", ctx.label));
    } else if !ctx.global_topic_ids.insert(id.to_string()) {
        report.error(format!("{}: topic id `{id}` 跨 sheet 重复", ctx.label));
    }

    match obj.get("title") {
        Some(Value::String(t)) => {
            if t.trim().is_empty() {
                report.warn(format!("{}: 主题 `{id}` 标题为空", ctx.label));
            }
        }
        Some(_) => report.error(format!("{}: 主题 `{id}` title 必须是字符串", ctx.label)),
        None => report.error(format!("{}: 主题 `{id}` 缺少 title", ctx.label)),
    }

    if let Some(class) = obj.get("structureClass") {
        match class.as_str() {
            Some(class) => {
                if !model::known_class(class) {
                    ctx.classes.insert(class.to_string());
                }
            }
            None => report.error(format!(
                "{}: 主题 `{id}` structureClass 必须是字符串",
                ctx.label
            )),
        }
    }

    if let Some(notes) = obj.get("notes")
        && !notes.is_null()
    {
        let ok = notes
            .get("plain")
            .and_then(|p| p.get("content"))
            .and_then(Value::as_str)
            .is_some()
            && notes
                .get("realHTML")
                .and_then(|p| p.get("content"))
                .and_then(Value::as_str)
                .is_some();
        if !ok {
            report.error(format!("{}: 主题 `{id}` notes 结构非法", ctx.label));
        }
    }

    if let Some(image) = obj.get("image") {
        check_image(image, id, ctx, report);
    }

    if let Some(labels) = obj.get("labels") {
        match labels.as_array() {
            Some(arr) if arr.iter().all(Value::is_string) => {}
            _ => report.error(format!(
                "{}: 主题 `{id}` labels 必须是字符串数组",
                ctx.label
            )),
        }
    }

    if let Some(markers) = obj.get("markers") {
        let ok = markers.as_array().is_some_and(|arr| {
            arr.iter().all(|m| {
                m.get("markerId")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty())
            })
        });
        if !ok {
            report.error(format!("{}: 主题 `{id}` markers 结构非法", ctx.label));
        }
    }

    if let Some(href) = obj.get("href")
        && !href.is_string()
    {
        report.error(format!("{}: 主题 `{id}` href 必须是字符串", ctx.label));
    }

    if let Some(children) = obj.get("children") {
        if !children.is_object() {
            report.error(format!("{}: 主题 `{id}` children 不是对象", ctx.label));
        } else {
            for kind in model::CHILD_KINDS {
                if let Some(value) = children.get(kind)
                    && !value.is_array()
                {
                    report.error(format!(
                        "{}: 主题 `{id}` children.{kind} 不是数组",
                        ctx.label
                    ));
                }
            }
        }
    }

    let attached_len = obj
        .get("children")
        .and_then(|c| c.get("attached"))
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if let Some(boundaries) = obj.get("boundaries") {
        match boundaries.as_array() {
            Some(arr) => {
                for boundary in arr {
                    if let Some(boundary_id) = boundary.get("id").and_then(Value::as_str) {
                        ctx.boundary_ids.insert(boundary_id.to_string());
                    }
                    check_range_entry(boundary, "boundary", id, attached_len, ctx, report);
                }
            }
            None => report.error(format!("{}: 主题 `{id}` boundaries 必须是数组", ctx.label)),
        }
    }
    if let Some(summaries) = obj.get("summaries") {
        match summaries.as_array() {
            Some(arr) => {
                for summary in arr {
                    check_range_entry(summary, "summary", id, attached_len, ctx, report);
                    if summary.get("topicId").and_then(Value::as_str).is_none() {
                        report.error(format!("{}: 主题 `{id}` summary 缺少 topicId", ctx.label));
                    }
                }
            }
            None => report.error(format!("{}: 主题 `{id}` summaries 必须是数组", ctx.label)),
        }
    }

    if let Some(children) = obj.get("children").and_then(Value::as_object) {
        for kind in ["attached", "detached"] {
            if let Some(arr) = children.get(kind).and_then(Value::as_array) {
                let mut seen = HashSet::new();
                for child in arr {
                    let title = child
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !title.trim().is_empty() && !seen.insert(title.to_string()) {
                        report.warn(format!("{}: 主题 `{id}` 下重名 `{title}`", ctx.label));
                    }
                }
            }
        }
        for kind in model::CHILD_KINDS {
            if let Some(arr) = children.get(kind).and_then(Value::as_array) {
                for child in arr {
                    walk_structure(child, depth + 1, ctx, report);
                }
            }
        }
    }
}

fn check_image(image: &Value, topic_id: &str, ctx: &WalkCtx, report: &mut Report) {
    if !image.is_object() {
        report.error(format!("{}: 主题 `{topic_id}` image 必须是对象", ctx.label));
        return;
    }
    let Some(src) = image.get("src").and_then(Value::as_str) else {
        report.error(format!("{}: 主题 `{topic_id}` image 缺少 src", ctx.label));
        return;
    };
    let Some(name) = src.strip_prefix("xap:resources/") else {
        report.error(format!(
            "{}: 主题 `{topic_id}` image.src 必须是 xap:resources/...",
            ctx.label
        ));
        return;
    };
    if name.is_empty() || xmind::get_entry(ctx.entries, &format!("resources/{name}")).is_none() {
        report.error(format!(
            "{}: 主题 `{topic_id}` 的图片资源 `{name}` 在 zip 中不存在",
            ctx.label
        ));
    }
    if let Some(width) = image.get("width") {
        let ok = width.as_f64().is_some_and(|w| w > 0.0 && w.fract() == 0.0);
        if !ok {
            report.error(format!(
                "{}: 主题 `{topic_id}` image.width 必须是正整数",
                ctx.label
            ));
        }
    }
}

fn check_range_entry(
    value: &Value,
    kind: &str,
    parent_id: &str,
    attached_len: usize,
    ctx: &WalkCtx,
    report: &mut Report,
) {
    if !value.is_object() {
        report.error(format!(
            "{}: 主题 `{parent_id}` {kind} 必须是对象",
            ctx.label
        ));
        return;
    }
    if value.get("id").and_then(Value::as_str).is_none() {
        report.error(format!(
            "{}: 主题 `{parent_id}` 的 {kind} 缺少 id",
            ctx.label
        ));
    }
    let Some(range) = value.get("range").and_then(Value::as_str) else {
        report.error(format!(
            "{}: 主题 `{parent_id}` 的 {kind} 缺少 range",
            ctx.label
        ));
        return;
    };
    let Some((start, end)) = parse_range(range) else {
        report.error(format!(
            "{}: 主题 `{parent_id}` 的 {kind} range `{range}` 格式非法",
            ctx.label
        ));
        return;
    };
    if start > end || end >= attached_len {
        report.error(format!(
            "{}: 主题 `{parent_id}` 的 {kind} range `{range}` 越界",
            ctx.label
        ));
    }
}

fn walk_refs(topic: &Value, ids: &HashSet<String>, report: &mut Report, label: &str) {
    let parent_id = topic.get("id").and_then(Value::as_str).unwrap_or_default();
    let summary_ids: HashSet<&str> = topic
        .get("children")
        .and_then(|c| c.get("summary"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.get("id").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    if let Some(arr) = topic.get("summaries").and_then(Value::as_array) {
        for summary in arr {
            let Some(topic_id) = summary.get("topicId").and_then(Value::as_str) else {
                continue;
            };
            if !ids.contains(topic_id) {
                report.error(format!(
                    "{label}: 主题 `{parent_id}` 的 summary 引用不存在的 topicId `{topic_id}`"
                ));
            } else if !summary_ids.contains(topic_id) {
                report.error(format!(
                    "{label}: 主题 `{parent_id}` 的 summary topic `{topic_id}` 不在 children.summary 中"
                ));
            }
        }
    }
    if let Some(children) = topic.get("children").and_then(Value::as_object) {
        for kind in model::CHILD_KINDS {
            if let Some(arr) = children.get(kind).and_then(Value::as_array) {
                for child in arr {
                    walk_refs(child, ids, report, label);
                }
            }
        }
    }
}

fn check_relationships(sheet: &Value, ids: &HashSet<String>, report: &mut Report, label: &str) {
    let Some(rels) = sheet.get("relationships") else {
        return;
    };
    let Some(rels) = rels.as_array() else {
        report.error(format!("{label}: relationships 必须是数组"));
        return;
    };
    let mut seen = HashSet::new();
    for rel in rels {
        let Some(obj) = rel.as_object() else {
            report.error(format!("{label}: relationship 必须是对象"));
            continue;
        };
        if let Some(id) = obj.get("id").and_then(Value::as_str) {
            if !seen.insert(id.to_string()) {
                report.error(format!("{label}: relationship id `{id}` 重复"));
            }
        } else {
            report.error(format!("{label}: relationship 缺少 id"));
        }
        for end in ["end1Id", "end2Id"] {
            match obj.get(end).and_then(Value::as_str) {
                Some(id) if ids.contains(id) => {}
                Some(id) => report.error(format!("{label}: relationship 端点 `{id}` 不存在")),
                None => report.error(format!("{label}: relationship 缺少 {end}")),
            }
        }
        if let Some(title) = obj.get("title")
            && !title.is_string()
        {
            report.error(format!("{label}: relationship title 必须是字符串"));
        }
    }
}

fn check_manifest(entries: &Entries, report: &mut Report) {
    let Some(raw) = xmind::get_entry(entries, "manifest.json") else {
        report.error("缺少 manifest.json");
        return;
    };
    let manifest: Value = match serde_json::from_slice(raw) {
        Ok(value) => value,
        Err(e) => {
            report.error(format!("manifest.json 不是合法 JSON: {e}"));
            return;
        }
    };
    let Some(file_entries) = manifest.get("file-entries").and_then(Value::as_object) else {
        report.error("manifest.json 缺少 file-entries");
        return;
    };
    for name in ["content.json", "metadata.json"] {
        if !file_entries.contains_key(name) {
            report.error(format!("manifest 未登记 {name}"));
        }
    }
    for name in xmind::resource_names(entries) {
        if !file_entries.contains_key(&name) {
            report.error(format!("manifest 未登记资源 {name}"));
        }
    }
}

fn parse_range(s: &str) -> Option<(usize, usize)> {
    let inner = s.strip_prefix('(')?.strip_suffix(')')?;
    let (a, b) = inner.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}
