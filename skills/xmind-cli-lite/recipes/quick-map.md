# Recipe: 快速单页导图

适用：会议记录、一个小主题的梳理、临时方案。目标是一页能看完。

## 骨架

默认 `mindmap`。不要加 skeleton 参数。

## 草稿

```markdown
# 主题

## 背景

- 现状
- 约束

## 方案

- 方案 A
- 方案 B

## 下一步

- [ ] 动作一
- [ ] 动作二

> 关键结论写在这里
```

## 执行

```bash
xmind-cli-lite create draft.md -o quick.xmind
xmind-cli-lite validate quick.xmind --json
```

需要标注时，一次 batch：

```json
[
  {"op":"label","topic":{"path":["下一步","动作一"]},"add":["today"]},
  {"op":"marker","topic":{"path":["方案"]},"add":["star-red"]}
]
```

## 约束

- 3–5 个一级分支，深度不超过 4。
- 不建 boundary/summary，除非确实要强调一组或给结论。
- 标题控制在 20 字内；细节放备注。
