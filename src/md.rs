use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde_json::{Value, json};

use crate::model;
use crate::util::{self, AppError, Result};

#[derive(Debug)]
pub struct Parsed {
    pub root: Value,
    pub resources: Vec<(String, Vec<u8>)>,
}

#[derive(Default)]
struct Inline {
    plain: String,
    html: String,
    links: Vec<(String, String)>,
    images: Vec<(String, String)>,
    other: bool,
    in_link: bool,
    link_start: usize,
    link_url: String,
    in_image: bool,
    image_alt: String,
}

struct Finished {
    plain: String,
    html: String,
    images: Vec<(String, String)>,
    solo_link: Option<(String, String)>,
    solo_image: Option<(String, String)>,
}

impl Inline {
    fn text(&mut self, s: &str) {
        if self.in_image {
            self.image_alt.push_str(s);
            return;
        }
        self.plain.push_str(s);
        self.html.push_str(&escape(s));
        if !self.in_link && !s.trim().is_empty() {
            self.other = true;
        }
    }

    fn code(&mut self, s: &str) {
        if self.in_image {
            self.image_alt.push_str(s);
            return;
        }
        self.plain.push_str(s);
        self.html.push_str("<code>");
        self.html.push_str(&escape(s));
        self.html.push_str("</code>");
        self.other = true;
    }

    fn break_(&mut self, multiline: bool) {
        if multiline {
            self.plain.push('\n');
            self.html.push_str("<br>");
        } else {
            self.plain.push(' ');
            self.html.push(' ');
        }
    }

    fn start_link(&mut self, url: &str) {
        self.in_link = true;
        self.link_start = self.plain.len();
        self.link_url = url.to_string();
        self.html
            .push_str(&format!("<a href=\"{}\">", escape_attr(url)));
    }

    fn end_link(&mut self) {
        self.in_link = false;
        self.html.push_str("</a>");
        let text = self
            .plain
            .get(self.link_start..)
            .unwrap_or_default()
            .to_string();
        self.links.push((text, self.link_url.clone()));
    }

    fn start_image(&mut self, src: &str) {
        self.in_image = true;
        self.image_alt.clear();
        self.images.push((src.to_string(), String::new()));
    }

    fn end_image(&mut self) {
        self.in_image = false;
        if let Some(last) = self.images.last_mut() {
            last.1 = std::mem::take(&mut self.image_alt);
        }
    }

    fn start_emphasis(&mut self, tag: &str) {
        if self.in_image {
            return;
        }
        if !self.in_link {
            self.other = true;
        }
        self.html.push_str(tag);
    }

    fn end_emphasis(&mut self, tag: &str) {
        if self.in_image {
            return;
        }
        self.html.push_str(tag);
    }

    fn is_empty(&self) -> bool {
        self.plain.trim().is_empty() && self.html.trim().is_empty() && self.images.is_empty()
    }

