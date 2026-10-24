# xmind-cli-lite 规格

状态：v0.1 · 2026-10-24
范围：本文件记录已达成共识的完整规格；主题定稿见 §7.2；实现前未定项见 §12。

## 0. 决策总览

| 维度 | 决策 |
|---|---|
| 产品 | 自用离线 Xmind CLI + 配套 skill，Rust 实现 |
| 二进制 | `xmind-cli-lite` |
| 能力范围 | 新建 + 读取 + 编辑 + 校验，不做官方 `generate` 的 route/anchor 引擎 |
| 文件格式 | Xmind 2020+/26 的 Zen 格式（`content.json` zip） |
| 离线 | 二进制零网络；只接受本地图片 |
| 新建输入 | 仅 Markdown + CLI flags；无 front-matter、无 JSON spec |
| 读取 | JSON 为语义视图；`outline` 为只读预览；写回保留未知字段 |
| 编辑 | `batch` ops，事务式写回 |
| 主题 | 只内嵌一套；不提供 `--color`/`theme` op；定稿见 §7.2 |
| 验收 | 结构校验 + Xmind Desktop 人工验收 |

## 1. 目标

- 让 agent 在无登录、无 credits、无云端的条件下创建和修改本地 `.xmind`。
- agent 负责语义判断（内容、结构、质量），CLI 只负责机械执行（解析、写文件、校验）。
- 新建：`Markdown -> .xmind`。
- 改旧：`read` 得到带 id/path 的 JSON，规划 `batch` ops，写回后 `validate`。
- Xmind Desktop 能正常打开生成/修改后的文件，并能继续编辑、另存。

## 2. 非目标

- Xmind 8 / `content.xml` 旧格式（只读兼容也不做）。
- 登录、auth、billing、Xmind credits、云端协作。
- AI 生图、公网图片下载、Wikipedia 图片、`--url` / `--wiki`。
- 官方 `generate --spec` 的 recipe/route/baseline/anchor 自动编排与降级。
- Markdown 回灌（xmind -> md -> xmind 的无损往返）。
- 多 sheet 新建、sheet 导入/删除、跨 sheet 编辑。
- 逐节点样式（emphasis/字体/颜色；`floating.style` 透传除外）、callout、numbering、background、todo 语义。
- 主题切换。

## 3. 文件格式模型

### 3.1 容器

`.xmind` 是 zip。v1 生成/编辑时：

- 必需：`content.json`、`metadata.json`、`manifest.json`。
- 可选：`resources/<name>`（本地图片）。
- v1 **不生成** `content.xml`，**不生成** `Thumbnails/thumbnail.png`；已验证最小 zip 可由 Desktop 打开，Desktop 另存时会补。
- 编辑已有文件时，未修改的 zip 条目原样保留（包括缩略图、`content.xml`、用户资源）。
- manifest 的 `file-entries` 必须包含 `content.json`、`metadata.json` 以及所有 `resources/*`。

`metadata.json`：

```json
{
  "dataStructureVersion": "3",
  "creator": { "name": "xmind-cli-lite", "version": "<crate version>" },
  "layoutEngineVersion": "5"
}
```

### 3.2 content.json

顶层是 sheet 数组。v1 新建只写 1 个 sheet；读取/编辑支持多 sheet。

sheet 关键字段：

```json
{
  "id": "<uuid>",
  "revisionId": "<uuid>",
  "class": "sheet",
  "title": "Sheet 1",
  "rootTopic": { "...": "topic" },
  "theme": { "...": "单主题，见 §7" },
  "topicOverlapping": "overlap",
  "floatingTopicAutoColor": "follow-color-theme",
  "relationshipAutoColor": null,
  "zones": [],
  "arrangeableLayerOrder": ["<root id>", "<floating topic id>", "..."],
  "relationships": [],
  "extensions": []
}
```

`style` 是 per-sheet 覆盖，可选。新建文件不写；全局画布色由主题的 `map.svg:fill` 控制。编辑旧文件时保留原有 `style`，它可能覆盖全局画布。

