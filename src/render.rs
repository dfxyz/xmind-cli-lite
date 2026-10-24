use std::collections::HashMap;

use serde_json::{Value, json};

use crate::model;
use crate::xmind;

pub fn annotate_paths(sheet: &mut Value) {
    let Some(root) = sheet.get_mut("rootTopic") else {
        return;
    };
    root["path"] = json!([]);
    let mut path = Vec::new();
    annotate_children(root, &mut path);
}

fn annotate_children(topic: &mut Value, path: &mut Vec<String>) {
    for kind in ["attached", "detached"] {
        let Some(arr) = topic
            .get_mut("children")
            .and_then(|c| c.get_mut(kind))
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        for child in arr.iter_mut() {
            path.push(model::title(child).to_string());
            child["path"] = json!(path);
            annotate_children(child, path);
            path.pop();
        }
    }
}

pub fn outline(sheets: &[&Value], depth: Option<usize>) -> String {
    let mut out = String::new();
    for (index, sheet) in sheets.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let title = xmind::sheet_title(sheet);
        out.push_str(&format!("# {title}\n"));
        let Some(root) = sheet.get("rootTopic") else {
            continue;
        };
        render_topic(root, 0, depth, &mut out, None);
        if let Some(detached) = root
            .get("children")
            .and_then(|c| c.get("detached"))
            .and_then(Value::as_array)
            .filter(|arr| !arr.is_empty())
        {
            out.push_str("\n## Floating topics\n");
            for floating in detached {
                let position = floating.get("position").and_then(|p| {
                    Some((
                        p.get("x").and_then(Value::as_f64)?,
                        p.get("y").and_then(Value::as_f64)?,
                    ))
                });
                render_topic(floating, 0, depth, &mut out, position);
            }
        }
        out.push_str("\n## Structure\n");
        render_structure(sheet, root, &mut out);
    }
    out
}

fn render_topic(
    topic: &Value,
    level: usize,
    max_depth: Option<usize>,
    out: &mut String,
    floating: Option<(f64, f64)>,
) {
    let indent = "  ".repeat(level);
    out.push_str(&format!("{indent}- {}", model::title(topic)));
    if let Some((x, y)) = floating {
        out.push_str(&format!(" ({x}, {y})"));
    }
    out.push('\n');
    if let Some(notes) = topic
        .get("notes")
        .and_then(|n| n.get("plain"))
        .and_then(|p| p.get("content"))
        .and_then(Value::as_str)
    {
        for line in notes.lines() {
            out.push_str(&format!("{indent}  > {line}\n"));
        }
    }
    if let Some(href) = topic.get("href").and_then(Value::as_str) {
        out.push_str(&format!("{indent}  -> {href}\n"));
    }
    if max_depth.is_some_and(|limit| level >= limit) {
        return;
    }
    if let Some(arr) = topic
        .get("children")
        .and_then(|c| c.get("attached"))
        .and_then(Value::as_array)
    {
        for child in arr {
            render_topic(child, level + 1, max_depth, out, None);
        }
    }
}

fn render_structure(sheet: &Value, root: &Value, out: &mut String) {
    let mut titles: HashMap<String, String> = HashMap::new();
    let mut path = Vec::new();
    model::walk_paths(root, &mut path, &mut |topic, _| {
        titles.insert(
            model::id(topic).to_string(),
            model::title(topic).to_string(),
        );
    });
    let mut lines: Vec<String> = Vec::new();
    collect_structure(root, &mut Vec::new(), &titles, &mut lines);
    if let Some(rels) = sheet.get("relationships").and_then(Value::as_array) {
        for rel in rels {
            let from = rel
                .get("end1Id")
                .and_then(Value::as_str)
                .and_then(|id| titles.get(id))
                .cloned()
                .unwrap_or_default();
            let to = rel
                .get("end2Id")
                .and_then(Value::as_str)
                .and_then(|id| titles.get(id))
                .cloned()
                .unwrap_or_default();
            let label = rel.get("title").and_then(Value::as_str).unwrap_or_default();
            let label = if label.is_empty() {
                String::new()
            } else {
                format!(" \"{label}\"")
            };
            lines.push(format!("relationship: \"{from}\" -> \"{to}\"{label}"));
        }
    }
    if lines.is_empty() {
        out.push_str("（无 boundary / summary / relationship）\n");
    } else {
        for line in lines {
            out.push_str(&format!("- {line}\n"));
        }
    }
}

