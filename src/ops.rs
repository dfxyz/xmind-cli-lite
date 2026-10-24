use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::model;
use crate::util::{self, AppError, Result};

pub struct Ctx {
    pub base: PathBuf,
    pub resources: Vec<(String, Vec<u8>)>,
}

impl Ctx {
    pub fn new(base: &Path) -> Self {
        Self {
            base: base.to_path_buf(),
            resources: Vec::new(),
        }
    }
}

pub fn apply(sheet: &mut Value, ops: &Value, ctx: &mut Ctx) -> Result<Vec<String>> {
    let ops = ops
        .as_array()
        .ok_or_else(|| AppError::usage("ops 必须是 JSON 数组"))?;
    let mut changes = Vec::new();
    for op in ops {
        let obj = op
            .as_object()
            .ok_or_else(|| AppError::usage("每个 op 必须是 JSON 对象"))?;
        let name = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::usage("op 缺少 `op` 字段"))?;
        let change = match name {
            "add" => op_add(sheet, op, ctx)?,
            "update" => op_update(sheet, op)?,
            "delete" => op_delete(sheet, op)?,
            "move" => op_move(sheet, op)?,
            "notes" => op_notes(sheet, op, ctx)?,
            "href" => op_href(sheet, op, ctx)?,
            "label" => op_label(sheet, op)?,
            "marker" => op_marker(sheet, op)?,
            "image" => op_image(sheet, op, ctx)?,
            "layout" => op_layout(sheet, op)?,
            "boundary" => op_boundary(sheet, op)?,
            "summary" => op_summary(sheet, op)?,
            "relationship" => op_relationship(sheet, op)?,
            "floating" => op_floating(sheet, op)?,
            "position" => op_position(sheet, op)?,
            other => {
                return Err(AppError::usage(format!(
                    "不支持的操作 `{other}`（可用: add/update/delete/move/notes/href/label/marker/image/layout/boundary/summary/relationship/floating/position）"
                )));
            }
        };
        changes.push(change);
    }
    Ok(changes)
}

#[derive(Clone)]
enum Loc {
    Id(String),
    Title(String),
    Path(Vec<String>),
}

fn parse_loc(value: &Value, field: &str) -> Result<Loc> {
    match value {
        Value::String(s) => Ok(Loc::Title(s.clone())),
        Value::Object(obj) => {
            let mut locs = Vec::new();
            if let Some(id) = obj.get("id") {
                locs.push(Loc::Id(
                    id.as_str()
                        .ok_or_else(|| AppError::usage(format!("{field}.id 必须是字符串")))?
                        .to_string(),
                ));
            }
            if let Some(path) = obj.get("path") {
                let arr = path
                    .as_array()
                    .ok_or_else(|| AppError::usage(format!("{field}.path 必须是字符串数组")))?;
                let mut parts = Vec::with_capacity(arr.len());
                for part in arr {
                    parts.push(
                        part.as_str()
                            .ok_or_else(|| {
                                AppError::usage(format!("{field}.path 必须是字符串数组"))
                            })?
                            .to_string(),
                    );
                }
                locs.push(Loc::Path(parts));
            }
            if let Some(topic) = obj.get("topic") {
                locs.push(Loc::Title(
                    topic
                        .as_str()
                        .ok_or_else(|| AppError::usage(format!("{field}.topic 必须是字符串")))?
                        .to_string(),
                ));
            }
            match locs.len() {
                1 => Ok(locs.remove(0)),
                0 => Err(AppError::usage(format!(
                    "{field} 必须是字符串或包含 id/topic/path 的对象"
                ))),
                _ => Err(AppError::usage(format!(
                    "{field} 只能包含 id、topic、path 之一"
                ))),
            }
        }
        _ => Err(AppError::usage(format!(
            "{field} 必须是字符串或包含 id/topic/path 的对象"
        ))),
    }
}

fn unique_title(sheet: &Value, title: &str) -> Result<String> {
    let root = model::root(sheet)?;
    let mut hits: Vec<(String, String)> = Vec::new();
    let mut path = Vec::new();
    model::walk_paths(root, &mut path, &mut |topic, path| {
        if model::title(topic) == title {
            let shown = if path.is_empty() {
                "[]".to_string()
            } else {
                path.join(" / ")
            };
            hits.push((model::id(topic).to_string(), shown));
        }
    });
    match hits.as_slice() {
        [] => Err(AppError::usage(format!("找不到标题为 `{title}` 的主题"))),
        [(id, _)] => Ok(id.clone()),
        _ => {
            let list = hits
                .iter()
                .map(|(id, path)| format!("{path} (id {id})"))
                .collect::<Vec<_>>()
                .join("；");
            Err(AppError::usage(format!(
                "标题 `{title}` 不唯一，候选: {list}"
            )))
        }
    }
}

fn resolve<'a>(sheet: &'a Value, loc: &Loc) -> Result<&'a Value> {
    let root = model::root(sheet)?;
    match loc {
        Loc::Id(id) => model::find_by_id(root, id)
            .ok_or_else(|| AppError::usage(format!("找不到 id 为 `{id}` 的主题"))),
        Loc::Title(title) => {
            let id = unique_title(sheet, title)?;
            model::find_by_id(root, &id)
                .ok_or_else(|| AppError::usage(format!("找不到标题为 `{title}` 的主题")))
        }
        Loc::Path(path) => resolve_path(root, path),
    }
}