`relationshipAutoColor` 写 `null`，禁用 Xmind 的 follow-target-topic 自动线色；否则关系线会取目标主题的填充色（例如 L3 的 `#08773628`）。theme 的 `relationship.line-color` 写 `#087736FF` 作为默认线色；若缺失，Desktop 会回退内置 Classic 的 `#00897B`。`floatingTopicAutoColor` 保留 `follow-color-theme`。

### 3.3 topic

```json
{
  "id": "<uuid>",
  "class": "topic",
  "title": "标题",
  "structureClass": "org.xmind.ui.map.clockwise",
  "notes": { "plain": { "content": "..." }, "realHTML": { "content": "<p>...</p>" } },
  "href": "https://example.com",
  "image": { "src": "xap:resources/<sha256>.png", "width": 320 },
  "labels": ["demo"],
  "markers": [{ "markerId": "people-blue" }],
  "position": { "x": 0, "y": 0 },
  "style": { "id": "<uuid>", "properties": { "...": "..." } },
  "boundaries": [],
  "summaries": [],
  "children": {
    "attached": [],
    "summary": [],
    "callout": [],
    "detached": []
  }
}
```

约定：

- `children` 四种子数组：`attached` / `summary` / `callout` / `detached`。读取、描述、校验、遍历必须全部覆盖。
- v1 新建只产生 `attached`；`summary` 由 `summary` op 产生；`detached` 由 `floating` op 产生；`callout` 只读。
- `position` 只出现在 `detached` 顶层节点上。
- `style` 是逐节点覆盖。读取/编辑时保留；v1 不提供通用样式写入，只允许 `floating` 的 `style` 透传（用于复刻 Desktop 已有的浮动主题外观）。
- 根节点 `structureClass` 决定整体骨架；`extensions` 里的 `org.xmind.ui.skeleton.structure.style` 原样保留（新建时复制固定模板）。
- 未知字段一律保留。实现上加载为 `serde_json::Value`，只改目标路径，不做强类型全量反序列化。

### 3.4 备注

同时写 `plain` 和 `realHTML`：

```json
{
  "plain": { "content": "纯文本" },
  "realHTML": { "content": "<p>含 <strong>粗体</strong> 的 HTML</p>" }
}
```

`plain` 是去标记文本；`realHTML` 由 Markdown 行内语法渲染（粗体、斜体、代码、链接），HTML 转义。`notes: null` 表示清除。

### 3.5 链接

- 主题的 `href` 是整题链接，不支持标题内局部链接。
- 本地路径转绝对 `file:///` URI 并百分号编码，与 Xmind Desktop 写法一致。
- `create` 时 Markdown 里的相对路径按 **markdown 文件所在目录** 解析。
- `link --file` 的相对路径按 **当前工作目录** 解析。

### 3.6 图片

- 只接受本地文件：PNG / JPEG / GIF / WebP；拒绝 SVG、URL、网络路径。
- 复制进 zip：`resources/<sha256>.<ext>`。
- 主题引用：`image: { "src": "xap:resources/<sha256>.<ext>", "width": 320 }`。
- 默认宽度 320，不放大；`--width` 可覆盖。
- manifest 必须登记该资源。

### 3.7 boundary / summary / relationship

boundary 挂在父主题上：

```json
{
  "id": "<uuid>",
  "range": "(0,1)",
  "title": "示例边界",
  "style": { "id": "<uuid>", "properties": { "svg:fill": "#08773680" } }
}
```

- `range` 是 `children.attached` 的闭区间，字符串格式 `"(start,end)"`，从 0 开始。
- 只允许连续的兄弟节点。
- v1 写 boundary 对象 `style.properties.svg:fill: #08773680`；boundary 自身还有一层 10% opacity，实际约 5.0%。不要用 `0x1A`（会被乘成约 1%）。

summary 挂在父主题上，summary 节点放在 `children.summary`：

```json
{ "id": "<uuid>", "range": "(2,3)", "topicId": "<summary topic id>" }
```

relationship 挂在 sheet 上：

