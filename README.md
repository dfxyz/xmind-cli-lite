# xmind-cli-lite

面向 AI Agent 的离线 Xmind 命令行工具。它把「Markdown → `.xmind` → JSON → batch 编辑 → validate」做成一个可预测的闭环，适合脚本和 Agent 调用。

- **格式**：Xmind 2020+ / 26 的 Zen 格式（zip 内 `content.json`），不支持 Xmind 8 旧格式
- **离线**：零网络、无登录、无云端
- **输入**：新建只接受 Markdown + CLI flags；编辑只接受 JSON ops
- **输出**：`.xmind` 文件；查看命令输出 JSON 或 outline，validate 输出文本或 JSON
- **不改 zip**：不要手工解压、改 JSON、重新压缩；所有写入都走本工具

设计细节见 [`SPEC.md`](SPEC.md)。Agent 的完整操作手册在 [`skills/xmind-cli-lite/SKILL.md`](skills/xmind-cli-lite/SKILL.md)。

## 什么时候用它

适合：

- 把大纲、会议纪要、方案、知识地图整理成 Xmind
- 读取已有 `.xmind` 的结构，定位节点后批量改标题、备注、链接、图片、标签、标记
- 添加 boundary / summary / relationship / 浮动主题
- 在交付前做结构、资源和引用校验

不适合：

- Xmind 8 旧格式、云端空间、登录、协作
- 主题切换、逐节点颜色/字体、callout、编号、背景
- 新建多 sheet、导入/删除 sheet
- URL 图片、AI 生图、Markdown 回灌
- 自动布线 / route / anchor 引擎

## 安装

```bash
cargo build --release
./target/release/xmind-cli-lite --help

# 安装到 PATH
cargo install --path .
xmind-cli-lite --version
```

Windows 下二进制为 `xmind-cli-lite.exe`。命令名是 `xmind-cli-lite`，不要调用官方 `xmind` 命令。

## Agent 最短工作流

### 1. 新建

写 Markdown，全文恰好一个 H1；图片只用本地路径。

```markdown
# 项目计划

## 目标与验收

- 上线 MVP
- 通过验收

## 风险

> 数据迁移可能延期。

## 参考

[需求文档](https://example.com/spec)
```

```bash
xmind-cli-lite create plan.md -o plan.xmind
```

- 无 H1 时用 `--title "根主题"`；有 H1 时 `--title` 覆盖它
- 管道输入：`cat plan.md | xmind-cli-lite create --stdin -o plan.xmind --title "根主题"`
- 默认骨架 `mindmap`；可选 `logic-right`、`org-down`、`tree-right`、`timeline`、`matrix`、`grid`、`brace`、`fishbone`、`treetable` 等，完整表见 [`references/skeletons.md`](skills/xmind-cli-lite/references/skeletons.md)
- 默认输出 `Sheet 1`，可用 `--sheet "标题"` 改名
- 输出已存在时拒绝覆盖；需要时加 `--force`

### 2. 查看

```bash
xmind-cli-lite describe plan.xmind
xmind-cli-lite sheets plan.xmind
xmind-cli-lite read plan.xmind --format outline --depth 3
xmind-cli-lite read plan.xmind --format json > plan.json
```

- `describe`：每张 sheet 的 id、标题、根标题、节点数、深度、骨架和特性计数
- `sheets`：sheet 列表
- `outline`：只读预览，**有损，不能回灌**
- `read --format json`：与 `content.json` 同构的数组；attached/detached topic 额外带 `path`
- 多 sheet 时先 `sheets`，再用 `--sheet <id|title>` 限定

`path` 规则：

- 从根节点的子级开始，包含目标自身；根是 `[]`
- 只覆盖 `children.attached` 和 `children.detached`
- summary / callout 子树里的 topic 没有 `path`，用 `id` 定位
- 移动后旧 `path` 失效，用 `id` 或重新读；删除后 `id` 失效

### 3. 编辑

先读 JSON，规划 ops，再一次性 batch。不要直接改 `content.json`。

```json
[
  {"op": "add", "parent": {"path": ["风险"]}, "title": "数据迁移"},
  {"op": "boundary", "parent": {"path": ["目标与验收"]}, "range": [0, 1], "title": "MVP 范围"},
  {"op": "summary", "parent": {"path": ["风险"]}, "range": [0, 0], "text": "需提前验证"},
  {"op": "relationship", "from": {"path": ["风险", "数据迁移"]}, "to": {"path": ["目标与验收"]}, "label": "影响"}
]
```

```bash
xmind-cli-lite batch plan.xmind --input ops.json --dry-run
xmind-cli-lite batch plan.xmind --input ops.json --backup
xmind-cli-lite validate plan.xmind --json
```