fn resolve_mut<'a>(sheet: &'a mut Value, loc: &Loc) -> Result<&'a mut Value> {
    match loc {
        Loc::Id(id) => {
            let root = model::root_mut(sheet)?;
            model::find_by_id_mut(root, id)
                .ok_or_else(|| AppError::usage(format!("找不到 id 为 `{id}` 的主题")))
        }
        Loc::Title(title) => {
            let id = unique_title(sheet, title)?;
            let root = model::root_mut(sheet)?;
            model::find_by_id_mut(root, &id)
                .ok_or_else(|| AppError::usage(format!("找不到标题为 `{title}` 的主题")))
        }
        Loc::Path(path) => {
            let root = model::root_mut(sheet)?;
            resolve_path_mut(root, path)
        }
    }
}

fn resolve_id(sheet: &Value, loc: &Loc) -> Result<String> {
    Ok(model::id(resolve(sheet, loc)?).to_string())
}

fn resolve_path<'a>(root: &'a Value, path: &[String]) -> Result<&'a Value> {
    let mut current = root;
    for (i, segment) in path.iter().enumerate() {
        let count = named_children(current, segment).len();
        if count == 0 {
            return Err(AppError::usage(format!(
                "path 第 {} 段 `{segment}` 不存在",
                i + 1
            )));
        }
        if count > 1 {
            return Err(AppError::usage(format!(
                "path 第 {} 段 `{segment}` 重名，请改用 id",
                i + 1
            )));
        }
        current = named_children(current, segment)[0];
    }
    Ok(current)
}

fn resolve_path_mut<'a>(root: &'a mut Value, path: &[String]) -> Result<&'a mut Value> {
    let mut current = root;
    for (i, segment) in path.iter().enumerate() {
        let count = named_children(current, segment).len();
        if count == 0 {
            return Err(AppError::usage(format!(
                "path 第 {} 段 `{segment}` 不存在",
                i + 1
            )));
        }
        if count > 1 {
            return Err(AppError::usage(format!(
                "path 第 {} 段 `{segment}` 重名，请改用 id",
                i + 1
            )));
        }
        current = named_child_mut(current, segment).expect("checked");
    }
    Ok(current)
}

fn named_children<'a>(topic: &'a Value, title: &str) -> Vec<&'a Value> {
    let mut out = Vec::new();
    for kind in ["attached", "detached"] {
        if let Some(arr) = topic
            .get("children")
            .and_then(|c| c.get(kind))
            .and_then(Value::as_array)
        {
            for child in arr {
                if model::title(child) == title {
                    out.push(child);
                }
            }
        }
    }
    out
}

fn named_child_mut<'a>(topic: &'a mut Value, title: &str) -> Option<&'a mut Value> {
    let children = topic.get_mut("children")?.as_object_mut()?;
    for (kind, value) in children.iter_mut() {
        if kind != "attached" && kind != "detached" {
            continue;
        }
        let Some(arr) = value.as_array_mut() else {
            continue;
        };
        for child in arr.iter_mut() {
            if model::title(child) == title {
                return Some(child);
            }
        }
    }
    None
}

enum Container {
    Child {
        parent_id: String,
        kind: &'static str,
        index: usize,
    },
}