```json
{
  "id": "<uuid>",
  "end1Id": "<topic id>",
  "end2Id": "<topic id>",
  "title": "依赖"
}
```

- 端点必须在同一 sheet 内；可以指向 summary 子树。
- v1 不写 relationship 对象 style/line-color；主题 `relationship.line-color` 写 `#087736FF` 作为默认线色。sheet 的 `relationshipAutoColor` 写 `null`，避免 Xmind 按目标主题填充色自动着色；若主题线色缺失会回退 Classic `#00897B`。
- v1 不写 `controlPoints` / `lineEndPoints`，由 Desktop 自动布线；已验证不写也能渲染，Desktop 会按需补。

### 3.8 浮动主题

```json
{
  "id": "<uuid>",
  "title": "Floating Topic",
  "position": { "x": 87.2, "y": 168.7 },
  "style": { "id": "<uuid>", "properties": { "fo:color": "<color>", "svg:fill": "<color>" } },
  "children": { "attached": [] }
}
```

- 位于根节点的 `children.detached`。
- 根节点 id 和所有浮动主题 id 都要出现在 `arrangeableLayerOrder`。
- 删除浮动主题时同步从 `arrangeableLayerOrder` 移除。
- 浮动主题可以拥有普通子主题；`add`/`move` 可以把节点挂到浮动主题下。
- 已验证 `position` + `arrangeableLayerOrder` 工作；浮动主题默认走 §7.2 的 `floatingTopic` 主题。

### 3.9 ID 与保真

- 新对象用 UUID v4。
- 编辑已有文件时保留原 id；新增对象才生成新 id。
- 主题 style id 保留 `assets/theme.json` 的值；Desktop 另存时可能重写，读取逻辑不要依赖具体值。
- 未知字段、未知的 `children` 子数组、未知的资源文件都保留。

## 4. Markdown 语法（仅输入）

```markdown
# 根主题
## 分支
### 子分支
- 列表项也是主题，可任意嵌套
1. 有序列表同理，不自动编号
> 备注，可多行；支持行内粗体/斜体/代码/链接
[标题](https://example.com)
![图](images/x.png)
- [ ] 待办
- [x] 完成
```

规则：

- 全文必须恰好一个 H1；0 个或多个直接报错。
- H2-H6 按层级嵌套；列表和标题可混用。
- `-` / `*` / `+` / 有序列表都产生普通主题；列表项不强制是叶子。
- `>` 连续行成为最近前置主题的备注；空行结束备注块。
- 行内粗体、斜体、代码、链接渲染进 `realHTML`；`plain` 去标记。
- 整行只有一个 `[标题](url)` 时，设为该主题的 `href` 和标题；否则按普通文本处理。
- 整行只有一个 `![alt](path)` 时，图片挂到最近前置主题；`alt` 忽略；相对路径按 md 文件目录解析。
- `- [ ]` / `- [x]` 去掉勾选符号，加 label `todo` / `done`。
- 其他块级语法（表格、独立代码块、水平线等）不在 v1 支持范围，遇到时报错并给行号。
- 普通段落（既不是标题、列表、引用、链接、图片）报错并给行号，禁止静默丢失。
- `create --stdin` 的相对路径按当前工作目录解析。
- 浮动主题不由 Markdown 生成；`create` 后用 `floating` op 添加。
- 输入编码 UTF-8，换行统一 `\n`。

## 5. CLI 契约

### 5.1 命令

```text
xmind-cli-lite create <draft.md> -o <out.xmind> [--skeleton S] [--sheet T] [--title T] [--dry-run] [--force] [--no-validate]
xmind-cli-lite create --stdin -o <out.xmind> --title T [--skeleton S] [--sheet T] [--dry-run] [--force]
xmind-cli-lite read <file.xmind> [--sheet <id|title>] [--format json|outline] [--depth N]
xmind-cli-lite describe <file.xmind> [--sheet <id|title>]
xmind-cli-lite sheets <file.xmind>
xmind-cli-lite validate <file.xmind> [--strict] [--quiet] [--json]
xmind-cli-lite batch <file.xmind> [--input ops.json] [--sheet <id|title>] [--out <copy.xmind>] [--dry-run] [--backup] [--force] [--no-validate]
xmind-cli-lite link <file.xmind> --topic <target> (--url U | --file P | --remove)
xmind-cli-lite image <file.xmind> --topic <target> (--input P | --remove) [--width N]
```

