# 人机协作决策收件箱

## 1. 问题

当前系统的人机交互模式有三个致命缺陷：

1. **人必须主动盯着** — 不推送，不通知，决策点混在活动日志里
2. **决策散落各处** — 审批在任务里、验收在任务里、问题在问题列表里、风险在运行日志里
3. **无法批量处理** — 每个决策都要单独操作，10 个待确认就要点 10 次

## 2. 设计目标

**人类不应该适应 Agent 的节奏，Agent 应该适应人类的节奏。**

核心原则：
- Agent 持续工作，遇到决策点就**投递到收件箱**
- 人类在方便的时候打开收件箱，看到所有待处理事项
- 每个决策都是一张**卡片**，包含完整上下文和推荐操作
- 支持**一键操作**和**批量处理**
- 低风险决策有**超时默认值**，不处理也不阻塞
- 高风险决策**必须人工确认**，超时则暂停

## 3. 决策类型

| 类型 | 紧急度 | 超时行为 | 示例 |
|------|--------|----------|------|
| `approval` | 中 | 超时暂停 | 任务需要审批后才能执行 |
| `acceptance` | 低 | 超时自动接受 | 任务完成后验收确认 |
| `reassess` | 低 | 超时按规则引擎默认 | 暂停任务该重启还是结束 |
| `risk_ack` | 高 | 超时暂停 | 危险操作确认（删除文件/强推等） |
| `scope_change` | 中 | 超时暂停 | Agent 建议扩大/缩小任务范围 |
| `question` | 低 | 超时跳过 | Agent 需要澄清信息 |
| `conflict` | 高 | 超时暂停 | 多任务/多 Agent 冲突 |
| `budget` | 高 | 超时暂停 | 预算或成本超出阈值 |

## 4. 决策卡片结构

```
DecisionCard {
  id:            UUID
  project_id:    UUID
  task_id:       Option<UUID>       // 关联任务（可选）
  kind:          DecisionKind       // approval / acceptance / reassess / ...
  urgency:       low / medium / high
  title:         String             // 一句话标题
  context:       String             // 完整上下文（markdown）
  options:       Vec<DecisionOption> // 可选操作
  recommended:   Option<String>     // Agent 推荐的选项 ID
  confidence:    f32                // Agent 对推荐的置信度
  timeout_secs:  Option<u64>        // 超时时间（秒）
  timeout_action: Option<String>    // 超时后自动执行的操作 ID
  status:        pending / resolved / expired / dismissed
  created_at:    String
  resolved_at:   Option<String>
  resolved_by:   Option<UUID>       // 谁处理的
  chosen_option: Option<String>     // 选了哪个
}

DecisionOption {
  id:      String       // "approve" / "reject" / "restart" / ...
  label:   String       // 按钮文字
  style:   String       // "primary" / "success" / "warn" / "danger"
  detail:  Option<String> // 补充说明
}
```

## 5. 交互模式

### 5.1 收件箱视图

```
┌─────────────────────────────────────────────────┐
│ 📥 决策收件箱  (5 个待处理)          [全部标记已读] │
├─────────────────────────────────────────────────┤
│                                                  │
│ 🔴 [高] 危险操作确认                    2 分钟前  │
│    任务《重构数据库》要执行 DROP TABLE              │
│    [确认执行]  [拒绝并暂停]                       │
│                                                  │
│ 🟡 [中] 任务审批                       10 分钟前  │
│    《[0.1.5] 后台管理面板》需要审批才能执行        │
│    规则引擎建议：批准 (置信度 0.85)               │
│    [批准]  [拒绝]  [需要更多信息]                 │
│                                                  │
│ 🟢 [低] 任务重评估  ×3               30 分钟前   │
│    3 个暂停任务需要决定继续还是结束                │
│    [查看详情]  [全部按推荐处理]                    │
│                                                  │
│ 🟢 [低] 验收确认                       1 小时前  │
│    《[0.1.2] Agent 调用 MVP》已完成               │
│    2 小时后自动接受                               │
│    [接受]  [退回重做]                             │
│                                                  │
└─────────────────────────────────────────────────┘
```

### 5.2 批量操作

同类型的决策可以分组，一键批处理：
- "3 个重评估全部按推荐处理"
- "5 个低风险验收全部接受"
- "按规则引擎推荐处理所有置信度 > 0.8 的决策"

### 5.3 超时与默认

| 紧急度 | 默认超时 | 超时行为 |
|--------|----------|----------|
| high | 无超时 | 阻塞直到人工处理 |
| medium | 2 小时 | 按 Agent 推荐执行 |
| low | 30 分钟 | 按 Agent 推荐执行 |

### 5.4 通知策略

- 高紧急度：桌面弹窗 + 声音提醒
- 中紧急度：桌面托盘图标变化
- 低紧急度：下次打开收件箱时显示

## 6. 与现有系统的关系

决策收件箱**统一**了以下现有功能：
- `PendingQuestion` → 变成 kind=question 的决策卡片
- `TaskApprovalState` → 变成 kind=approval 的决策卡片
- `TaskAcceptanceState` → 变成 kind=acceptance 的决策卡片
- reassess 结果 → 变成 kind=reassess 的决策卡片
- 策略 Agent 异常检测 → 变成 kind=conflict 的决策卡片