    fn finish(self) -> Finished {
        let trimmed = self.plain.trim();
        let solo_link = if self.links.len() == 1
            && self.images.is_empty()
            && !self.other
            && !trimmed.is_empty()
            && trimmed == self.links[0].0.trim()
        {
            Some(self.links[0].clone())
        } else {
            None
        };
        let solo_image =
            if self.images.len() == 1 && self.links.is_empty() && !self.other && trimmed.is_empty()
            {
                Some(self.images[0].clone())
            } else {
                None
            };
        Finished {
            plain: self.plain,
            html: self.html,
            images: self.images,
            solo_link,
            solo_image,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Rank(u8, u16);

type NodePath = Vec<(&'static str, usize)>;

struct StackEntry {
    rank: Rank,
    path: NodePath,
}

struct ItemState {
    stack_index: usize,
    title_set: bool,
}

#[derive(Default)]
struct Quote {
    plain: Vec<String>,
    html: Vec<String>,
}

struct Builder<'a> {
    base: &'a Path,
    title_override: Option<String>,
    root: Option<Value>,
    last_path: Option<NodePath>,
    stack: Vec<StackEntry>,
    list_depth: u16,
    items: Vec<ItemState>,
    quote: Option<Quote>,
    para: Option<Inline>,
    para_line: usize,
    resources: Vec<(String, Vec<u8>)>,
}

pub fn parse(text: &str, base: &Path, title_override: Option<&str>) -> Result<Parsed> {
    let mut b = Builder {
        base,
        title_override: title_override.map(str::to_string),
        root: None,
        last_path: None,
        stack: Vec::new(),
        list_depth: 0,
        items: Vec::new(),
        quote: None,
        para: None,
        para_line: 1,
        resources: Vec::new(),
    };
    let options = Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        let line = line_of(text, range.start);
        match event {
            Event::Start(tag) => b.start(tag, line)?,
            Event::End(tag) => b.end(tag, line)?,
            Event::Text(s) => b.inline()?.text(&s),
            Event::Code(s) => b.inline()?.code(&s),
            Event::SoftBreak | Event::HardBreak => {
                let multiline = b.quote.is_some();
                b.inline()?.break_(multiline);
            }
            Event::InlineHtml(s) => b.inline()?.text(&s),
            Event::Rule => return Err(err(line, "不支持水平线")),
            Event::Html(_) => return Err(err(line, "不支持 HTML 块")),
            Event::FootnoteReference(_) => return Err(err(line, "不支持脚注")),
            Event::InlineMath(_) | Event::DisplayMath(_) => {
                return Err(err(line, "不支持数学公式"));
            }
            Event::TaskListMarker(done) => b.task_marker(done)?,
        }
    }
    b.finish()
}

impl Builder<'_> {
    fn inline(&mut self) -> Result<&mut Inline> {
        self.para
            .as_mut()
            .ok_or_else(|| AppError::usage("Markdown 解析状态异常"))
    }

    fn line_err(&self, line: usize, message: &str) -> AppError {
        err(line, message)
    }

    fn ensure_root(&mut self, line: usize) -> Result<()> {
        if self.root.is_none() {
            let Some(title) = self.title_override.clone() else {
                return Err(self.line_err(line, "缺少 H1 根主题（或用 --title 指定）"));
            };
            self.root = Some(model::new_topic(&title));
            self.last_path = Some(Vec::new());
        }
        Ok(())
    }

    fn current_path(&self) -> NodePath {
        self.stack
            .last()
            .map(|entry| entry.path.clone())
            .unwrap_or_default()
    }

    fn node_mut(&mut self, path: &NodePath, line: usize) -> Result<&mut Value> {
        let mut node = self
            .root
            .as_mut()
            .ok_or_else(|| err(line, "缺少 H1 根主题"))?;
        for (kind, index) in path {
            let children = node
                .get_mut("children")
                .and_then(Value::as_object_mut)
                .ok_or_else(|| err(line, "Markdown 结构异常"))?;
            let arr = children
                .get_mut(*kind)
                .and_then(Value::as_array_mut)
                .ok_or_else(|| err(line, "Markdown 结构异常"))?;
            node = arr
                .get_mut(*index)
                .ok_or_else(|| err(line, "Markdown 结构异常"))?;
        }
        Ok(node)
    }

    fn last_node_mut(&mut self, line: usize) -> Result<&mut Value> {
        let path = self
            .last_path
            .clone()
            .ok_or_else(|| self.line_err(line, "备注/图片前缺少主题"))?;
        self.node_mut(&path, line)
    }

    fn pop_for(&mut self, rank: Rank) {
        while self.stack.last().is_some_and(|e| e.rank >= rank) {
            self.stack.pop();
        }
    }

    fn add_topic(&mut self, rank: Rank, topic: Value, line: usize) -> Result<()> {
        self.ensure_root(line)?;
        let mut path = self.current_path();
        let index = {
            let parent = self.node_mut(&path, line)?;
            let arr = model::attached_mut(parent);
            let index = arr.len();
            arr.push(topic);
            index
        };
        path.push(("attached", index));
        self.stack.push(StackEntry {
            rank,
            path: path.clone(),
        });
        self.last_path = Some(path);
        Ok(())
    }

    fn start(&mut self, tag: Tag, line: usize) -> Result<()> {
        match tag {
            Tag::Heading { level, .. } => {
                if self.quote.is_some() {
                    return Err(self.line_err(line, "备注中不支持标题"));
                }
                self.flush_pending(line)?;
                self.start_heading(level, line)
            }
            Tag::Paragraph => {
                self.flush_pending(line)?;
                self.para = Some(Inline::default());
                self.para_line = line;
                Ok(())
            }
            Tag::BlockQuote(_) => {
                self.flush_pending(line)?;
                if self.quote.is_some() {
                    return Err(self.line_err(line, "不支持嵌套备注"));
                }
                self.quote = Some(Quote::default());
                Ok(())
            }
            Tag::List(_) => {
                self.flush_pending(line)?;
                if self.quote.is_some() {
                    return Err(self.line_err(line, "备注中不支持列表"));
                }
                self.list_depth = self.list_depth.saturating_add(1);
                Ok(())
            }
            Tag::Item => {
                if self.quote.is_some() {
                    return Err(self.line_err(line, "备注中不支持列表"));
                }
                self.start_item(line)
            }
            Tag::Link { dest_url, .. } => {
                let url = util::link_target(&dest_url, self.base);
                self.inline()?.start_link(&url);
                Ok(())
            }
            Tag::Image { dest_url, .. } => {
                self.inline()?.start_image(&dest_url);
                Ok(())
            }
            Tag::Emphasis => {
                self.inline()?.start_emphasis("<em>");
                Ok(())
            }
            Tag::Strong => {
                self.inline()?.start_emphasis("<strong>");
                Ok(())
            }
            Tag::CodeBlock(_) => Err(self.line_err(line, "不支持代码块")),
            Tag::HtmlBlock => Err(self.line_err(line, "不支持 HTML 块")),
            Tag::Table(_) => Err(self.line_err(line, "不支持表格")),
            Tag::FootnoteDefinition(_) => Err(self.line_err(line, "不支持脚注")),
            Tag::MetadataBlock(_) => Err(self.line_err(line, "不支持 front-matter")),
            Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript => Err(self.line_err(line, "不支持的 Markdown 语法")),
        }
    }

    fn start_heading(&mut self, level: HeadingLevel, line: usize) -> Result<()> {
        if level == HeadingLevel::H1 {
            if self.root.is_some() {
                return Err(self.line_err(line, "只能有一个 H1"));
            }
            self.root = Some(model::new_topic(""));
            self.last_path = Some(Vec::new());
        } else {
            self.ensure_root(line)?;
            let rank = Rank(0, level as u16);
            self.pop_for(rank);
            self.add_topic(rank, model::new_topic(""), line)?;
        }
        self.para = Some(Inline::default());
        self.para_line = line;
        Ok(())
    }

    fn end_heading(&mut self, level: HeadingLevel, line: usize) -> Result<()> {
        let finished = self.take_inline(line)?;
        if !finished.images.is_empty() {
            return Err(self.line_err(line, "标题中不支持图片"));
        }
        let title_text = if level == HeadingLevel::H1 {
            self.title_override
                .clone()
                .unwrap_or_else(|| finished.plain.trim().to_string())
        } else {
            finished
                .solo_link
                .as_ref()
                .map(|(t, _)| t.trim().to_string())
                .unwrap_or_else(|| finished.plain.trim().to_string())
        };
        let href = finished.solo_link.map(|(_, url)| url);
        if level == HeadingLevel::H1 {
            let root = self.root.as_mut().expect("root");
            root["title"] = Value::String(title_text);
            if let Some(href) = href {
                root["href"] = Value::String(href);
            }
        } else {
            let path = self.current_path();
            let topic = self.node_mut(&path, line)?;
            topic["title"] = Value::String(title_text);
            if let Some(href) = href {
                topic["href"] = Value::String(href);
            }
        }
        Ok(())
    }

    fn start_item(&mut self, line: usize) -> Result<()> {
        self.flush_pending(line)?;
        self.ensure_root(line)?;
        let depth = self.list_depth.max(1);
        let rank = Rank(1, depth);
        self.pop_for(rank);
        self.add_topic(rank, model::new_topic(""), line)?;
        let stack_index = self.stack.len() - 1;
        self.items.push(ItemState {
            stack_index,
            title_set: false,
        });
        self.para = Some(Inline::default());
        self.para_line = line;
        Ok(())
    }

    fn end_item(&mut self, line: usize) -> Result<()> {
        self.flush_pending(line)?;
        if let Some(state) = self.items.pop() {
            self.stack.truncate(state.stack_index);
        }
        Ok(())
    }

    fn flush_pending(&mut self, line: usize) -> Result<()> {
        let has_content = self.para.as_ref().is_some_and(|inline| !inline.is_empty());
        if has_content {
            self.end_paragraph(line)
        } else {
            self.para = None;
            Ok(())
        }
    }

    fn task_marker(&mut self, done: bool) -> Result<()> {
        let label = if done { "done" } else { "todo" };
        let path = self.current_path();
        let topic = self.node_mut(&path, 1)?;
        let labels = topic
            .as_object_mut()
            .expect("topic")
            .entry("labels")
            .or_insert_with(|| json!([]));
        let arr = labels.as_array_mut().expect("labels array");
        if !arr.iter().any(|v| v.as_str() == Some(label)) {
            arr.push(Value::String(label.to_string()));
        }
        Ok(())
    }

    fn take_inline(&mut self, line: usize) -> Result<Finished> {
        self.para
            .take()
            .map(Inline::finish)
            .ok_or_else(|| self.line_err(line, "Markdown 解析状态异常"))
    }

    fn end_paragraph(&mut self, line: usize) -> Result<()> {
        let finished = self.take_inline(line)?;
        if !finished.images.is_empty() && finished.solo_image.is_none() {
            return Err(self.line_err(
                line,
                "图片必须单独成行，且不能出现在标题、备注或列表项文本中",
            ));
        }
        let plain = finished.plain.trim().to_string();
        let html = finished.html.trim().to_string();

        if let Some(quote) = self.quote.as_mut() {
            if !finished.images.is_empty() {
                return Err(err(line, "备注中不支持图片"));
            }
            if !plain.is_empty() || !html.is_empty() {
                quote.plain.push(plain);
                quote.html.push(format!("<p>{html}</p>"));
            }
            return Ok(());
        }

        if !self.items.is_empty() {
            if self.items.last().expect("item").title_set {
                return Err(self.line_err(line, "列表项只支持一段文本"));
            }
            let image = match &finished.solo_image {
                Some((src, _alt)) => Some(self.image_value(src, line)?),
                None => None,
            };
            let path = self.current_path();
            let topic = self.node_mut(&path, line)?;
            if let Some(image) = image {
                topic["image"] = image;
            } else if let Some((text, url)) = finished.solo_link {
                topic["title"] = Value::String(text.trim().to_string());
                topic["href"] = Value::String(url);
            } else {
                topic["title"] = Value::String(plain);
            }
            self.items.last_mut().expect("item").title_set = true;
            return Ok(());
        }

        if let Some((src, _alt)) = finished.solo_image {
            self.ensure_root(line)?;
            let image = self.image_value(&src, line)?;
            let topic = self.last_node_mut(line)?;
            topic["image"] = image;
            return Ok(());
        }
        if let Some((text, url)) = finished.solo_link {
            let mut topic = model::new_topic(text.trim());
            topic["href"] = Value::String(url);
            self.pop_for(Rank(1, 1));
            self.add_topic(Rank(1, 1), topic, line)?;
            return Ok(());
        }
        Err(self.line_err(line, "不支持的普通段落；列表项、引用、链接或图片请单独成行"))
    }

    fn end_quote(&mut self, line: usize) -> Result<()> {
        let quote = self
            .quote
            .take()
            .ok_or_else(|| self.line_err(line, "Markdown 解析状态异常"))?;
        if quote.plain.is_empty() {
            return Ok(());
        }
        let plain = quote
            .plain
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let html = quote
            .html
            .iter()
            .filter(|s| s != &"<p></p>")
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        self.ensure_root(line)?;
        let topic = self.last_node_mut(line)?;
        set_notes(topic, &plain, &html);
        Ok(())
    }

    fn image_value(&mut self, src: &str, line: usize) -> Result<Value> {
        if src.contains("://")
            || src.starts_with("xap:")
            || src.starts_with("data:")
            || src.starts_with("//")
        {
            return Err(self.line_err(line, "图片只接受本地文件路径"));
        }
        let decoded = util::percent_decode(src);
        let path = Path::new(&decoded);
        let path: PathBuf = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base.join(path)
        };
        let image = util::load_image(&path, None).map_err(|e| self.line_err(line, &e.message))?;
        if !self.resources.iter().any(|(name, _)| name == &image.name) {
            self.resources.push((image.name.clone(), image.bytes));
        }
        Ok(json!({
            "src": format!("xap:resources/{}", image.name),
            "width": image.width,
        }))
    }