- `read` 默认 `--format json`；`outline` 是人/模型可读的**有损预览**。
- `outline` 只输出层级、备注、链接，浮动主题单列一节；末尾附只读结构附录（boundary/summary/relationship）。**不可回灌**。
- `--depth` 只影响 `outline`；JSON 始终完整。
- `--topic` 是精确标题，必须唯一；重名时报错。需要 path/id 定位时用 `batch`。
- `create --stdin` 没有 H1 时必须给 `--title`；有 H1 时 `--title` 覆盖 H1。
- `describe` 永远 JSON。
- `sheets` 输出 sheet id/title/根标题/节点数。
- `link` / `image` 是单 op 的语法糖，内部走 batch。
- 不提供 `auth`、`skill`、`generate`、`advise`、`theme`、`sheet import`。

### 5.2 输出与退出码

- 变更命令（create/batch/link/image）默认 stdout 输出 JSON；人读信息走 stderr。
- `read --format json`、`describe`、`sheets`、`validate --json` 输出 JSON。
- 错误也输出 JSON：`{"ok":false,"error":{"code":"...","message":"..."}}`。
- 退出码：`0` 成功；`1` IO/文件错误；`2` 用法错误；`3` 校验失败。

变更输出最小形状：

```json
{"ok":true,"file":"out.xmind","sheet":{"id":"...","title":"..."},"topics":42,"warnings":[]}
```

`batch` 额外返回 `"ops": 3`、`"changes": [...]`。

### 5.3 覆盖、备份、原子性

- `create -o` 或 `batch --out` 目标已存在：拒绝，除非 `--force`。
- `batch` 默认原地写：同目录临时文件 -> 校验 -> 原子替换。
- `--backup` 先复制旧文件为 `<file>.bak`。
- `--dry-run` 解析/规划/校验，不落盘，JSON 报告计划。
- 任何 op 失败、校验失败、图片缺失：整批不落盘。

### 5.4 多 sheet

- `read`/`describe` 默认展示全部 sheet；`--sheet` 限定一个。
- `batch`/`link`/`image` 只编辑 `--sheet` 指定的一张（默认第一张），其余 sheet 和未知字段原样保留。
- 不支持跨 sheet 的 op；relationship 端点必须同 sheet。
- 不新建/删除/导入 sheet。

## 6. batch ops

输入是 JSON 数组，来自 `--input` 或 stdin。ops 顺序执行。

### 6.1 目标定位

三选一：

```json
{"id":"<uuid>"}
{"topic":"唯一标题"}
{"path":["父主题","目标主题"]}
```

- `path` 从根节点的子级开始，包含目标自身；根节点用 `[]`。
- `topic` 必须唯一；重名时报错并列出候选 path。
- `id` 最稳；`path` 用于读 JSON 后直接定位。

### 6.2 操作

```json
[
  {"op":"add","parent":{"path":["分支"]},"title":"新节点","index":0,"notes":"...","labels":["x"],"markers":["people-blue"],"href":"..."},
  {"op":"update","topic":{"path":["分支","旧"]},"title":"新"},
  {"op":"delete","topic":{"id":"..."}},
  {"op":"move","topics":[{"path":["A"]}],"parent":{"path":["B"]},"index":0},
  {"op":"notes","topic":{"id":"..."},"notes":"...或 null"},
  {"op":"href","topic":{"id":"..."},"href":"https://..."},
  {"op":"href","topic":{"id":"..."},"file":"../x.xmind"},
  {"op":"label","topic":{"id":"..."},"add":["x"],"remove":["y"]},
  {"op":"marker","topic":{"id":"..."},"add":["people-blue"],"remove":[]},
  {"op":"image","topic":{"id":"..."},"input":"a.png","width":320,"remove":false},
  {"op":"layout","topic":{"id":"..."},"layout":"grid"},
  {"op":"boundary","parent":{"id":"..."},"topics":[{"id":"..."},{"id":"..."}],"title":"..."},
  {"op":"summary","parent":{"id":"..."},"range":[2,3],"text":"..."},
  {"op":"relationship","from":{"id":"..."},"to":{"id":"..."},"label":"..."},
  {"op":"floating","title":"Parking lot","position":{"x":420,"y":60},"style":{"fo:color":"#...","svg:fill":"#..."}},
  {"op":"position","topic":{"id":"..."},"x":420,"y":60}
]
```

