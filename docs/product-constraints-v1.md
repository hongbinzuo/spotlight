# Product Constraints v1

## 1. Product Goal

Build a cross-platform desktop GUI tool for Windows, macOS, and future Linux support that enables:

- multiple users
- multiple projects
- multiple tasks per project
- multiple local Agents per project
- coordinated execution through a shared task system and a Zed-style Agent UI

The desktop application must:

- show a task list on the left
- show an Agent UI on the right
- connect the Agent UI to a local model CLI runtime, with Codex CLI as the first supported provider
- use ACP as the only Agent protocol surface
- support all ACP-visible operations in the UI
- allow shared project/task coordination through a central service

## 2. Product Shape

### 2.1 Left Panel: Task List

The task list is a shared, ordered queue scoped to the current project.

Users can:

- create a task from free-form text
- browse tasks in time order
- claim a task for execution
- request approval for a task
- approve a task
- assign or reassign a task to a specific Agent
- enable or disable Agent auto mode
- inspect task status, assignee, acceptance owner, retries, and audit trail

### 2.2 Right Panel: Agent UI

The Agent UI is inspired by Zed's assistant/workspace interaction model, but specialized for model CLI runtimes exposed through ACP.

The Agent UI must support:

- conversation timeline
- streaming responses
- ACP tool calls
- command execution output
- file operation summaries
- approval and confirmation requests
- session switching
- task binding
- resume and reopen of task sessions
- audit-relevant activity summaries

Provider strategy:

- Codex CLI is the first supported provider
- the architecture should support additional local or desktop-reachable model CLIs later
- future providers may include Kimi, Claude, MiniMax, or other tools
- the UI should not be tightly coupled to any one provider brand
- all provider-specific behavior should be normalized behind the ACP-facing runtime layer

## 3. Core Actors

- `Admin`
  - manages project visibility, membership, roles, and project-level policy
- `User`
  - creates tasks, joins projects, claims tasks, requests approval, accepts or rejects output if designated
- `Approver`
  - may approve a task before execution
- `Acceptor`
  - may accept or reject final output
- `Agent`
  - a local Codex CLI-backed execution unit running on a user's machine

Notes:

- A single human user may hold multiple roles.
- Project administrators, designated approvers, assigned task recipients, and eligible team leaders can approve tasks.
- Team leaders can approve tasks for team-visible projects.
- A single approval is sufficient in MVP.
- A single acceptance is sufficient in MVP.

## 4. Project Visibility and Membership

Users only see projects they are allowed to access.

Project visibility modes:

- `admin_only`
- `public`
- `team_scoped`
- `user_scoped`
- `mixed`

Rules:

- projects are private to admins by default
- admins can grant visibility to one or more teams
- admins can grant visibility to one or more users
- admins can make a project visible to all users
- a user may belong to many teams
- a project may be visible to many teams and many users

## 5. Workspace Model

Each project may bind multiple `workspace roots`.

Definitions:

- `primary workspace`
  - the main directory selected for a task run
- `attached workspace`
  - additional project directories attached to the current task context
- `external directory`
  - any directory not registered to the project
- `temp workspace`
  - approved writable temporary paths such as `/tmp` or `C:\tmp`

Rules:

- all project workspace roots are readable
- project workspace roots are writable unless an explicit policy marks them read-only
- external directories are readable
- external directories are not writable, deletable, or renameable by the Agent
- temp workspaces are writable for temporary execution artifacts
- a project may reference content from external read-only directories

## 6. Task Model

### 6.1 Task Creation

Users create a task by entering free-form text that describes a requirement or request.

Minimum task fields:

- title
- description
- project
- queue position
- creator
- requested acceptance owner or acceptance role if needed

### 6.2 Task Assignment Modes

Tasks support two assignment modes:

- `public_queue`
  - available for eligible Agents to pick
- `assigned_agent`
  - preferred for a specific Agent, with admin reassignment allowed

Automatic execution behavior:

- each Agent has an independent auto mode switch
- auto mode is enabled by default
- when notified of available work, an Agent first pulls tasks assigned to itself
- if none exist, the Agent pulls the oldest eligible task from the project's public queue
- if a specified Agent is offline for too long, an admin may reassign the task