    fn end(&mut self, tag: TagEnd, line: usize) -> Result<()> {
        match tag {
            TagEnd::Paragraph => self.end_paragraph(line),
            TagEnd::Heading(level) => self.end_heading(level, line),
            TagEnd::BlockQuote(_) => self.end_quote(line),
            TagEnd::List(_) => {
                self.flush_pending(line)?;
                self.list_depth = self.list_depth.saturating_sub(1);
                Ok(())
            }
            TagEnd::Item => self.end_item(line),
            TagEnd::Link => {
                self.inline()?.end_link();
                Ok(())
            }
            TagEnd::Image => {
                self.inline()?.end_image();
                Ok(())
            }
            TagEnd::Emphasis => {
                self.inline()?.end_emphasis("</em>");
                Ok(())
            }
            TagEnd::Strong => {
                self.inline()?.end_emphasis("</strong>");
                Ok(())
            }
            TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::MetadataBlock(_) => Err(self.line_err(line, "不支持的 Markdown 语法")),
        }
    }

    fn finish(mut self) -> Result<Parsed> {
        if self.root.is_none() {
            match self.title_override.take() {
                Some(title) => {
                    self.root = Some(model::new_topic(&title));
                    self.last_path = Some(Vec::new());
                }
                None => {
                    return Err(AppError::usage(
                        "Markdown 必须恰好包含一个 H1 根主题（或用 --title 指定）",
                    ));
                }
            }
        }
        Ok(Parsed {
            root: self.root.expect("root"),
            resources: self.resources,
        })
    }
}