- `--dry-run`：规划并校验，不落盘
- `--backup`：先复制 `<file>.bak`
- `--out copy.xmind`：写到副本；目标存在时需 `--force`
- `--sheet`：只改指定 sheet，默认第一张；其余 sheet 和未知字段原样保留
- 省略 `--input` 或传 `-`：从 stdin 读 ops
- `--no-validate`：跳过写入前自动校验（不推荐，仅用于已确认合法的特殊文件）
- 任何 op、图片或校验失败：整批不落盘
- 成功输出包含 `ok`、`file`、`sheet`、`topics`、`ops`、`changes`、`warnings`

### 4. 交付

```bash
xmind-cli-lite validate plan.xmind --strict --json
```

- 结构/主题/关系变化后，让用户在 Xmind Desktop 打开确认；`validate` 不验证渲染效果
- 最终回复给绝对路径、validate 结果、1–2 个关键设计选择（骨架、层级、boundary 等）
- 不要贴整份 JSON

## batch ops

定位三选一：

```json
{"id": "<uuid>"}
{"topic": "唯一标题"}
{"path": ["父主题", "目标主题"]}
```

`topic` 必须在整个 sheet 内唯一；重名会报错并列出候选 path。优先用 `id`。

| op | 关键字段 | 说明 |
|---|---|---|
| `add` | `parent`, `title`, `index?`, `notes?`, `labels?`, `markers?`, `href?` | 默认追加；`index` 插入 |
| `update` | `topic`, `title` | 只改标题 |
| `delete` | `topic` | 删整棵子树；清理指向它的 relationship 和失效 summary |
| `move` | `topics`, `parent`, `index?` | 支持多目标；不能移根或移入自身子树 |
| `notes` | `topic`, `notes` | 字符串或 `null`；Markdown 会转 plain + realHTML |
| `href` | `topic`, `href` 或 `file` | `href: null` 清除；`file` 按当前目录解析为 `file:///` |
| `label` | `topic`, `add?`, `remove?` | 字符串标签；清空后移除字段 |
| `marker` | `topic`, `add?`, `remove?` | markerId，如 `people-blue`、`flag-red` |
| `image` | `topic`, `input`, `width?`, `remove?` | 本地 PNG/JPEG/GIF/WebP；默认宽 320，不放大 |
| `layout` | `topic`, `layout` | 只改 `structureClass`；根/一级覆盖可能被 Desktop 忽略 |
| `boundary` | `parent`, `range` 或 `topics`, `title?` | `range` 是 attached 子级的 0 起始闭区间 |
| `summary` | `parent`, `range` 或 `topics`, `text` | 创建 summary topic 和 `topicId` 引用 |
| `relationship` | `from`, `to`, `label?` | 端点可在任意子树；不写控制点 |
| `floating` | `title`, `position`, `style?` | 根级 detached 主题，加入 `arrangeableLayerOrder` |
| `position` | `topic`, `x`, `y` | 只对浮动主题有效 |

补充规则：

- `boundary` / `summary` 的 `range: [start, end]` 是 `children.attached` 的闭区间，从 0 开始；必须是同一父节点下的连续兄弟
- 删除/移出主题时会调整原父节点上的 boundary/summary range；range 空了会清理对应结构
- `relationship` op 的 `from` / `to` 只能定位当前 sheet 的 topic（含 summary 子树）；校验时允许旧文件的端点指向 boundary/zone
- `layout` 是局部骨架；要给分支换布局，优先在 `###` 层设置，避免依赖根/一级覆盖
- `floating.style` 是原始 properties 透传，不是通用样式 API

## 图片与链接

图片规则：

- 只接受本地 PNG / JPEG / GIF / WebP；拒绝 SVG、URL、网络路径
- 按 SHA-256 去重，存为 `resources/<sha256>.<ext>`
- 默认宽度 320，不放大；`--width` 可覆盖
- Markdown 图片按 Markdown 文件所在目录解析；stdin 输入按当前工作目录解析

图片命令：

```bash
xmind-cli-lite image plan.xmind --topic "架构图" --input images/arch.png --width 480
xmind-cli-lite image plan.xmind --topic "架构图" --remove
```

链接命令：

```bash
xmind-cli-lite link plan.xmind --topic "需求文档" --url https://example.com/spec
xmind-cli-lite link plan.xmind --topic "需求文档" --file ../docs/spec.md
xmind-cli-lite link plan.xmind --topic "需求文档" --remove
```

链接规则：

- `--url` 与 `--file` 二选一
- `--file` 生成 `file:///` URI，路径做百分号编码
- `--sheet` 可指定 sheet；`--topic` 必须精确且唯一

## Markdown 语法

支持：

- 唯一的 H1 作为根；H2–H6 形成层级
- 嵌套列表形成树；列表项标题就是主题标题
- `>` 引用块作为备注，支持基础 Markdown（段落、强调、链接、换行）
- `- [ ]` / `- [x]` 转成 labels `todo` / `done`
- 整行链接生成带 `href` 的子主题
- 整行图片挂到最近的前置主题

不支持，遇到会报错并给行号：

- 代码块、表格、HTML 块、脚注、数学公式、水平线、front matter（内联 HTML 会按普通文本保留）
- 多 H1、没有 H1 且没给 `--title`

