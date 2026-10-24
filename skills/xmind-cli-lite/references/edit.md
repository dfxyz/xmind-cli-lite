# 编辑已有 .xmind

## 读取

```bash
xmind-cli-lite read file.xmind --format json
xmind-cli-lite read file.xmind --sheet <id|title> --format json
```

输出是 sheet 数组，与 `content.json` 同构；topic 额外带 `path`。未知字段、主题、缩略图、`content.xml` 都会保留。

`path` 只覆盖 `children.attached` 和 `children.detached`；summary / callout 子树里的 topic 用 `id` 定位。

定位三选一：

```json
{"id":"<uuid>"}
{"topic":"唯一标题"}
{"path":["父主题","目标主题"]}
```

- `path` 从根的子级开始，包含目标自身；根是 `[]`。
- 浮动主题也可用 `path:["浮动主题"]`。
- `topic` 全 sheet 精确匹配；重名会报错并列出候选。
- 被移动/删除后旧 id 失效，后续 op 要用新位置重新读。

## 官方模板不要当主题底稿

官方模板可能自带 `relationshipAutoColor: "follow-target-topic"` 和旧的 `relationship.style.line-color`（常见半透明绿）。`batch` 会原样保留这些字段，关系线颜色由模板决定，不受 CLI 主题控制。需要统一主题色时用 `create` 新建，不要基于官方模板改。

## batch 输入

ops 是 JSON 数组，按顺序执行；任何一步失败整批不落盘。

```bash
xmind-cli-lite batch file.xmind --input ops.json --dry-run
xmind-cli-lite batch file.xmind --input ops.json --backup
xmind-cli-lite batch file.xmind --out copy.xmind
```

`--dry-run` 规划/校验但不写；`--no-validate` 跳过自动校验；`--force` 允许覆盖 `--out`。

## 单点修改的语法糖

只改一个链接或图片时，可以不走 ops 文件：

```bash
xmind-cli-lite link file.xmind --topic "唯一标题" --url https://example.com
xmind-cli-lite link file.xmind --topic "唯一标题" --file ../doc.md
xmind-cli-lite link file.xmind --topic "唯一标题" --remove

xmind-cli-lite image file.xmind --topic "唯一标题" --input pic.png --width 480
xmind-cli-lite image file.xmind --topic "唯一标题" --remove
```

它们内部走一个 batch op，同样会校验后原地写回；`--sheet` 可指定 sheet。

## 操作

### add

```json
{"op":"add","parent":{"path":["分支"]},"title":"新节点","index":0,"notes":"备注","labels":["x"],"markers":["people-blue"],"href":"https://example.com"}
```

- 默认追加；`index` 插入。
- `labels` 是字符串数组；`markers` 是 markerId 字符串数组。
- 不要用空标题凑结构。

### update / delete / move

```json
{"op":"update","topic":{"id":"..."},"title":"新标题"}
{"op":"delete","topic":{"id":"..."}}
{"op":"move","topics":[{"id":"..."}],"parent":{"path":["B"]},"index":0}
```

- `delete` 删整棵子树；自动清理指向它的 relationship 和失效的 summary。
- 删除/移动会用掉父节点上的 boundary/summary range；range 空了会一并删除对应结构。
- 不能删/移根主题；不能移进自身子树。
- 移动多个主题时 `index` 按全部移除后的目标列表计算。

### notes / href

```json
{"op":"notes","topic":{"id":"..."},"notes":"纯文本，支持 **Markdown** 行内语法"}
{"op":"notes","topic":{"id":"..."},"notes":null}
{"op":"href","topic":{"id":"..."},"href":"https://example.com"}
{"op":"href","topic":{"id":"..."},"file":"../doc.md"}
{"op":"href","topic":{"id":"..."},"href":null}
```

`file` 相对当前工作目录，转成绝对 `file:///`。标题内不能放链接。

### label / marker

```json
{"op":"label","topic":{"id":"..."},"add":["risk"],"remove":["todo"]}
{"op":"marker","topic":{"id":"..."},"add":["star-red"],"remove":[]}
```

清空后字段会移除。

### image

```json
{"op":"image","topic":{"id":"..."},"input":"photo.png","width":480}
{"op":"image","topic":{"id":"..."},"remove":true}
```

只接受本地 PNG/JPEG/GIF/WebP；按 sha256 去重；默认宽度 320 且不放大。

### layout / position / floating

```json
{"op":"layout","topic":{"id":"..."},"layout":"logic-right"}
{"op":"floating","title":"Parking lot","position":{"x":420,"y":60}}
{"op":"position","topic":{"id":"..."},"x":300,"y":80}
```

- `position` 只对根级浮动主题有效。
- 浮动主题加入 `arrangeableLayerOrder`；删除时自动移除。
- `floating.style` 是原始 properties 透传，不是通用样式 API：

```json
{"op":"floating","title":"Parking","position":{"x":420,"y":60},"style":{"fo:color":"#0A0A0AFF","svg:fill":"#5C6B2140"}}
```

### boundary / summary

```json
{"op":"boundary","parent":{"path":["分支"]},"topics":[{"id":"..."},{"id":"..."}],"title":"阶段一"}
{"op":"boundary","parent":{"path":["分支"]},"range":[0,2],"title":"阶段一"}
{"op":"summary","parent":{"path":["分支"]},"range":[2,3],"text":"结论"}
```

- range 是父节点 `children.attached` 的 0 起始闭区间。
- `topics` 必须映射到同一父节点下的连续兄弟。
- boundary 自动写 `svg:fill: #08773680`。
- summary 会在 `children.summary` 建节点并写 `topicId`。

### relationship

```json
{"op":"relationship","from":{"path":["A"]},"to":{"path":["B"]},"label":"依赖"}
```

- 端点可在任意子树，包括 summary。
- 不写控制点；默认线色来自主题 `#087736FF`，sheet 的 `relationshipAutoColor` 会置 `null`。
- 读取旧文件时，relationship 也可能指向 boundary 或 zone；编辑时用 topic id 最稳。

## 常见错误

- `path 第 N 段 ... 不存在`：path 只含标题，不含 sheet 根；重新 read。
- `标题 ... 不唯一`：改用 id。
- `range ... 越界`：先 read 确认父节点当前 attached 数量。
- `callout 主题只读`：不要改 callout 子树。
- `校验失败`：整批未写；按错误修 ops 或先修文件。