fn find_container(topic: &Value, id: &str) -> Option<Container> {
    for kind in model::CHILD_KINDS {
        if let Some(arr) = topic
            .get("children")
            .and_then(|c| c.get(kind))
            .and_then(Value::as_array)
        {
            for (index, child) in arr.iter().enumerate() {
                if model::id(child) == id {
                    return Some(Container::Child {
                        parent_id: model::id(topic).to_string(),
                        kind,
                        index,
                    });
                }
                if let Some(found) = find_container(child, id) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn contains_id(topic: &Value, id: &str) -> bool {
    model::find_by_id(topic, id).is_some()
}

fn in_array(topic: &Value, id: &str, kind: &str) -> bool {
    topic
        .get("children")
        .and_then(|c| c.get(kind))
        .and_then(Value::as_array)
        .is_some_and(|arr| arr.iter().any(|t| contains_id(t, id)))
}

fn inside_callout(topic: &Value, id: &str) -> bool {
    if in_array(topic, id, "callout") {
        return true;
    }
    for kind in ["attached", "summary", "detached"] {
        if let Some(arr) = topic
            .get("children")
            .and_then(|c| c.get(kind))
            .and_then(Value::as_array)
        {
            for child in arr {
                if inside_callout(child, id) {
                    return true;
                }
            }
        }
    }
    false
}

fn ensure_writable(sheet: &Value, loc: &Loc) -> Result<String> {
    let id = resolve_id(sheet, loc)?;
    if inside_callout(model::root(sheet)?, &id) {
        return Err(AppError::usage("callout 主题只读"));
    }
    Ok(id)
}

fn is_floating_top(sheet: &Value, id: &str) -> bool {
    model::root(sheet)
        .ok()
        .and_then(|r| r.get("children"))
        .and_then(|c| c.get("detached"))
        .and_then(Value::as_array)
        .is_some_and(|arr| arr.iter().any(|t| model::id(t) == id))
}

fn op_add(sheet: &mut Value, op: &Value, ctx: &Ctx) -> Result<String> {
    let parent_loc = parse_loc(
        op.get("parent")
            .ok_or_else(|| AppError::usage("add 缺少 parent"))?,
        "parent",
    )?;
    let parent_id = ensure_writable(sheet, &parent_loc)?;
    let title = op
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::usage("add 缺少 title"))?;
    let mut topic = model::new_topic(title);
    if let Some(notes) = op.get("notes") {
        let notes = notes
            .as_str()
            .ok_or_else(|| AppError::usage("add.notes 必须是字符串"))?;
        topic["notes"] = crate::md::notes_value(notes, &ctx.base);
    }
    if let Some(labels) = op.get("labels") {
        let labels = string_array(labels, "add.labels")?;
        if !labels.is_empty() {
            topic["labels"] = json!(labels);
        }
    }
    if let Some(markers) = op.get("markers") {
        let markers = string_array(markers, "add.markers")?;
        if !markers.is_empty() {
            topic["markers"] = json!(
                markers
                    .iter()
                    .map(|m| json!({ "markerId": m }))
                    .collect::<Vec<_>>()
            );
        }
    }
    if let Some(href) = op.get("href") {
        let href = href
            .as_str()
            .ok_or_else(|| AppError::usage("add.href 必须是字符串"))?;
        topic["href"] = Value::String(href.to_string());
    }
    let index = opt_index(op, "add")?;
    let parent = resolve_mut(sheet, &Loc::Id(parent_id.clone()))?;
    let arr = model::attached_mut(parent);
    insert_at(arr, topic, index, "add")?;
    Ok(format!("add {parent_id} -> {title}"))
}

fn op_update(sheet: &mut Value, op: &Value) -> Result<String> {
    let loc = target_loc(op, "update")?;
    let id = ensure_writable(sheet, &loc)?;
    let title = op
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::usage("update 缺少 title"))?;
    let topic = resolve_mut(sheet, &Loc::Id(id.clone()))?;
    topic["title"] = Value::String(title.to_string());
    Ok(format!("update {id}"))
}

fn op_delete(sheet: &mut Value, op: &Value) -> Result<String> {
    let loc = target_loc(op, "delete")?;
    let target = resolve(sheet, &loc)?.clone();
    let id = model::id(&target).to_string();
    let root_id = model::id(model::root(sheet)?).to_string();
    if id == root_id {
        return Err(AppError::usage("不能删除根主题"));
    }
    if inside_callout(model::root(sheet)?, &id) {
        return Err(AppError::usage("callout 主题只读"));
    }
    let (_, mut extra) = remove_one(sheet, &id)?;
    let mut removed = HashSet::new();
    model::collect_ids(&target, &mut removed);
    removed.extend(extra.drain());
    cleanup_summary_refs(sheet, &removed);
    cleanup_relationships(sheet, &removed);
    Ok(format!("delete {id}"))
}

fn op_move(sheet: &mut Value, op: &Value) -> Result<String> {
    let topics = op
        .get("topics")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::usage("move 缺少 topics 数组"))?;
    if topics.is_empty() {
        return Err(AppError::usage("move.topics 不能为空"));
    }
    let parent_loc = parse_loc(
        op.get("parent")
            .ok_or_else(|| AppError::usage("move 缺少 parent"))?,
        "parent",
    )?;
    let parent_id = ensure_writable(sheet, &parent_loc)?;
    let mut moves = Vec::new();
    for value in topics {
        let loc = parse_loc(value, "move.topics[]")?;
        let id = ensure_writable(sheet, &loc)?;
        let topic = resolve(sheet, &Loc::Id(id.clone()))?.clone();
        moves.push((id, topic));
    }
    let mut seen = HashSet::new();
    for (id, _) in &moves {
        if !seen.insert(id.clone()) {
            return Err(AppError::usage(format!("move.topics 重复包含 `{id}`")));
        }
    }
    for (i, (_, topic)) in moves.iter().enumerate() {
        for (j, (id, _)) in moves.iter().enumerate() {
            if i != j && contains_id(topic, id) {
                return Err(AppError::usage("要移动的主题互为祖先，请分开执行"));
            }
        }
    }
    let root_id = model::id(model::root(sheet)?).to_string();
    if moves.iter().any(|(id, _)| id == &root_id) {
        return Err(AppError::usage("不能移动根主题"));
    }
    let dest = resolve(sheet, &Loc::Id(parent_id.clone()))?;
    if moves
        .iter()
        .any(|(_, topic)| contains_id(topic, &parent_id))
    {
        return Err(AppError::usage("不能移动到自身子树"));
    }
    if dest.get("id").and_then(Value::as_str).is_none() {
        return Err(AppError::usage("目标 parent 缺少 id"));
    }

    let index = opt_index(op, "move")?;
    let mut extra = HashSet::new();
    let mut removed_topics = Vec::new();
    for (id, _) in &moves {
        let floating = is_floating_top(sheet, id);
        let (mut topic, extras) = remove_one(sheet, id)?;
        extra.extend(extras);
        if floating {
            topic.as_object_mut().expect("topic").remove("position");
        }
        removed_topics.push(topic);
    }
    cleanup_relationships(sheet, &extra);

    let parent = resolve_mut(sheet, &Loc::Id(parent_id.clone()))?;
    let arr = model::attached_mut(parent);
    let start = index.unwrap_or(arr.len());
    if start > arr.len() {
        return Err(AppError::usage(format!(
            "move.index {start} 超出目标列表长度 {}",
            arr.len()
        )));
    }
    for (offset, topic) in removed_topics.into_iter().enumerate() {
        arr.insert(start + offset, topic);
    }
    Ok(format!("move {} -> {parent_id}", moves.len()))
}

fn op_notes(sheet: &mut Value, op: &Value, ctx: &Ctx) -> Result<String> {
    let loc = target_loc(op, "notes")?;
    let id = ensure_writable(sheet, &loc)?;
    let value = op
        .get("notes")
        .ok_or_else(|| AppError::usage("notes 缺少 notes 字段"))?;
    let topic = resolve_mut(sheet, &Loc::Id(id.clone()))?;
    match value {
        Value::Null => {
            topic.as_object_mut().expect("topic").remove("notes");
        }
        Value::String(text) => {
            topic["notes"] = crate::md::notes_value(text, &ctx.base);
        }
        _ => return Err(AppError::usage("notes 必须是字符串或 null")),
    }
    Ok(format!("notes {id}"))
}

fn op_href(sheet: &mut Value, op: &Value, ctx: &Ctx) -> Result<String> {
    let loc = target_loc(op, "href")?;
    let id = ensure_writable(sheet, &loc)?;
    let has_href = op.get("href").is_some();
    let has_file = op.get("file").is_some();
    if has_href == has_file {
        return Err(AppError::usage("href 需要 href 或 file 二者之一"));
    }
    let topic = resolve_mut(sheet, &Loc::Id(id.clone()))?;
    if has_href {
        match &op["href"] {
            Value::Null => {
                topic.as_object_mut().expect("topic").remove("href");
            }
            Value::String(s) => {
                topic["href"] = Value::String(s.clone());
            }
            _ => return Err(AppError::usage("href 必须是字符串或 null")),
        }
    } else {
        let file = op["file"]
            .as_str()
            .ok_or_else(|| AppError::usage("file 必须是字符串"))?;
        topic["href"] = Value::String(util::file_uri(Path::new(file), &ctx.base));
    }
    Ok(format!("href {id}"))
}

fn op_label(sheet: &mut Value, op: &Value) -> Result<String> {
    let loc = target_loc(op, "label")?;
    let id = ensure_writable(sheet, &loc)?;
    let add = string_array_opt(op.get("add"), "label.add")?;
    let remove = string_array_opt(op.get("remove"), "label.remove")?;
    let topic = resolve_mut(sheet, &Loc::Id(id.clone()))?;
    let mut labels: Vec<String> = topic
        .get("labels")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    for label in add {
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels.retain(|label| !remove.contains(label));
    if labels.is_empty() {
        topic.as_object_mut().expect("topic").remove("labels");
    } else {
        topic["labels"] = json!(labels);
    }
    Ok(format!("label {id}"))
}

fn op_marker(sheet: &mut Value, op: &Value) -> Result<String> {
    let loc = target_loc(op, "marker")?;
    let id = ensure_writable(sheet, &loc)?;
    let add = string_array_opt(op.get("add"), "marker.add")?;
    let remove = string_array_opt(op.get("remove"), "marker.remove")?;
    let topic = resolve_mut(sheet, &Loc::Id(id.clone()))?;
    let mut markers: Vec<String> = topic
        .get("markers")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("markerId").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    for marker in add {
        if !markers.contains(&marker) {
            markers.push(marker);
        }
    }
    markers.retain(|marker| !remove.contains(marker));
    if markers.is_empty() {
        topic.as_object_mut().expect("topic").remove("markers");
    } else {
        topic["markers"] = json!(
            markers
                .iter()
                .map(|m| json!({ "markerId": m }))
                .collect::<Vec<_>>()
        );
    }
    Ok(format!("marker {id}"))
}

fn op_image(sheet: &mut Value, op: &Value, ctx: &mut Ctx) -> Result<String> {
    let loc = target_loc(op, "image")?;
    let id = ensure_writable(sheet, &loc)?;
    let remove = op.get("remove").and_then(Value::as_bool).unwrap_or(false);
    if remove {
        if op.get("input").is_some() {
            return Err(AppError::usage("image.remove 不能同时提供 input"));
        }
        let topic = resolve_mut(sheet, &Loc::Id(id.clone()))?;
        topic.as_object_mut().expect("topic").remove("image");
        return Ok(format!("image-remove {id}"));
    }
    let input = op
        .get("input")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::usage("image 需要 input 或 remove: true"))?;
    let width = match op.get("width") {
        None => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or_else(|| AppError::usage("image.width 必须是正整数"))?,
        ),
    };
    let path = Path::new(input);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        ctx.base.join(path)
    };
    let image = util::load_image(&path, width)?;
    if !ctx.resources.iter().any(|(name, _)| name == &image.name) {
        ctx.resources.push((image.name.clone(), image.bytes));
    }
    let topic = resolve_mut(sheet, &Loc::Id(id.clone()))?;
    topic["image"] = json!({
        "src": format!("xap:resources/{}", image.name),
        "width": image.width,
    });
    Ok(format!("image {id}"))
}

