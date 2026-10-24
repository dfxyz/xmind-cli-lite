# 交付前质量门

## 必跑

```bash
xmind-cli-lite validate file.xmind --json
xmind-cli-lite describe file.xmind
xmind-cli-lite read file.xmind --format outline --depth 3
```

要求：

- `errors` 为空；`warnings` 逐条看过。
- 有 `--strict` 需求时，重跑 `validate --strict`。
- 节点数、深度、重名、空标题都在可接受范围。

## 结构

- 只有一个 H1/根；层级没有为了排版而造的空节点。
- 一级分支数量与内容匹配，通常 3–7 个；过多时考虑分组。
- 同级标题语义一致（名词/动词/问题不要混着用）。
- boundary 只覆盖连续兄弟，range 与当前顺序一致。
- summary 有实际结论，不是把子节点再抄一遍。
- relationship 端点存在，方向与含义一致。
- 浮动主题在 `arrangeableLayerOrder` 中，位置不压住主干。
- 备注承载细节，不要把长段落塞进标题。
- 图片为本地文件，能显示；链接可点。

## 内容

- 标题短、可扫读；一个节点一个意思。
- 备注解释 why / 例子 / 验收标准，不复述标题。
- 待办用 `todo` / `done` label，不用标题里的 `[ ]`。
- 风险、阻塞、决策有明确标记（label/marker 或独立分支）。

## Desktop 人工验收

自动化过了也要在 Xmind Desktop 打开：

1. 主题、骨架、颜色是否正常。
2. boundary/summary/relationship/浮动主题是否渲染。
3. 图片是否显示，`file:///` 链接是否能点。
4. 能否继续编辑并另存。

发现渲染问题，先判断是结构（range、id、order）还是主题；不要直接改 zip。

## 回复

给文件绝对路径、validate 结果、1–2 个关键设计选择；提一句 Desktop 验收结论或待确认点。
