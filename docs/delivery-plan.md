# Delivery Plan

## 1. MVP Objective

Deliver a usable cross-platform desktop application that:

- lets users collaborate on project-scoped task queues
- connects local Agents to Codex CLI over ACP
- supports approval, automatic execution, retries, acceptance, rollback, and audit

## 2. Suggested Implementation Order

### Phase 1: Foundation

- initialize `Tauri 2 + React + TypeScript + Rust`
- initialize central `Axum + Postgres + WebSocket` service
- implement auth placeholder and local user identity
- implement projects, visibility rules, and workspaces
- define provider abstraction and ship `Codex` as the first adapter

### Phase 2: Task Queue

- implement task CRUD
- implement queue ordering
- implement claim and assignment flow
- implement approval flow
- implement acceptance owner configuration

### Phase 3: Local Agent Runtime

- integrate local Codex CLI process management
- build ACP adapter and normalized event stream
- create Agent sessions and session resume/reopen
- bind tasks to task runs and sessions

### Phase 4: Auto Execution

- implement Agent presence and heartbeat
- implement Agent auto mode state
- implement WebSocket `task.available`
- implement atomic `pull-next`
- implement retry policy to 3 attempts

### Phase 5: Safety and Audit

- implement workspace guard
- implement dangerous action classification
- implement pre-run git tagging
- implement rollback
- implement dangerous action monitoring panel

### Phase 6: UX Refinement

- polish Zed-inspired layout
- add acceptance and manual review queue views
- improve timeline rendering for ACP actions
- improve error handling and reconnect flow

## 3. Thin Vertical Slice

The first end-to-end slice should prove:

1. create a project
2. add a workspace
3. create a task
4. claim the task
5. open local Agent session
6. send task as prompt to Codex CLI over ACP
7. complete task run
8. move task to pending acceptance
9. accept the task
10. inspect audit records

## 4. Engineering Milestones

### Milestone A

- task list visible
- Agent pane visible
- local Codex CLI session works

### Milestone B

- project/task persistence works
- approval and acceptance states work
- session linking works

### Milestone C

- auto mode works
- retries work
- audit pipeline works

### Milestone D

- git tagging works
- rollback works
- dangerous action panel works

## 5. Testing Strategy

Required test layers:

- unit tests for state transitions
- integration tests for `pull-next` atomic allocation
- integration tests for approval and acceptance flows
- integration tests for retry limit logic
- integration tests for git tag and rollback metadata
- desktop smoke tests for session resume/reopen

## 6. Risks

Main implementation risks:

- ACP capability mapping may differ from assumptions
- cross-platform filesystem policy enforcement is tricky
- git rollback UX can be confusing in dirty repositories
- Codex CLI lifecycle management may need careful recovery logic
- future provider CLIs may not expose enough structured events for a full ACP-equivalent UI

## 7. Recommended Repository Layout

```text
apps/
  desktop/
  server/
crates/
  acp-adapter/
  task-domain/
  audit-domain/
  git-safety/
docs/
```

## 8. Definition of Done for MVP

The MVP is done when:

- a user can create and process tasks across projects they can see
- a local Agent can execute a task via Codex CLI over ACP
- auto mode can pick queued tasks
- approval and acceptance are visible and enforced
- retries stop after 3 failures and route to manual review
- dangerous actions are audited and visible
- task runs create pre-run git tags
- rollback can be triggered manually and is audited