fn op_layout(sheet: &mut Value, op: &Value) -> Result<String> {
    let loc = target_loc(op, "layout")?;
    let id = ensure_writable(sheet, &loc)?;
    let layout = op
        .get("layout")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::usage("layout 缺少 layout"))?;
    let class = model::structure_class(layout)?;
    let topic = resolve_mut(sheet, &Loc::Id(id.clone()))?;
    topic["structureClass"] = Value::String(class);
    Ok(format!("layout {id}"))
}

fn op_boundary(sheet: &mut Value, op: &Value) -> Result<String> {
    let parent_loc = parse_loc(
        op.get("parent")
            .ok_or_else(|| AppError::usage("boundary 缺少 parent"))?,
        "parent",
    )?;
    let parent_id = ensure_writable(sheet, &parent_loc)?;
    let (start, end) = requested_range(sheet, &parent_id, op, "boundary")?;
    let parent = resolve_mut(sheet, &Loc::Id(parent_id.clone()))?;
    let mut boundary = json!({
        "id": model::new_id(),
        "range": format!("({start},{end})"),
        "style": {
            "id": model::new_id(),
            "properties": { "svg:fill": "#08773680" }
        }
    });
    if let Some(title) = op.get("title") {
        let title = title
            .as_str()
            .ok_or_else(|| AppError::usage("boundary.title 必须是字符串"))?;
        boundary["title"] = Value::String(title.to_string());
    }
    model::top_list_mut(parent, "boundaries").push(boundary);
    Ok(format!("boundary {parent_id} ({start},{end})"))
}