规则：

- `add` 默认追加到末尾；`index` 可插入。
- `update` 只改标题；其他字段用各自的 op。
- `delete` 删除整棵子树；浮动主题同步从 `arrangeableLayerOrder` 移除。
- `move` 支持多目标；`parent` 可以是普通主题或浮动主题。
- `notes` / `href` / `label` / `marker` 都是 set/clear 语义。
- `href` 的 `href` 与 `file` 二选一；`href: null` 清除；`file` 的相对路径按当前工作目录解析。
- `image` 最后执行；`remove: true` 清除图片；`input` 的相对路径按当前工作目录解析。
- `layout` 设置子树 `structureClass`；根和一级子主题的覆盖可能被 Desktop 忽略（官方行为），skill 需在 `###` 层做局部布局。
- `boundary` / `summary` 接受 `topics` 列表或 `range: [start,end]`（0 起始、闭区间）；必须是同一父节点下的连续兄弟。boundary 写对象 style `svg:fill: #08773680`。
- `summary` 在 `children.summary` 创建 summary 节点并写 `topicId`。
- `relationship` 端点可在任意子树，包括 summary 子树；不写控制点，也不写对象 style/line-color；默认线色来自主题 `relationship.line-color: #087736FF`。新建 sheet 的 `relationshipAutoColor` 写 `null`；否则 Xmind 用目标主题填充色自动着色。
- `floating` 创建根级 `children.detached` 节点，并加入 `arrangeableLayerOrder`。
- `position` 只对浮动主题有效；非浮动主题报错。
- `floating.style` 是原始 `properties` 透传，不是通用样式 API。

### 6.3 明确不支持

`theme`、`emphasis`、`fontColor`、`fillColor`、`callout`、`numbering`、`background`、`themeColor`、`todo`、`sheet` 系列。

## 7. 骨架与主题

### 7.1 骨架别名

`--skeleton` 和 `layout` 接受友好名和官方名，统一映射到 `structureClass`：

| 别名 | 官方名 | structureClass |
|---|---|---|
| `mindmap` | `MindMap-1` | `org.xmind.ui.map.clockwise`（默认，balanced） |
| `mindmap-unbalanced` | `MindMap-2` | `org.xmind.ui.map.unbalanced` |
| `mindmap-balanced` | `MindMap-1` | `org.xmind.ui.map.clockwise`（`mindmap` 别名） |
| `logic-right` | `LogicChart-1` | `org.xmind.ui.logic.right` |
| `logic-left` | — | `org.xmind.ui.logic.left` |
| `org-down` | `OrgChart-1` | `org.xmind.ui.org-chart.down` |
| `tree-right` | `TreeChart-1` | `org.xmind.ui.tree.right` |
| `timeline` | `Timeline-1` | `org.xmind.ui.timeline.horizontal` |
| `timeline-side` | `Timeline-3` | `org.xmind.ui.timeline.sided.horizontal` |
| `matrix` | `Matrix-1` | `org.xmind.ui.spreadsheet` |
| `grid` | `Matrix-2` | `org.xmind.ui.grid` |
| `brace` | `BraceMap-1` | `org.xmind.ui.brace.right` |
| `fishbone` | `Fishbone-1` | `org.xmind.ui.fishbone.leftHeaded` |
| `treetable` | `TreeTable-1` | `org.xmind.ui.treetable.toptitle` |

