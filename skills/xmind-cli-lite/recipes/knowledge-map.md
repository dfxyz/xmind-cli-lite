# Recipe: 知识地图

适用：学习一个领域、读书笔记、概念关系梳理。

## 骨架

默认 `mindmap`；概念有明确先后时用 `logic-right`。

## 草稿

```markdown
# 领域

## 核心概念

### 概念 A

- 定义
- 关键特征
  > 与概念 B 的区别：...
- 例子

### 概念 B

- 定义
- 适用边界

## 方法

- 步骤一
- 步骤二

## 常见误区

- 误区一
- 误区二

## 参考资料

[官方文档](https://example.com)
```

## 补充结构

一次 batch 给分组加 boundary、给结论加 summary：

```json
[
  {"op":"boundary","parent":{"path":["核心概念"]},"range":[0,1],"title":"基础"},
  {"op":"summary","parent":{"path":["常见误区"]},"range":[0,1],"text":"先验证前提"},
  {"op":"relationship","from":{"path":["方法","步骤一"]},"to":{"path":["核心概念","概念 A"]},"label":"依赖"}
]
```

## 约束

- 每个分支先结构后细节；不要把所有定义都放标题。
- 链接放备注或独立链接节点，不要塞进标题。
- 一页超过 80 个节点时，拆成多个 sheet 或拆文件；v1 不支持新建多 sheet，建议拆文件。