fn op_summary(sheet: &mut Value, op: &Value) -> Result<String> {
    let parent_loc = parse_loc(
        op.get("parent")
            .ok_or_else(|| AppError::usage("summary 缺少 parent"))?,
        "parent",
    )?;
    let parent_id = ensure_writable(sheet, &parent_loc)?;
    let (start, end) = requested_range(sheet, &parent_id, op, "summary")?;
    let text = match op.get("text") {
        None => String::new(),
        Some(v) => v
            .as_str()
            .ok_or_else(|| AppError::usage("summary.text 必须是字符串"))?
            .to_string(),
    };
    let parent = resolve_mut(sheet, &Loc::Id(parent_id.clone()))?;
    let summary_topic = model::new_topic(text.trim());
    let topic_id = model::id(&summary_topic).to_string();
    model::list_mut(parent, "summary").push(summary_topic);
    model::top_list_mut(parent, "summaries").push(json!({
        "id": model::new_id(),
        "range": format!("({start},{end})"),
        "topicId": topic_id,
    }));
    Ok(format!("summary {parent_id} ({start},{end})"))
}

fn op_relationship(sheet: &mut Value, op: &Value) -> Result<String> {
    let from_loc = parse_loc(
        op.get("from")
            .ok_or_else(|| AppError::usage("relationship 缺少 from"))?,
        "from",
    )?;
    let to_loc = parse_loc(
        op.get("to")
            .ok_or_else(|| AppError::usage("relationship 缺少 to"))?,
        "to",
    )?;
    let from_id = resolve_id(sheet, &from_loc)?;
    let to_id = resolve_id(sheet, &to_loc)?;
    let label = match op.get("label") {
        None => "",
        Some(v) => v
            .as_str()
            .ok_or_else(|| AppError::usage("relationship.label 必须是字符串"))?,
    };
    let rel = json!({
        "id": model::new_id(),
        "end1Id": from_id,
        "end2Id": to_id,
        "title": label,
    });
    let sheet_obj = sheet
        .as_object_mut()
        .ok_or_else(|| AppError::invalid("sheet 不是对象"))?;
    sheet_obj.insert("relationshipAutoColor".into(), Value::Null);
    let slot = sheet_obj
        .entry("relationships".to_string())
        .or_insert_with(|| json!([]));
    if !slot.is_array() {
        *slot = json!([]);
    }
    slot.as_array_mut().expect("array").push(rel);
    Ok(format!("relationship {from_id} -> {to_id}"))
}

fn op_floating(sheet: &mut Value, op: &Value) -> Result<String> {
    let title = op
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::usage("floating 缺少 title"))?;
    let mut topic = model::new_topic(title);
    let position = match op.get("position") {
        None => json!({ "x": 0, "y": 0 }),
        Some(v) => {
            let obj = v
                .as_object()
                .ok_or_else(|| AppError::usage("floating.position 必须是对象"))?;
            let x = obj
                .get("x")
                .and_then(Value::as_f64)
                .ok_or_else(|| AppError::usage("floating.position 缺少数字 x"))?;
            let y = obj
                .get("y")
                .and_then(Value::as_f64)
                .ok_or_else(|| AppError::usage("floating.position 缺少数字 y"))?;
            json!({ "x": x, "y": y })
        }
    };
    topic["position"] = position;
    if let Some(style) = op.get("style") {
        let properties = style
            .as_object()
            .ok_or_else(|| AppError::usage("floating.style 必须是 properties 对象"))?;
        topic["style"] = json!({
            "id": model::new_id(),
            "properties": properties,
        });
    }
    let id = model::id(&topic).to_string();
    let root = model::root_mut(sheet)?;
    model::list_mut(root, "detached").push(topic);
    model::top_list_mut(sheet, "arrangeableLayerOrder").push(Value::String(id.clone()));
    Ok(format!("floating {id}"))
}