空标题不会中断解析，但 `validate` 会警告；交付前清掉。

完整语法见 [`references/markdown-grammar.md`](skills/xmind-cli-lite/references/markdown-grammar.md)。

## 输出与退出码

退出码：

| 码 | 含义 |
|---|---|
| `0` | 成功 |
| `1` | 文件 / IO 错误 |
| `2` | 用法 / 输入错误 |
| `3` | 校验失败 |

错误输出：

```json
{"ok": false, "error": {"code": "usage|io|invalid_file|validation_failed", "message": "..."}}
```

- 成功和失败的 JSON 都走 stdout；人读进度/错误摘要走 stderr
- `read --format json` 成功时是 **sheet 数组**，不是 `ok` 信封
- `read --format outline` 是文本
- `describe` / `sheets` / `create` / `batch` / `link` / `image` / `validate --json` 是 JSON 信封
- `validate --quiet` 只返回退出码

## 硬规则

- 任何结构修改后必须 `validate`；新文件建议 `--strict`
- 不要为了凑结构造空节点，不要为 boundary/summary 发明包装层
- 图片先下载到本地再传入，不要依赖网络
- 编辑优先级：`id` > `path` > `topic`
- `read --format outline` 只用于上下文预览；编辑必须从 JSON + batch
- `validate` 通过不等于 Desktop 渲染正确；涉及主题、图片、关系线时让用户打开确认
- 多 sheet 时确认改的是 `--sheet` 指定的那一张

## 不要拿官方模板当主题底稿

官方模板可能自带 `relationshipAutoColor: "follow-target-topic"` 和旧的 `relationship.style.line-color`（常见半透明绿）。不做关系线相关编辑时，`batch` 会原样保留这些字段；`relationship` op 会把选中 sheet 的 `relationshipAutoColor` 置 `null`，但不会重写旧关系线的 `style`。因此关系线颜色仍可能由模板决定，而不是本工具主题。

需要统一主题色时用 `create` 新建。本工具生成的新文件会：

- 写 `relationshipAutoColor: null`
- 在主题 `relationship` 里写默认线色 `#087736FF`
- 不写 relationship 对象 `style`
- 用一套内嵌主题，不提供 `theme` / 颜色 / 字体 op

## 主题与骨架

- 只有一套内嵌主题，不提供切换
- `create --skeleton` 和 `layout` 只改结构，不改配色
- 非默认骨架会写 `sheet.extensions` 的 `org.xmind.ui.skeleton.structure.style`
- 根节点和一级子主题的局部布局覆盖可能被 Desktop 忽略；换局部布局时在更深层设置

## 质量门

提交前：

```bash
cargo fmt --check
cargo clippy --all-targets
cargo test
```

Agent 交付前：

1. `validate <file> --strict` 必须为 0
2. 在 Xmind Desktop 打开确认（结构/主题/图片/链接/关系线）
3. 回复中给绝对路径和校验结果

`validate` 还会对空标题、同级重名、深度 > 8、节点 > 1500、未知骨架发 warning；`--strict` 下 warning 也算失败。

已知自动化覆盖：Markdown 解析、15 个 ops、图片/链接、退出码、多 sheet、25 个官方模板的 read/validate 回归。

## 仓库结构

```
src/                 Rust 源码
  main.rs            clap + dispatch + 退出码
  app.rs             命令实现
  md.rs              Markdown -> 主题树
  model.rs           模板、UUID、主题、topic 辅助
  xmind.rs           zip 读写 / manifest / 资源 / 替换
  ops.rs             batch 定位与 15 个 op
  validate.rs        结构 / 资源 / 引用校验
  render.rs          JSON / outline / describe / sheets
  util.rs            file URI、图片、错误
assets/theme.json    内嵌主题
tests/cli.rs         集成测试
skills/xmind-cli-lite/
                     Agent skill、语法/编辑/骨架/质量门文档和 recipes
SPEC.md              设计与验收规格
reference/           官方样本与验收产物（gitignore，不进入发布物）
```

## Agent skill

支持 skills 的 Agent 优先加载 [`skills/xmind-cli-lite/SKILL.md`](skills/xmind-cli-lite/SKILL.md)。把 `skills/xmind-cli-lite/` 复制到所用 Agent 的 skills 目录即可（复制，不用 symlink）。README 是命令层说明，skill 里还有按场景拆分的 recipes：

- [`recipes/quick-map.md`](skills/xmind-cli-lite/recipes/quick-map.md)
- [`recipes/knowledge-map.md`](skills/xmind-cli-lite/recipes/knowledge-map.md)
- [`recipes/code-architecture.md`](skills/xmind-cli-lite/recipes/code-architecture.md)
- [`recipes/project-execution.md`](skills/xmind-cli-lite/recipes/project-execution.md)
- [`recipes/comparison-decision.md`](skills/xmind-cli-lite/recipes/comparison-decision.md)
