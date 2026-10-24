# 骨架与局部布局

## 别名

| 意图 | 别名 | 官方名 |
|---|---|---|
| 默认平衡思维导图 | `mindmap` | MindMap-1 |
| 单向思维导图 | `mindmap-unbalanced` | MindMap-2 |
| 右向逻辑图 | `logic-right` | LogicChart-1 |
| 左向逻辑图 | `logic-left` | — |
| 向下组织图 | `org-down` | OrgChart-1 |
| 右向树图 | `tree-right` | TreeChart-1 |
| 横向时间线 | `timeline` | Timeline-1 |
| 双边时间线 | `timeline-side` | Timeline-3 |
| 矩阵 | `matrix` | Matrix-1 |
| 网格 | `grid` | Matrix-2 |
| 大括号图 | `brace` | BraceMap-1 |
| 鱼骨图 | `fishbone` | Fishbone-1 |
| 树表 | `treetable` | TreeTable-1 |

`create --skeleton` 和 `layout` op 都接受别名、官方名或完整 `org.xmind.ui.*`。

## 怎么选

- 默认 `mindmap`：通用、最稳。
- `logic-right`：有先后/因果/流程，分支单向展开。
- `org-down`：组织、职责、归属。
- `tree-right`：分类树、目录。
- `timeline` / `timeline-side`：阶段、里程碑；一级分支就是时间点。
- `matrix` / `grid`：少数并列项的对照；层级深时容易拥挤，先试默认骨架。
- `brace` / `fishbone`：整体拆分、因果分析；内容要少。
- `treetable`：需要表格感时；仍然只有树层级。

## 局部布局

```json
{"op":"layout","topic":{"path":["方案 A"]},"layout":"logic-right"}
```

根节点和一级子主题的局部覆盖可能被 Xmind Desktop 忽略。要给某分支换布局时，在该分支下加一个普通包装节点，把 layout 设到包装节点或更深层；包装节点要有真实语义，不要空节点。

## 注意

- 只改 `structureClass`；不提供 `create --layout "分支=..."` 语法。
- 骨架影响的是结构，不是主题配色；主题只有一套。
- 换骨架后仍要 validate，并用 Desktop 打开确认。