fn op_position(sheet: &mut Value, op: &Value) -> Result<String> {
    let loc = target_loc(op, "position")?;
    let id = ensure_writable(sheet, &loc)?;
    if !is_floating_top(sheet, &id) {
        return Err(AppError::usage("position 只对根级浮动主题有效"));
    }
    let x = op
        .get("x")
        .and_then(Value::as_f64)
        .ok_or_else(|| AppError::usage("position 缺少数字 x"))?;
    let y = op
        .get("y")
        .and_then(Value::as_f64)
        .ok_or_else(|| AppError::usage("position 缺少数字 y"))?;
    let topic = resolve_mut(sheet, &Loc::Id(id.clone()))?;
    topic["position"] = json!({ "x": x, "y": y });
    Ok(format!("position {id}"))
}

fn target_loc(op: &Value, name: &str) -> Result<Loc> {
    parse_loc(
        op.get("topic")
            .ok_or_else(|| AppError::usage(format!("{name} 缺少 topic")))?,
        "topic",
    )
}

fn string_array(value: &Value, field: &str) -> Result<Vec<String>> {
    let arr = value
        .as_array()
        .ok_or_else(|| AppError::usage(format!("{field} 必须是字符串数组")))?;
    arr.iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| AppError::usage(format!("{field} 必须是字符串数组")))
        })
        .collect()
}

fn string_array_opt(value: Option<&Value>, field: &str) -> Result<Vec<String>> {
    match value {
        None => Ok(Vec::new()),
        Some(v) => string_array(v, field),
    }
}

fn opt_index(op: &Value, name: &str) -> Result<Option<usize>> {
    match op.get("index") {
        None => Ok(None),
        Some(v) => {
            let n = v
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| AppError::usage(format!("{name}.index 必须是非负整数")))?;
            Ok(Some(n))
        }
    }
}

fn insert_at(arr: &mut Vec<Value>, value: Value, index: Option<usize>, name: &str) -> Result<()> {
    match index {
        None => {
            arr.push(value);
            Ok(())
        }
        Some(i) if i <= arr.len() => {
            arr.insert(i, value);
            Ok(())
        }
        Some(i) => Err(AppError::usage(format!(
            "{name}.index {i} 超出列表长度 {}",
            arr.len()
        ))),
    }
}

fn requested_range(
    sheet: &Value,
    parent_id: &str,
    op: &Value,
    name: &str,
) -> Result<(usize, usize)> {
    let parent = resolve(sheet, &Loc::Id(parent_id.to_string()))?;
    let len = parent
        .get("children")
        .and_then(|c| c.get("attached"))
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let has_range = op.get("range").is_some();
    let has_topics = op.get("topics").is_some();
    if has_range == has_topics {
        return Err(AppError::usage(format!(
            "{name} 需要 range 或 topics 二者之一"
        )));
    }
    if has_range {
        let range = op["range"]
            .as_array()
            .ok_or_else(|| AppError::usage(format!("{name}.range 必须是 [start,end]")))?;
        if range.len() != 2 {
            return Err(AppError::usage(format!("{name}.range 必须是 [start,end]")));
        }
        let start = range[0]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| AppError::usage(format!("{name}.range 下标必须是非负整数")))?;
        let end = range[1]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| AppError::usage(format!("{name}.range 下标必须是非负整数")))?;
        if start > end || end >= len {
            return Err(AppError::usage(format!(
                "{name}.range ({start},{end}) 越界，父节点有 {len} 个子主题"
            )));
        }
        return Ok((start, end));
    }
    let topics = op["topics"]
        .as_array()
        .ok_or_else(|| AppError::usage(format!("{name}.topics 必须是数组")))?;
    if topics.is_empty() {
        return Err(AppError::usage(format!("{name}.topics 不能为空")));
    }
    let attached = parent
        .get("children")
        .and_then(|c| c.get("attached"))
        .and_then(Value::as_array)
        .expect("attached");
    let mut indices = Vec::new();
    for value in topics {
        let loc = parse_loc(value, &format!("{name}.topics[]"))?;
        let child_id = resolve_id(sheet, &loc)?;
        let index = attached
            .iter()
            .position(|t| model::id(t) == child_id)
            .ok_or_else(|| {
                AppError::usage(format!(
                    "{name}.topics 中的 `{child_id}` 不是父节点的直接子主题"
                ))
            })?;
        if indices.contains(&index) {
            return Err(AppError::usage(format!("{name}.topics 重复包含同一个主题")));
        }
        indices.push(index);
    }
    indices.sort_unstable();
    let start = indices[0];
    let end = *indices.last().expect("non-empty");
    if end - start + 1 != indices.len() {
        return Err(AppError::usage(format!("{name} 只能覆盖连续兄弟节点")));
    }
    Ok((start, end))
}

