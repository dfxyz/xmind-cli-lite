---
name: xmind-cli-lite
description: 创建、读取、编辑、校验本地 Xmind（.xmind，Zen 格式）。当用户要做思维导图、把大纲/笔记/方案整理成 Xmind、修改已有 .xmind，或询问离线 Xmind 工作流时使用。
---

# xmind-cli-lite

用 `xmind-cli-lite` 处理 `.xmind`。离线、无登录、无云端；不要用官方 `xmind` CLI，也不要手工改 zip。

## 前置

```bash
xmind-cli-lite --help
```

不在 PATH 时，在仓库根目录执行 `cargo install --path .`。

退出码：`0` 成功；`1` 文件/IO；`2` 用法/输入；`3` 校验失败。

## 先读哪些

- 新建：`references/markdown-grammar.md` + 一个 `recipes/` 文件。
- 改旧：先 `read --format json`，再读 `references/edit.md`。
- 选布局：`references/skeletons.md`。
- 交付前：`references/quality-gate.md`。

## 新建

1. 写 Markdown。全文恰好一个 H1；图片只用本地路径；独立成行的链接/图片前后留空行。
2. 生成：

```bash
xmind-cli-lite create draft.md -o out.xmind
```

3. 需要 boundary / summary / relationship / 浮动主题时，用一次 batch 补：

```bash
xmind-cli-lite batch out.xmind --input ops.json
```

4. 校验：

```bash
xmind-cli-lite validate out.xmind --json
```

无 H1 的管道输入用 `create --stdin -o out.xmind --title "根主题"`；有 H1 时 `--title` 覆盖 H1。

## 改旧

1. 读成 JSON：

```bash
xmind-cli-lite read old.xmind --format json > tree.json
```

输出与 `content.json` 同构，attached/detached 的 topic 额外带 `path`：从根的子级开始，根为 `[]`。

2. 规划 ops。优先用 `id`；`path` 次之；`topic` 标题必须全 sheet 唯一。
3. 先 `--dry-run`，再一次 batch 正式执行；重要文件加 `--backup`。
4. 改完必须 `validate`。
5. 多 sheet 只改 `--sheet` 指定的一张（默认第一张），其余原样保留。

## 查看

```bash
xmind-cli-lite describe x.xmind
xmind-cli-lite sheets x.xmind
xmind-cli-lite read x.xmind --format outline --depth 2
```

`outline` 是有损预览，不能回灌；要改文件必须从 JSON + batch。

## 硬规则

- 不做主题切换、逐节点颜色/字体、callout、todo op、sheet 导入删除。
- 图片只接受本地 PNG/JPEG/GIF/WebP；网图先下载。
- 不要为 boundary/summary 造包装节点，不要为视觉平衡造空节点。
- 改后 validate 必须为 0；`--strict` 下警告也算失败。
- 结构变化后，提醒用户在 Xmind Desktop 打开确认。

## 交付回复

给：文件绝对路径、validate 结果、1–2 个关键设计选择（骨架、层级、boundary 等）。不要贴整份 JSON。
