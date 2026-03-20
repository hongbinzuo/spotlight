# API Design

## 1. API Layers

There are two API surfaces:

- central service API
- local desktop core API

The desktop UI should talk to:

- the local core for session operations and local execution details
- the central service for project/task coordination and shared state

## 2. Central Service HTTP API

Base prefix:

- `/api/v1`

当前 `0.1.0` 服务端落地说明：

- 统一入口页面继续挂在 `/`
- JSON API 主前缀为 `/api/v1`
- 为兼容已有页面脚本与测试，`/api` 保留同构兼容路由

Provider note:

- APIs should treat Agent runtime as provider-neutral even though Codex is the first implementation
- API and event payloads should carry explicit schema versions before external ecosystem integrations begin

## 3. Project APIs

### `GET /projects`

Returns projects visible to the current user.

当前 `0.1.0` 实现说明：

- 先返回服务端内存态里的项目列表
- 当前字段为最小骨架：`id`、`name`、`description`、`workspace_roots`、`is_spotlight_self`
- 项目可见性与能力摘要留到后台能力版本再补齐

Response fields:

- project id
- key
- name
- visibility mode
- visible teams/users summary
- current user capabilities

### `POST /projects`

Creates a project.

Request:

```json
{
  "key": "demo",
  "name": "Demo Project",
  "description": "Cross-platform Agent GUI",
  "visibilityMode": "mixed"
}
```

### `POST /projects/{projectId}/visibility-rules`

Adds or updates project visibility.

### `POST /projects/{projectId}/workspaces`

Registers a project workspace root.

Request:

```json
{
  "label": "backend",
  "path": "C:\\Users\\zuoho\\code\\demo5\\backend",
  "isPrimaryDefault": true,
  "isWritable": true
}
```

## 4. Task APIs

### `GET /projects/{projectId}/tasks`

Supports filters:

- `status`
- `assignmentMode`
- `requestedAgentId`
- `mine`
- `pendingAcceptance`
- `manualReview`

当前 `0.1.0` 实现说明：

- 已提供项目级任务看板读取接口
- 当前先支持 `status` 查询参数
- 其余筛选项作为 `0.1.1` 看板增强范围继续补齐

### `POST /projects/{projectId}/tasks`

Creates a task from free-form text.

Request:

```json
{
  "title": "Implement task queue UI",
  "description": "Build the left-side task list and integrate claim/approve actions.",
  "assignmentMode": "public_queue",
  "requestedAgentId": null,
  "approvalRequired": true,
  "primaryWorkspaceId": "workspace-uuid",
  "acceptanceOwnerUserId": "user-uuid"
}
```

### `POST /tasks/{taskId}/claim`

Human-driven claim for a specific Agent.

### `POST /tasks/{taskId}/request-approval`

Moves the task into approval flow.

### `POST /tasks/{taskId}/approve`

Approves a task for execution.

### `POST /tasks/{taskId}/reassign`

Assigns or reassigns a task to another Agent.

Request:

```json
{
  "assignedAgentId": "agent-uuid",
  "reason": "Original agent offline"
}
```

### `POST /tasks/{taskId}/accept`

Marks task output as accepted.

### `POST /tasks/{taskId}/reject`

Rejects task output and returns task to `open`.

Request:

```json
{
  "comment": "Acceptance criteria not met"
}
```

### `POST /tasks/{taskId}/cancel`

Cancels an unstarted or manually halted task.

### `GET /agents`

Returns the minimal Agent list required by the right-side Agent panel.

当前 `0.1.0` 实现说明：

- 返回 `id`、`owner_user_id`、`name`、`provider`、`status`、`auto_mode`、`current_task_id`、`last_action`
- 能力协商、Provider 归一化能力与心跳扩展继续沿用下方协调接口推进

## 5. Agent Coordination APIs

### `POST /agents/{agentId}/auto-mode`

Turns auto mode on or off.

Request:

```json
{
  "enabled": true
}
```

### `GET /agents/{agentId}/capabilities`

Returns normalized provider capabilities for UI feature gating.

Example:

```json
{
  "providerType": "codex",
  "providerMode": "native_acp",
  "capabilities": {
    "streamingText": true,
    "toolCalls": true,
    "fileChangeEvents": true,
    "sessionResume": true
  }
}
```

### `POST /agents/{agentId}/heartbeat`

Updates presence and status.

Request:

```json
{
  "machineId": "macbook-pro-01",
  "status": "online",
  "currentTaskId": null
}
```

### `POST /agents/{agentId}/pull-next`

Atomically allocates the next eligible task for the Agent.

Selection rules:

- assigned tasks for that Agent first
- then oldest eligible queued task visible to that Agent
- completed tasks must be skipped

Current `0.1.0` implementation notes:

