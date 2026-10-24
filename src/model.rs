use std::collections::HashSet;

use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::util::{AppError, Result, as_obj_mut};

pub const DEFAULT_CLASS: &str = "org.xmind.ui.map.clockwise";
pub const CHILD_KINDS: [&str; 4] = ["attached", "summary", "callout", "detached"];

const THEME_JSON: &str = include_str!("../assets/theme.json");

const ALIASES: &[(&str, &str)] = &[
    ("mindmap", "org.xmind.ui.map.clockwise"),
    ("mindmap-balanced", "org.xmind.ui.map.clockwise"),
    ("mindmap-unbalanced", "org.xmind.ui.map.unbalanced"),
    ("logic-right", "org.xmind.ui.logic.right"),
    ("logic-left", "org.xmind.ui.logic.left"),
    ("org-down", "org.xmind.ui.org-chart.down"),
    ("tree-right", "org.xmind.ui.tree.right"),
    ("timeline", "org.xmind.ui.timeline.horizontal"),
    ("timeline-side", "org.xmind.ui.timeline.sided.horizontal"),
    ("matrix", "org.xmind.ui.spreadsheet"),
    ("grid", "org.xmind.ui.grid"),
    ("brace", "org.xmind.ui.brace.right"),
    ("fishbone", "org.xmind.ui.fishbone.leftHeaded"),
    ("treetable", "org.xmind.ui.treetable.toptitle"),
];

const OFFICIAL: &[(&str, &str)] = &[
    ("MindMap-1", "org.xmind.ui.map.clockwise"),
    ("MindMap-2", "org.xmind.ui.map.unbalanced"),
    ("LogicChart-1", "org.xmind.ui.logic.right"),
    ("OrgChart-1", "org.xmind.ui.org-chart.down"),
    ("TreeChart-1", "org.xmind.ui.tree.right"),
    ("Timeline-1", "org.xmind.ui.timeline.horizontal"),
    ("Timeline-3", "org.xmind.ui.timeline.sided.horizontal"),
    ("Matrix-1", "org.xmind.ui.spreadsheet"),
    ("Matrix-2", "org.xmind.ui.grid"),
    ("BraceMap-1", "org.xmind.ui.brace.right"),
    ("Fishbone-1", "org.xmind.ui.fishbone.leftHeaded"),
    ("TreeTable-1", "org.xmind.ui.treetable.toptitle"),
];

pub fn structure_class(name: &str) -> Result<String> {
    let name = name.trim();
    if let Some((_, class)) = ALIASES.iter().find(|(alias, _)| *alias == name) {
        return Ok((*class).to_string());
    }
    if let Some((_, class)) = OFFICIAL.iter().find(|(official, _)| *official == name) {
        return Ok((*class).to_string());
    }
    if name.starts_with("org.xmind.ui.") {
        return Ok(name.to_string());
    }
    let mut known: Vec<&str> = ALIASES.iter().map(|(a, _)| *a).collect();
    known.extend(OFFICIAL.iter().map(|(o, _)| *o));
    Err(AppError::usage(format!(
        "未知骨架 `{name}`，可用: {}",
        known.join(", ")
    )))
}

pub fn alias_for(class: &str) -> Option<&'static str> {
    ALIASES.iter().find(|(_, c)| *c == class).map(|(a, _)| *a)
}