### 6.3 Claim and Approval

Agreed behavior:

- a task can only have one active Agent at a time
- `claim` binds a task to an Agent for execution
- `approval` is a separate action from claim
- approval may be requested before execution

### 6.4 Automatic Mode

Automatic mode is event-driven:

- the central service emits task availability notifications via WebSocket
- the Agent wakes on relevant events and pulls work from the queue
- the central service does not directly push a task into execution
- on reconnect, an Agent whose auto mode was enabled resumes auto behavior

Retries:

- an automatically executed task may retry up to 3 times
- after the third failed retry, the task moves to manual handling

## 7. Task Lifecycle

High-level lifecycle states:

- `draft`
- `open`
- `claimed`
- `approval_requested`
- `approved`
- `auto_claimed`
- `running`
- `agent_done`
- `pending_acceptance`
- `accepted`
- `rejected`
- `failed`
- `manual_review`
- `rolled_back`
- `cancelled`

Behavior:

- `rejected` returns the task to `open`
- failed automatic execution retries up to 3 times before `manual_review`
- acceptance is a separate stage after Agent execution

## 8. Session Model

Task-to-session rules:

- one task has one `main task run`
- a task run may include multiple `sessions`
- sessions may be resumed
- sessions may be reopened
- all sessions within the same task run must remain linked in audit history

This design supports:

- interrupted local sessions
- long-running tasks split into phases
- recovery without losing task continuity

## 9. Acceptance Model

Acceptance is distinct from Agent completion.

Flow:

- the Agent finishes execution
- the task enters `pending_acceptance`
- the designated acceptor reviews outputs
- acceptance produces `accepted`
- rejection returns the task to `open`

Acceptance configuration:

- a project may define a default acceptance owner or acceptance role
- task creation may override the default acceptance owner
- single-person acceptance is sufficient in MVP

Potential acceptor roles include:

- developer
- product manager
- tester
- test manager
- engineering manager
- architect
- operations or business owner

## 10. Dangerous Action Policy

Dangerous actions do not require a second approval in MVP, but they must be strongly audited.

Dangerous action examples:

- deleting files
- overwriting files
- rewriting git history
- executing scripts

Non-dangerous examples for MVP:

- reading external directories
- installing dependencies
- network access

Policy:

- dangerous actions are logged with elevated audit severity
- the server stores and visualizes dangerous actions in a dedicated panel
- grouping is primarily by ACP action type, with command categorization as supplemental metadata
- once execution is allowed for the task run, actions are not blocked by a second approval step

## 11. Git Safety and Rollback

Before a new task begins execution:

- every writable Git repository that may be modified must be tagged
- the tag format is `task/<task_id>/pre-run/<timestamp>`
- repositories with uncommitted changes may still run
- dirty repository state must be recorded in audit data

Rollback policy:

- rollback is manually triggered
- rollback is used when a task failed or execution caused problems
- the current session can stop the running task and then trigger rollback
- rollback actions must be audited to the server
- rollback can be initiated by any authorized operator in MVP

## 12. Audit Requirements

The system must retain auditable records for:

- task creation
- task claim and auto-claim
- approval request and approval
- assignment and reassignment
- Agent auto mode changes
- Agent session creation, resume, reopen, stop
- ACP actions
- dangerous actions
- git tag creation
- rollback requests and results
- acceptance and rejection
- retry attempts

## 13. MVP Scope

Included in MVP:

- Tauri desktop application
- Rust local core
- React/TypeScript front-end
- Rust central sync service
- Postgres-backed project and task coordination
- WebSocket event streaming
- local Codex CLI integration over ACP
- task queue with approval, acceptance, retries, and rollback
- audit logs and dangerous action panel
- provider abstraction points for future CLI integrations

Explicitly deferred:

- multi-approval workflows
- conflict-free collaborative editing inside a single shared live session
- advanced policy scripting
- cloud-hosted remote Agent execution
- automatic rollback

## 14. Non-Goals for MVP

- replacing Git hosting platforms
- replacing issue tracking platforms at enterprise scale
- full endpoint security orchestration
- arbitrary remote code execution on behalf of other users