- 新建默认 `mindmap` = `org.xmind.ui.map.clockwise`（balanced）；`--skeleton` 可覆盖。
- 按分支改布局只用 `layout` op；不提供 `create --layout "分支=..."` 语法。
- 根节点和一级子主题的局部覆盖可能被 Desktop 忽略；skill 要求在 `###` 或更深层做包装节点。

### 7.2 主题

- 只内嵌一套主题；**不提供 `--color`、`theme` op、多主题切换**。
- 主题源：`assets/theme.json`（严格 JSON）；Rust 用 `include_str!` 加载。只维护这一份，不再保留 JSONC 中间文件。
- 自包含：不写 `colorThemeId` / `skeletonThemeId`。内置 Mind Map 骨架 `8f137afe...` 会在自定义主题之后强制 `subTopic.fillPattern: none`、`level3.fillPattern: solid`，覆盖自定义填充；结构由 `root.structureClass` + `extensions` 提供。
- 字体：不用 `global`；逐元素显式 `Fira Noto SC`。central 30pt/800；mainTopic 18pt/500；level3/subTopic 14pt/400；floatingTopic 14pt/500；relationship 13pt/400；summaryTopic 14pt/400；boundary/zone 14pt/400。
- 层级：`centralTopic` = L1，`mainTopic` = L2，其余主题默认 `subTopic`；`level3` 单独覆盖 L3。L3 填充 `#08773628` + `fill-pattern: solid`；L4+ 无填充（`fill-pattern: none`）；L3/subTopic 文字 `inherited`。
- 文字/画布：root style `fo:color #0A0A0A`（central 继承）；mainTopic 文字 `#EDF0E8FF`（画布色）；relationship 文字 `#0A0A0A`；boundary/zone 文字 `#EDF0E8FF`。`theme.map.svg:fill #EDF0E8FF`；新文件不写 `sheet.style`（已验证全局画布生效）。
- mainTopic 填充 `#087736FF` + solid；root style `line-color #087736FF`。
- floatingTopic：`svg:fill #5C6B2140` + solid；`border-line-width: 0pt`（边框不启用）；`border-line-color: #5C6B21FF`（占位）。
- boundary：主题 fill `#08773680`、line `#087736FF` dash、文字 `#EDF0E8FF`；新建 boundary 对象写 `style.properties.svg:fill #08773680`。boundary 自身还有一层 10% opacity，实际约 5.0%；不要用 `0x1A`（会被乘成约 1%）。
- zone：`svg:fill #0877361A`（实测 10%）、`border-line-color #087736FF`、文字 `#EDF0E8FF`；不写对象 style。
- summary / summaryTopic：绿色线/边框 `#087736FF`；summaryTopic 文字 `inherited`。
- relationship：默认 `line-color #087736FF`、`line-width: 2`、`line-pattern: dash`；对象不写 style；sheet `relationshipAutoColor: null`。自动色开启时 Xmind 会取目标主题填充色（L3 -> `#08773628`）；主题线色缺失时回退内置 Classic `#00897B`。
- 不生成 `calloutTopic` / `importantTopic` / `minorTopic` / `expiredTopic`；删除彩虹分支 `map.color-list` / `multi-line-colors`。
- alpha：`0x28` = 15.686%（UI 15% 截断）、`0x1A` = 10.196%、`0x80` = 50.196%；boundary 的 10% 层再乘，实际约 5.0%。
- 生成 `.xmind` 时保留主题 style id；只给新 sheet/root/topic 生成 id。读取/编辑已有文件时原样保留其 theme，不强制替换。
- boundary 文字 `#EDF0E8FF` 在约 5% 底色上已确认可接受；zone 文字已确认。
- 以上是实现/验收的关键值；未列出的 shape、arrow、line、text-align 等以 `assets/theme.json` 为准。

## 8. validate

### 8.1 错误（exit 3）