pub fn known_class(class: &str) -> bool {
    ALIASES
        .iter()
        .chain(OFFICIAL.iter())
        .any(|(_, c)| *c == class)
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn theme() -> Value {
    serde_json::from_str(THEME_JSON).expect("assets/theme.json 必须是合法 JSON")
}

pub fn new_topic(title: &str) -> Value {
    json!({
        "id": new_id(),
        "class": "topic",
        "title": title,
        "children": { "attached": [] },
    })
}

pub fn new_sheet(title: &str, mut root: Value, class: &str) -> Value {
    if !root.get("style").is_some_and(Value::is_object) {
        root["style"] = json!({
            "id": new_id(),
            "properties": {
                "fo:color": "#0A0A0A",
                "line-color": "#087736FF",
            },
        });
    }
    let root_id = root
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut order = vec![Value::String(root_id)];
    if let Some(detached) = root
        .get("children")
        .and_then(|c| c.get("detached"))
        .and_then(Value::as_array)
    {
        for t in detached {
            if let Some(id) = t.get("id").and_then(Value::as_str) {
                order.push(Value::String(id.to_string()));
            }
        }
    }
    let extensions = if class == DEFAULT_CLASS {
        json!([])
    } else {
        json!([{
            "provider": "org.xmind.ui.skeleton.structure.style",
            "content": { "centralTopic": class }
        }])
    };
    json!({
        "id": new_id(),
        "revisionId": new_id(),
        "class": "sheet",
        "title": title,
        "rootTopic": root,
        "theme": theme(),
        "topicOverlapping": "overlap",
        "floatingTopicAutoColor": "follow-color-theme",
        "relationshipAutoColor": null,
        "zones": [],
        "arrangeableLayerOrder": order,
        "relationships": [],
        "extensions": extensions,
    })
}

pub fn metadata_json() -> Value {
    json!({
        "dataStructureVersion": "3",
        "creator": { "name": "xmind-cli-lite", "version": env!("CARGO_PKG_VERSION") },
        "layoutEngineVersion": "5",
    })
}

pub fn root(sheet: &Value) -> Result<&Value> {
    sheet
        .get("rootTopic")
        .filter(|v| v.is_object())
        .ok_or_else(|| AppError::invalid("sheet 缺少 rootTopic"))
}

pub fn root_mut(sheet: &mut Value) -> Result<&mut Value> {
    let ok = sheet.get("rootTopic").is_some_and(Value::is_object);
    if !ok {
        return Err(AppError::invalid("sheet 缺少 rootTopic"));
    }
    Ok(sheet.get_mut("rootTopic").expect("checked"))
}

pub fn children_map_mut(topic: &mut Value) -> &mut Map<String, Value> {
    let obj = as_obj_mut(topic);
    if !obj.get("children").is_some_and(Value::is_object) {
        obj.insert("children".into(), json!({ "attached": [] }));
    }
    obj.get_mut("children")
        .and_then(Value::as_object_mut)
        .expect("children object")
}

pub fn list_mut<'a>(topic: &'a mut Value, kind: &str) -> &'a mut Vec<Value> {
    let children = children_map_mut(topic);
    let slot = children
        .entry(kind.to_string())
        .or_insert_with(|| json!([]));
    if !slot.is_array() {
        *slot = json!([]);
    }
    slot.as_array_mut().expect("array")
}

pub fn attached_mut(topic: &mut Value) -> &mut Vec<Value> {
    list_mut(topic, "attached")
}

pub fn top_list_mut<'a>(topic: &'a mut Value, key: &str) -> &'a mut Vec<Value> {
    let obj = as_obj_mut(topic);
    let slot = obj.entry(key.to_string()).or_insert_with(|| json!([]));
    if !slot.is_array() {
        *slot = json!([]);
    }
    slot.as_array_mut().expect("array")
}

pub fn walk<'a>(topic: &'a Value, f: &mut impl FnMut(&'a Value)) {
    f(topic);
    for kind in CHILD_KINDS {
        if let Some(arr) = topic
            .get("children")
            .and_then(|c| c.get(kind))
            .and_then(Value::as_array)
        {
            for child in arr {
                walk(child, f);
            }
        }
    }
}

pub fn walk_paths<'a>(
    topic: &'a Value,
    path: &mut Vec<String>,
    f: &mut impl FnMut(&'a Value, &[String]),
) {
    f(topic, path);
    for kind in CHILD_KINDS {
        if let Some(arr) = topic
            .get("children")
            .and_then(|c| c.get(kind))
            .and_then(Value::as_array)
        {
            for child in arr {
                let title = child
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                path.push(title);
                walk_paths(child, path, f);
                path.pop();
            }
        }
    }
}

pub fn find_by_id<'a>(topic: &'a Value, id: &str) -> Option<&'a Value> {
    if topic.get("id").and_then(Value::as_str) == Some(id) {
        return Some(topic);
    }
    for kind in CHILD_KINDS {
        if let Some(arr) = topic
            .get("children")
            .and_then(|c| c.get(kind))
            .and_then(Value::as_array)
        {
            for child in arr {
                if let Some(found) = find_by_id(child, id) {
                    return Some(found);
                }
            }
        }
    }
    None
}

pub fn find_by_id_mut<'a>(topic: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    if topic.get("id").and_then(Value::as_str) == Some(id) {
        return Some(topic);
    }
    let children = topic.get_mut("children").and_then(Value::as_object_mut)?;
    for value in children.values_mut() {
        let Some(arr) = value.as_array_mut() else {
            continue;
        };
        for child in arr.iter_mut() {
            if let Some(found) = find_by_id_mut(child, id) {
                return Some(found);
            }
        }
    }
    None
}

pub fn collect_ids(topic: &Value, out: &mut HashSet<String>) {
    if let Some(id) = topic.get("id").and_then(Value::as_str) {
        out.insert(id.to_string());
    }
    for kind in CHILD_KINDS {
        if let Some(arr) = topic
            .get("children")
            .and_then(|c| c.get(kind))
            .and_then(Value::as_array)
        {
            for child in arr {
                collect_ids(child, out);
            }
        }
    }
}

pub fn count_topics(topic: &Value) -> usize {
    let mut n = 0;
    walk(topic, &mut |_| n += 1);
    n
}

pub fn title(topic: &Value) -> &str {
    topic
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

pub fn id(topic: &Value) -> &str {
    topic.get("id").and_then(Value::as_str).unwrap_or_default()
}