pub fn notes_value(text: &str, base: &Path) -> Value {
    let mut paragraphs: Vec<Finished> = Vec::new();
    let mut inline = Inline::default();
    let mut in_para = false;
    for event in Parser::new_ext(text, Options::empty()) {
        match event {
            Event::Start(Tag::Paragraph) => {
                inline = Inline::default();
                in_para = true;
            }
            Event::End(TagEnd::Paragraph) => {
                paragraphs.push(inline.finish());
                inline = Inline::default();
                in_para = false;
            }
            Event::Text(s) => inline.text(&s),
            Event::Code(s) => inline.code(&s),
            Event::SoftBreak | Event::HardBreak => inline.break_(true),
            Event::InlineHtml(s) => inline.text(&s),
            Event::Start(Tag::Link { dest_url, .. }) => {
                inline.start_link(&util::link_target(&dest_url, base));
            }
            Event::End(TagEnd::Link) => inline.end_link(),
            Event::Start(Tag::Emphasis) => inline.start_emphasis("<em>"),
            Event::End(TagEnd::Emphasis) => inline.end_emphasis("</em>"),
            Event::Start(Tag::Strong) => inline.start_emphasis("<strong>"),
            Event::End(TagEnd::Strong) => inline.end_emphasis("</strong>"),
            Event::Start(Tag::Image { dest_url, .. }) => inline.start_image(&dest_url),
            Event::End(TagEnd::Image) => inline.end_image(),
            _ => {}
        }
    }
    if in_para || !inline.plain.trim().is_empty() {
        paragraphs.push(inline.finish());
    }
    let plain = paragraphs
        .iter()
        .map(|p| p.plain.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let html = paragraphs
        .iter()
        .filter(|p| !p.html.trim().is_empty())
        .map(|p| format!("<p>{}</p>", p.html.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    json!({
        "plain": { "content": plain },
        "realHTML": { "content": html },
    })
}

fn set_notes(topic: &mut Value, plain: &str, html: &str) {
    let existing_plain = topic
        .get("notes")
        .and_then(|n| n.get("plain"))
        .and_then(|p| p.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let existing_html = topic
        .get("notes")
        .and_then(|n| n.get("realHTML"))
        .and_then(|p| p.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let plain = if existing_plain.is_empty() {
        plain.to_string()
    } else {
        format!("{existing_plain}\n{plain}")
    };
    let html = if existing_html.is_empty() {
        html.to_string()
    } else {
        format!("{existing_html}\n{html}")
    };
    topic["notes"] = json!({
        "plain": { "content": plain },
        "realHTML": { "content": html },
    });
}

fn err(line: usize, message: &str) -> AppError {
    AppError::usage(format!("第 {line} 行: {message}"))
}

fn line_of(text: &str, offset: usize) -> usize {
    let end = offset.min(text.len());
    text.as_bytes()[..end]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
        + 1
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(s: &str) -> String {
    escape(s).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        std::env::temp_dir()
    }

    fn children(topic: &Value) -> Vec<Value> {
        topic
            .get("children")
            .and_then(|c| c.get("attached"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    fn titles(topic: &Value) -> Vec<String> {
        children(topic)
            .iter()
            .map(|t| model::title(t).to_string())
            .collect()
    }

    #[test]
    fn heading_and_list_hierarchy() {
        let parsed = parse(
            "# R
## A
- x
  - y
### B
1. z
",
            &base(),
            None,
        )
        .unwrap();
        let root = &parsed.root;
        assert_eq!(model::title(root), "R");
        assert_eq!(titles(root), ["A"]);
        let a = &children(root)[0];
        assert_eq!(titles(a), ["x", "B"]);
        let x = &children(a)[0];
        assert_eq!(titles(x), ["y"]);
        let b = &children(a)[1];
        assert_eq!(titles(b), ["z"]);
    }

    #[test]
    fn checkbox_and_notes() {
        let parsed = parse(
            "# R
- [ ] todo
- item
  > note **bold**
",
            &base(),
            None,
        )
        .unwrap();
        let items = children(&parsed.root);
        assert_eq!(model::title(&items[0]), "todo");
        assert_eq!(items[0]["labels"], json!(["todo"]));
        assert_eq!(model::title(&items[1]), "item");
        let plain = items[1]["notes"]["plain"]["content"].as_str().unwrap();
        let html = items[1]["notes"]["realHTML"]["content"].as_str().unwrap();
        assert!(plain.contains("note bold"));
        assert!(html.contains("<strong>bold</strong>"));
    }

    #[test]
    fn standalone_link_creates_topic() {
        let parsed = parse(
            "# R

[官网](https://xmind.app)
",
            &base(),
            None,
        )
        .unwrap();
        let links = children(&parsed.root);
        assert_eq!(links.len(), 1);
        assert_eq!(model::title(&links[0]), "官网");
        assert_eq!(links[0]["href"], "https://xmind.app");
    }

    #[test]
    fn title_override_can_replace_or_supply_root() {
        let parsed = parse(
            "## A
",
            &base(),
            Some("Root"),
        )
        .unwrap();
        assert_eq!(model::title(&parsed.root), "Root");
        let parsed = parse(
            "# Old
",
            &base(),
            Some("New"),
        )
        .unwrap();
        assert_eq!(model::title(&parsed.root), "New");
    }

    #[test]
    fn rejects_missing_h1_and_plain_paragraphs() {
        assert!(
            parse(
                "## A
",
                &base(),
                None
            )
            .is_err()
        );
        let err = parse(
            "# R

plain text
",
            &base(),
            None,
        )
        .unwrap_err();
        assert!(err.message.contains("第 3 行"), "{}", err.message);
    }

    #[test]
    fn rejects_unsupported_blocks_with_line() {
        let err = parse(
            "# R

```
x
```
",
            &base(),
            None,
        )
        .unwrap_err();
        assert!(err.message.contains("第 3 行"), "{}", err.message);
        let err = parse(
            "# R

| a | b |
|---|---|
| 1 | 2 |
",
            &base(),
            None,
        )
        .unwrap_err();
        assert!(err.message.contains("表格"), "{}", err.message);
    }
}