fn remove_one(sheet: &mut Value, id: &str) -> Result<(Value, HashSet<String>)> {
    let container = find_container(model::root(sheet)?, id)
        .ok_or_else(|| AppError::usage("找不到要删除的主题"))?;
    let (parent_id, kind, index) = match container {
        Container::Child {
            parent_id,
            kind,
            index,
        } => (parent_id, kind, index),
    };
    let mut extra = HashSet::new();
    let root = model::root_mut(sheet)?;
    let parent =
        model::find_by_id_mut(root, &parent_id).ok_or_else(|| AppError::usage("找不到父主题"))?;
    let removed = match kind {
        "attached" => {
            let arr = model::list_mut(parent, "attached");
            if arr.get(index).map(model::id) != Some(id) {
                return Err(AppError::usage("主题位置已变化，请重试"));
            }
            let removed = arr.remove(index);
            adjust_parent_ranges(parent, index, &mut extra);
            removed
        }
        "summary" => {
            let arr = model::list_mut(parent, "summary");
            let removed = arr.remove(index);
            if let Some(refs) = parent.get_mut("summaries").and_then(Value::as_array_mut) {
                refs.retain(|r| r.get("topicId").and_then(Value::as_str) != Some(id));
            }
            removed
        }
        "detached" => {
            let arr = model::list_mut(parent, "detached");
            let removed = arr.remove(index);
            remove_from_order(sheet, id);
            removed
        }
        _ => return Err(AppError::usage("callout 主题只读")),
    };
    Ok((removed, extra))
}

fn remove_from_order(sheet: &mut Value, id: &str) {
    if let Some(arr) = sheet
        .get_mut("arrangeableLayerOrder")
        .and_then(Value::as_array_mut)
    {
        arr.retain(|v| v.as_str() != Some(id));
    }
}

fn adjust_parent_ranges(parent: &mut Value, removed_index: usize, extra: &mut HashSet<String>) {
    if let Some(arr) = parent.get_mut("boundaries").and_then(Value::as_array_mut) {
        arr.retain_mut(|boundary| {
            let Some((start, end)) = boundary
                .get("range")
                .and_then(Value::as_str)
                .and_then(parse_range)
            else {
                return true;
            };
            match shift_range(start, end, removed_index) {
                Some((s, e)) => {
                    boundary["range"] = Value::String(format!("({s},{e})"));
                    true
                }
                None => false,
            }
        });
    }
    let mut drop_ids = HashSet::new();
    if let Some(arr) = parent.get_mut("summaries").and_then(Value::as_array_mut) {
        arr.retain_mut(|summary| {
            let Some((start, end)) = summary
                .get("range")
                .and_then(Value::as_str)
                .and_then(parse_range)
            else {
                return true;
            };
            match shift_range(start, end, removed_index) {
                Some((s, e)) => {
                    summary["range"] = Value::String(format!("({s},{e})"));
                    true
                }
                None => {
                    if let Some(id) = summary.get("topicId").and_then(Value::as_str) {
                        drop_ids.insert(id.to_string());
                    }
                    false
                }
            }
        });
    }
    if drop_ids.is_empty() {
        return;
    }
    if let Some(arr) = parent
        .get_mut("children")
        .and_then(|c| c.get_mut("summary"))
        .and_then(Value::as_array_mut)
    {
        arr.retain(|topic| {
            if drop_ids.contains(model::id(topic)) {
                model::collect_ids(topic, extra);
                false
            } else {
                true
            }
        });
    }
}

fn shift_range(start: usize, end: usize, removed: usize) -> Option<(usize, usize)> {
    if end < removed {
        Some((start, end))
    } else if start > removed {
        Some((start - 1, end - 1))
    } else if end > start {
        Some((start, end - 1))
    } else {
        None
    }
}