- only `open` tasks are eligible for automatic allocation
- allocation order is based on task creation/activity timestamp from oldest to newest
- allocated tasks transition to `CLAIMED` immediately in the server state
- the current minimal response only returns the allocated task, without a separate `runConfig` payload

Response:

```json
{
  "task": {
    "id": "task-uuid",
    "title": "Implement task queue UI",
    "description": "Build the left-side task list and integrate claim/approve actions.",
    "status": "CLAIMED"
  }
}
```

### `POST /tasks/{taskId}/runs`

Creates a task run and enters preflight state.

### `POST /task-runs/{runId}/attempts`

Records a new attempt.

### `POST /task-runs/{runId}/complete`

Marks Agent execution complete and transitions toward acceptance.

### `POST /task-runs/{runId}/fail`

Records failure and lets the service decide retry or manual review.

Request:

```json
{
  "errorSummary": "Tests failed in backend workspace",
  "recoverable": true
}
```

### `POST /task-runs/{runId}/rollback`

Starts a rollback workflow.

Request:

```json
{
  "reason": "Task introduced breaking repository changes"
}
```

## 6. Audit APIs

### `POST /audit-events`

Ingests structured audit events from the desktop core.

Payload shape:

```json
{
  "projectId": "project-uuid",
  "taskId": "task-uuid",
  "taskRunId": "run-uuid",
  "sessionId": "session-uuid",
  "actorType": "agent",
  "actorId": "agent-uuid",
  "eventType": "dangerous_action.detected",
  "severity": "important",
  "payload": {
    "actionType": "delete_file",
    "targetPath": "src/old_file.rs",
    "classificationSource": "acp"
  }
}
```

### `GET /projects/{projectId}/dangerous-actions`

Returns dangerous action records for the monitoring panel.

## 7. WebSocket Events

Recommended channel topics:

- `project.updated`
- `task.created`
- `task.updated`
- `task.available`
- `task.approval_requested`
- `task.acceptance_pending`
- `task.manual_review`
- `agent.presence_changed`
- `agent.auto_mode_changed`
- `dangerous_action.detected`
- `rollback.updated`

Example payload:

```json
{
  "schemaVersion": 1,
  "type": "task.available",
  "projectId": "project-uuid",
  "taskId": "task-uuid",
  "queueHint": {
    "assignmentMode": "public_queue"
  }
}
```

## 8. Local Core API

The Tauri front-end should call local commands for execution-sensitive operations.

Suggested commands:

- `list_local_agents`
- `list_provider_capabilities`
- `create_local_agent_session`
- `resume_local_agent_session`
- `bind_task_to_session`
- `start_task_run`
- `stop_task_run`
- `rollback_task_run`
- `set_agent_auto_mode`
- `send_acp_input`
- `interrupt_acp_session`
- `get_session_timeline`

### 8.1 Bootstrap Local Project Surface

During the `0.1.x` self-hosting bootstrap phase, the local desktop/server shell may also expose a small project-oriented surface before the full central API split is complete.

Minimal useful operations:

- register a project workspace root from a local absolute path
- scan the current primary workspace and persist a lightweight project summary
- create a project-scoped conversation that is not yet bound to a formal task
- continue a previously opened project-scoped conversation; if the in-memory runtime process is gone, reconnect the persisted provider thread and resume execution

This bootstrap surface is intentionally local-first:

- it helps users open a directory, inspect code/docs/assets, and ask clarifying questions before creating tasks
- it is not the final multi-user coordination API
- once the full desktop core and central service boundaries are established, these operations should be mapped into the provider-neutral local core API and persisted shared models

## 9. ACP Event Normalization

Suggested normalized event envelope:

```json
{
  "schemaVersion": 1,
  "sessionId": "session-uuid",
  "seq": 102,
  "timestamp": "2026-03-18T10:00:00Z",
  "kind": "tool_call",
  "name": "write_file",
  "status": "started",
  "payload": {}
}
```

Kinds may include:

- `message`
- `tool_call`
- `tool_result`
- `command`
- `command_output`
- `file_change`
- `approval_request`
- `status`
- `error`

## 10. Error Handling Conventions

Service errors should return:

```json
{
  "error": {
    "code": "TASK_NOT_ELIGIBLE",
    "message": "Task cannot be claimed in its current state",
    "details": {}
  }
}
```

Recommended codes:

- `TASK_NOT_ELIGIBLE`
- `APPROVAL_REQUIRED`
- `AGENT_OFFLINE`
- `WORKSPACE_NOT_ALLOWED`
- `ROLLBACK_NOT_ALLOWED`
- `SESSION_NOT_FOUND`
- `AUTO_MODE_DISABLED`

## 11. Compatibility Rules

Recommended platform rules before third-party integrations grow:

- version HTTP APIs explicitly
- version WebSocket and normalized session events explicitly
- add capability negotiation instead of assuming every client supports every panel/action
- make write operations idempotent where practical
- avoid breaking enum or status changes without a migration path
