# Recipe: 代码架构图

适用：模块划分、调用链、依赖关系、重构方案。

## 骨架

- 分层/流程：`logic-right`
- 模块从属：默认 `mindmap` 或 `org-down`
- 模块很多、关系复杂：默认 `mindmap`，用 relationship 画依赖

## 草稿

```markdown
# 系统架构

## 入口

- CLI
- HTTP API

## 核心

### 解析

- lexer
- parser
  > 输入契约：...

### 存储

- repository
- migrations

## 外部依赖

- 数据库
- 对象存储

## 风险

- 单点
- 迁移窗口
```

## 补充关系与边界

```json
[
  {"op":"boundary","parent":{"path":["核心"]},"range":[0,1],"title":"纯函数层"},
  {"op":"relationship","from":{"path":["入口","CLI"]},"to":{"path":["核心","解析"]},"label":"调用"},
  {"op":"notes","topic":{"path":["核心","存储","repository"]},"notes":"入口文件：src/store.rs:42"}
]
```

## 局部布局

给某个大分支换布局时，先加有语义的包装节点，例如 `### 数据流`，再：

```json
{"op":"layout","topic":{"path":["核心","数据流"]},"layout":"logic-right"}
```

## 约束

- 文件路径、行号放备注，不要做标题。
- relationship 只画重要依赖，避免蜘蛛网。
- 不写逐节点颜色；层级已经表达重要性。
- 架构图不是文件树；叶子节点控制在 5–9 个。