fn parse_range(s: &str) -> Option<(usize, usize)> {
    let inner = s.strip_prefix('(')?.strip_suffix(')')?;
    let (a, b) = inner.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn cleanup_summary_refs(topic: &mut Value, removed: &HashSet<String>) {
    if let Some(arr) = topic.get_mut("summaries").and_then(Value::as_array_mut) {
        arr.retain(|summary| {
            summary
                .get("topicId")
                .and_then(Value::as_str)
                .is_none_or(|id| !removed.contains(id))
        });
    }
    if let Some(children) = topic.get_mut("children").and_then(Value::as_object_mut) {
        for kind in model::CHILD_KINDS {
            if let Some(arr) = children.get_mut(kind).and_then(Value::as_array_mut) {
                for child in arr {
                    cleanup_summary_refs(child, removed);
                }
            }
        }
    }
}

fn cleanup_relationships(sheet: &mut Value, removed: &HashSet<String>) {
    if let Some(arr) = sheet.get_mut("relationships").and_then(Value::as_array_mut) {
        arr.retain(|rel| {
            let end1 = rel.get("end1Id").and_then(Value::as_str);
            let end2 = rel.get("end2Id").and_then(Value::as_str);
            !(end1.is_some_and(|id| removed.contains(id))
                || end2.is_some_and(|id| removed.contains(id)))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sheet() -> Value {
        let mut root = model::new_topic("Root");
        let mut a = model::new_topic("A");
        model::attached_mut(&mut a).push(model::new_topic("X"));
        model::attached_mut(&mut a).push(model::new_topic("Y"));
        model::attached_mut(&mut root).push(a);
        model::attached_mut(&mut root).push(model::new_topic("B"));
        model::new_sheet("Sheet 1", root, model::DEFAULT_CLASS)
    }

    fn ctx() -> Ctx {
        Ctx::new(&std::env::temp_dir())
    }

    fn find_opt<'a>(topic: &'a Value, title: &str) -> Option<&'a Value> {
        if model::title(topic) == title {
            return Some(topic);
        }
        for kind in model::CHILD_KINDS {
            if let Some(arr) = topic
                .get("children")
                .and_then(|c| c.get(kind))
                .and_then(Value::as_array)
            {
                for child in arr {
                    if let Some(found) = find_opt(child, title) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    fn find<'a>(topic: &'a Value, title: &str) -> &'a Value {
        find_opt(topic, title).unwrap_or_else(|| panic!("topic {title} not found"))
    }

    fn attached(topic: &Value) -> Vec<String> {
        topic
            .get("children")
            .and_then(|c| c.get("attached"))
            .and_then(Value::as_array)
            .map(|arr| arr.iter().map(|t| model::title(t).to_string()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn range_ops_and_move_keep_structure_valid() {
        let mut sheet = sheet();
        let ops = json!([
            {"op":"boundary","parent":{"path":["A"]},"range":[0,1],"title":"AB"},
            {"op":"summary","parent":{"path":["A"]},"range":[0,1],"text":"Both"},
            {"op":"relationship","from":{"path":["A","X"]},"to":{"path":["B"]},"label":"R"},
            {"op":"floating","title":"F","position":{"x":1,"y":2}}
        ]);
        apply(&mut sheet, &ops, &mut ctx()).unwrap();
        assert_eq!(
            find(&sheet["rootTopic"], "A")["boundaries"][0]["range"],
            "(0,1)"
        );
        assert_eq!(
            find(&sheet["rootTopic"], "A")["summaries"][0]["range"],
            "(0,1)"
        );
        assert_eq!(sheet["relationshipAutoColor"], Value::Null);
        assert_eq!(sheet["arrangeableLayerOrder"].as_array().unwrap().len(), 2);

        let ops = json!([
            {"op":"move","topics":[{"path":["A","X"]}],"parent":{"path":["B"]}}
        ]);
        apply(&mut sheet, &ops, &mut ctx()).unwrap();
        let a = find(&sheet["rootTopic"], "A");
        assert_eq!(attached(a), ["Y"]);
        assert_eq!(a["boundaries"][0]["range"], "(0,0)");
        assert_eq!(a["summaries"][0]["range"], "(0,0)");
        assert_eq!(attached(find(&sheet["rootTopic"], "B")), ["X"]);
        let rels = sheet["relationships"].as_array().unwrap();
        assert_eq!(rels.len(), 1, "移动主题不应删除关系");
    }

    #[test]
    fn deleting_last_ranged_child_drops_boundary_and_summary() {
        let mut sheet = sheet();
        let ops = json!([
            {"op":"boundary","parent":{"path":["A"]},"range":[1,1]},
            {"op":"summary","parent":{"path":["A"]},"range":[1,1],"text":"Only Y"}
        ]);
        apply(&mut sheet, &ops, &mut ctx()).unwrap();
        let ops = json!([{"op":"delete","topic":{"path":["A","Y"]}}]);
        apply(&mut sheet, &ops, &mut ctx()).unwrap();
        let a = find(&sheet["rootTopic"], "A");
        assert!(
            a.get("boundaries")
                .and_then(Value::as_array)
                .is_none_or(|v| v.is_empty())
        );
        assert!(
            a.get("summaries")
                .and_then(Value::as_array)
                .is_none_or(|v| v.is_empty())
        );
        assert_eq!(attached(a), ["X"]);
    }

    #[test]
    fn delete_floating_updates_layer_order() {
        let mut sheet = sheet();
        apply(
            &mut sheet,
            &json!([{"op":"floating","title":"F","position":{"x":0,"y":0}}]),
            &mut ctx(),
        )
        .unwrap();
        apply(
            &mut sheet,
            &json!([{"op":"delete","topic":{"path":["F"]}}]),
            &mut ctx(),
        )
        .unwrap();
        assert_eq!(sheet["arrangeableLayerOrder"].as_array().unwrap().len(), 1);
        assert!(
            sheet["rootTopic"]["children"]["detached"]
                .as_array()
                .is_none_or(|arr| arr.is_empty())
        );
    }

    #[test]
    fn rejects_invalid_move_and_position() {
        let mut sheet = sheet();
        let err = apply(
            &mut sheet,
            &json!([{"op":"move","topics":[{"path":["A"]}],"parent":{"path":["A","X"]}}]),
            &mut ctx(),
        )
        .unwrap_err();
        assert!(err.message.contains("自身子树"), "{}", err.message);
        let err = apply(
            &mut sheet,
            &json!([{"op":"position","topic":{"path":["A"]},"x":1,"y":2}]),
            &mut ctx(),
        )
        .unwrap_err();
        assert!(err.message.contains("浮动"), "{}", err.message);
    }
}