fn collect_structure(
    topic: &Value,
    path: &mut Vec<String>,
    titles: &HashMap<String, String>,
    out: &mut Vec<String>,
) {
    let shown = if path.is_empty() {
        "[]".to_string()
    } else {
        path.join(" / ")
    };
    if let Some(boundaries) = topic.get("boundaries").and_then(Value::as_array) {
        for boundary in boundaries {
            let range = boundary
                .get("range")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let title = boundary.get("title").and_then(Value::as_str).unwrap_or("");
            let title = if title.is_empty() {
                String::new()
            } else {
                format!(" \"{title}\"")
            };
            out.push(format!("boundary: {shown} @ {range}{title}"));
        }
    }
    if let Some(summaries) = topic.get("summaries").and_then(Value::as_array) {
        for summary in summaries {
            let range = summary
                .get("range")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let text = summary
                .get("topicId")
                .and_then(Value::as_str)
                .and_then(|id| titles.get(id))
                .cloned()
                .unwrap_or_default();
            let text = if text.is_empty() {
                String::new()
            } else {
                format!(" \"{text}\"")
            };
            out.push(format!("summary: {shown} @ {range}{text}"));
        }
    }
    if let Some(children) = topic.get("children").and_then(Value::as_object) {
        for kind in model::CHILD_KINDS {
            if let Some(arr) = children.get(kind).and_then(Value::as_array) {
                for child in arr {
                    path.push(model::title(child).to_string());
                    collect_structure(child, path, titles, out);
                    path.pop();
                }
            }
        }
    }
}

pub fn describe_sheet(sheet: &Value) -> Value {
    let root = sheet.get("rootTopic");
    let class = root
        .and_then(|r| r.get("structureClass"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut features = Features::default();
    let mut max_depth = 0;
    let topics = root.map(|r| {
        walk_features(r, 0, &mut features, &mut max_depth);
        model::count_topics(r)
    });
    json!({
        "id": xmind::sheet_id(sheet),
        "title": xmind::sheet_title(sheet),
        "root": root.map(model::title).unwrap_or_default(),
        "topics": topics.unwrap_or(0),
        "depth": max_depth,
        "skeleton": if class.is_empty() { Value::Null } else { Value::String(model::alias_for(class).unwrap_or(class).to_string()) },
        "structureClass": if class.is_empty() { Value::Null } else { Value::String(class.to_string()) },
        "features": {
            "notes": features.notes,
            "links": features.links,
            "images": features.images,
            "labels": features.labels,
            "markers": features.markers,
            "boundaries": features.boundaries,
            "summaries": features.summaries,
            "relationships": sheet.get("relationships").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
            "floating": root.and_then(|r| r.get("children")).and_then(|c| c.get("detached")).and_then(Value::as_array).map(Vec::len).unwrap_or(0),
        }
    })
}

#[derive(Default)]
struct Features {
    notes: usize,
    links: usize,
    images: usize,
    labels: usize,
    markers: usize,
    boundaries: usize,
    summaries: usize,
}

fn walk_features(topic: &Value, depth: usize, features: &mut Features, max_depth: &mut usize) {
    *max_depth = (*max_depth).max(depth);
    if topic.get("notes").is_some_and(|n| !n.is_null()) {
        features.notes += 1;
    }
    if topic.get("href").is_some_and(Value::is_string) {
        features.links += 1;
    }
    if topic.get("image").is_some() {
        features.images += 1;
    }
    if topic
        .get("labels")
        .and_then(Value::as_array)
        .is_some_and(|arr| !arr.is_empty())
    {
        features.labels += 1;
    }
    if topic
        .get("markers")
        .and_then(Value::as_array)
        .is_some_and(|arr| !arr.is_empty())
    {
        features.markers += 1;
    }
    features.boundaries += topic
        .get("boundaries")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    features.summaries += topic
        .get("summaries")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if let Some(children) = topic.get("children").and_then(Value::as_object) {
        for kind in model::CHILD_KINDS {
            if let Some(arr) = children.get(kind).and_then(Value::as_array) {
                for child in arr {
                    walk_features(child, depth + 1, features, max_depth);
                }
            }
        }
    }
}

pub fn sheets_info(sheets: &[Value]) -> Value {
    Value::Array(
        sheets
            .iter()
            .map(|sheet| {
                json!({
                    "id": xmind::sheet_id(sheet),
                    "title": xmind::sheet_title(sheet),
                    "root": sheet.get("rootTopic").map(model::title).unwrap_or_default(),
                    "topics": xmind::sheet_topic_count(sheet),
                })
            })
            .collect(),
    )
}