- 不是 zip / 缺 `content.json` / JSON 非法 / 空 sheet 数组。
- 缺 `rootTopic` / `rootTopic` 不是对象。
- sheet id 或 topic id 重复。
- `children` 某子数组不是数组。
- `notes` / `image` / `boundaries` / `summaries` / `relationships` 结构非法。
- boundary/summary/relationship 引用不存在的 id。
- boundary/summary 的 `range` 越界或不是闭区间格式。
- 图片 `xap:resources/...` 指向的 zip 条目缺失。
- manifest 未登记 `content.json` 或已存在的资源。

### 8.2 警告（exit 0）

- 未知 `structureClass`（`--strict` 时升级为错误）。
- 空标题。
- 同一父节点下重名。
- 深度 > 8；节点数 > 1500。

### 8.3 行为

- `create` / `batch` 成功后自动 validate；失败删除临时输出并返回 3。
- `--no-validate` 跳过自动校验（仍做解析级检查）。
- `--strict` 把警告升级为错误。
- `--quiet` 只返回退出码。
- `--json` 输出 `{"ok":bool,"file":"...","errors":[],"warnings":[]}`。

## 9. Skill

目录：仓库 `skills/xmind-cli-lite/`；使用时复制到所用 Agent 的 skills 目录（复制，不用 symlink）。

```text
skills/xmind-cli-lite/
  SKILL.md
  references/markdown-grammar.md
  references/skeletons.md
  references/edit.md
  references/quality-gate.md
  recipes/quick-map.md
  recipes/knowledge-map.md
  recipes/code-architecture.md
  recipes/project-execution.md
  recipes/comparison-decision.md
```

规则：

- 正文中文，命令/JSON 保持英文。
- 新建前加载 `markdown-grammar` + 对应 recipe；改旧前先 `read --format json`。
- 编辑优先一次 `batch` 完成；改后必须 `validate`。
- `read --format outline` 只用于上下文预览，不能用于回灌。
- 图片只允许本地文件；agent 需要网图时自行下载再传入。
- 不为凑视觉锚点造空节点；不要为了 boundary/summary 发明包装层。
- 最终回复给文件路径、校验结果、1–2 个关键设计选择。

## 10. 测试与验收

### 10.1 自动化

- Markdown 解析：标题/列表/备注/链接/图片/复选框/错误行号。
- ops：add/update/delete/move/notes/href/label/marker/image/layout/boundary/summary/relationship/floating/position。
- 读写往返：对 36 个官方模板做 read -> validate；不把模板提交进仓库。
- 生成 fixture：覆盖 summary/boundary/relationship/floating/notes/链接/图片；fixture 由测试临时生成，不提交 `.xmind`。
- 生成 JSON 的 id 随机，golden 测试需归一化 id。

### 10.2 Desktop 人工验收（必须）

1. 最小生成文件：能打开、主题/结构正确、可编辑另存。
2. 编辑回路：batch add/move/boundary/summary/relationship/floating，重开确认。
3. 图片与链接：本地图片显示，`file:///` 链接可点。

### 10.3 本地资产

- `reference/`：从 Xmind 解出的官方主题 JSON，仅本地参考，已 gitignore；不进入发布物。
- 官方模板、测试 fixture 不进入发布物。

## 11. 实现约定

- Rust 2024 edition；二进制名 `xmind-cli-lite`。
- 依赖最小化；候选：`clap`、`serde`/`serde_json`、`zip`、`pulldown-cmark`、`uuid`、`sha2`、`anyhow`/`thiserror`。最终选型在实现前定。
- 零网络：不引入 reqwest/tokio。
- 跨平台核心，Windows 为唯一验收环境：
  - zip 条目路径统一 `/`。
  - `file:///` URI 用 `/` 和百分号编码。
  - 原子替换处理 Windows 上目标被占用/已存在的情况。
  - stdout 强制 UTF-8。
- 未知字段用 `serde_json::Value` 透传；不做全量强类型反序列化。
- 编辑已有文件时，只重写 `content.json`/`metadata.json`/`manifest.json` 和新增资源；其他 zip 条目原样复制。
- 临时文件写在目标同目录，成功后替换；失败清理。

## 12. 实现前待定

- **crate 选型与模块划分**：进入实现前单独定。
