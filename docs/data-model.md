# Data Model

## 1. Modeling Principles

- keep task state separate from run/session state
- keep approvals and acceptance as explicit first-class records
- model audit logs as append-only events
- allow a project to target both teams and individual users
- allow one task to accumulate many sessions over time
- avoid locking long-term platform evolution behind database `check` enums; prefer registries or versioned workflow definitions once the product moves beyond MVP

## 2. Core Entities

### 2.1 Identity and Membership

- `users`
- `teams`
- `team_members`
- `roles`
- `user_roles`

### 2.2 Project Scope

- `projects`
- `project_visibility_rules`
- `project_members`
- `project_workspaces`
- `project_acceptance_defaults`

### 2.3 Agent Scope

- `agents`
- `agent_presences`
- `agent_auto_modes`
- `agent_sessions`
- `agent_provider_configs`

### 2.4 Task Scope

- `tasks`
- `task_assignments`
- `task_approvals`
- `task_runs`
- `task_run_attempts`
- `task_acceptance`
- `task_rollbacks`

### 2.5 Audit Scope

- `audit_events`
- `dangerous_action_events`
- `git_tag_events`

## 3. Relational Schema

### 3.1 `users`

```sql
create table users (
  id uuid primary key,
  username text not null unique,
  display_name text not null,
  email text,
  is_active boolean not null default true,
  created_at timestamptz not null default now()
);
```

### 3.2 `teams`

```sql
create table teams (
  id uuid primary key,
  name text not null unique,
  description text,
  created_at timestamptz not null default now()
);
```

### 3.3 `team_members`

```sql
create table team_members (
  team_id uuid not null references teams(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  is_team_lead boolean not null default false,
  created_at timestamptz not null default now(),
  primary key (team_id, user_id)
);
```

### 3.4 `projects`

```sql
create table projects (
  id uuid primary key,
  key text not null unique,
  name text not null,
  description text,
  visibility_mode text not null check (
    visibility_mode in ('admin_only', 'public', 'team_scoped', 'user_scoped', 'mixed')
  ),
  created_by uuid not null references users(id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
```

### 3.5 `project_visibility_rules`

```sql
create table project_visibility_rules (
  id uuid primary key,
  project_id uuid not null references projects(id) on delete cascade,
  subject_type text not null check (subject_type in ('user', 'team', 'all_users')),
  subject_id uuid,
  can_view boolean not null default true,
  can_approve boolean not null default false,
  can_accept boolean not null default false,
  created_at timestamptz not null default now()
);
```

### 3.6 `project_workspaces`

```sql
create table project_workspaces (
  id uuid primary key,
  project_id uuid not null references projects(id) on delete cascade,
  label text not null,
  path text not null,
  is_primary_default boolean not null default false,
  is_writable boolean not null default true,
  created_at timestamptz not null default now()
);
```

### 3.7 `project_acceptance_defaults`

```sql
create table project_acceptance_defaults (
  project_id uuid primary key references projects(id) on delete cascade,
  acceptor_type text not null check (acceptor_type in ('user', 'team_role')),
  acceptor_user_id uuid references users(id),
  acceptor_role text,
  updated_at timestamptz not null default now()
);
```

### 3.8 `agents`

```sql
create table agents (
  id uuid primary key,
  owner_user_id uuid not null references users(id),
  project_id uuid not null references projects(id) on delete cascade,
  name text not null,
  provider_type text not null default 'codex' check (
    provider_type in ('codex', 'claude', 'kimi', 'minimax', 'custom')
  ),
  provider_mode text not null default 'native_acp' check (
    provider_mode in ('native_acp', 'adapted', 'text_only')
  ),
  assignment_mode text not null default 'public_and_direct' check (
    assignment_mode in ('public_only', 'direct_only', 'public_and_direct')
  ),
  created_at timestamptz not null default now(),
  unique (project_id, name)
);
```

### 3.8.1 `agent_provider_configs`

```sql
create table agent_provider_configs (
  agent_id uuid primary key references agents(id) on delete cascade,
  provider_version text,
  executable_path text,
  capabilities jsonb not null default '{}'::jsonb,
  settings jsonb not null default '{}'::jsonb,
  updated_at timestamptz not null default now()
);
```

### 3.9 `agent_presences`

```sql
create table agent_presences (
  agent_id uuid primary key references agents(id) on delete cascade,
  machine_id text not null,
  status text not null check (status in ('online', 'offline', 'busy', 'error')),
  last_heartbeat_at timestamptz not null,
  current_task_id uuid references tasks(id),
  updated_at timestamptz not null default now()
);
```

### 3.10 `agent_auto_modes`

```sql
create table agent_auto_modes (
  agent_id uuid primary key references agents(id) on delete cascade,
  enabled boolean not null default true,
  updated_by uuid not null references users(id),
  updated_at timestamptz not null default now()
);
```

### 3.11 `agent_sessions`

```sql
create table agent_sessions (
  id uuid primary key,
  agent_id uuid not null references agents(id) on delete cascade,
  task_run_id uuid,
  local_session_key text not null,
  state text not null check (
    state in ('new', 'connecting', 'attached', 'active', 'paused', 'disconnected', 'resuming', 'closed', 'errored')
  ),
  is_main boolean not null default false,
  cwd text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
```

### 3.12 `tasks`

```sql
create table tasks (
  id uuid primary key,
  project_id uuid not null references projects(id) on delete cascade,
  title text not null,
  description text not null,
  status text not null check (
    status in (
      'draft', 'open', 'claimed', 'approval_requested', 'approved', 'auto_claimed',
      'running', 'agent_done', 'pending_acceptance', 'accepted', 'rejected',
      'failed', 'manual_review', 'rolled_back', 'cancelled'
    )
  ),
  assignment_mode text not null check (assignment_mode in ('public_queue', 'assigned_agent')),
  requested_agent_id uuid references agents(id),
  active_agent_id uuid references agents(id),
  created_by uuid not null references users(id),
  approval_required boolean not null default false,
  primary_workspace_id uuid references project_workspaces(id),
  queue_order bigint not null,
  retry_count integer not null default 0,
  acceptance_owner_user_id uuid references users(id),
  acceptance_owner_role text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
```

### 3.13 `task_assignments`

```sql
create table task_assignments (
  id uuid primary key,
  task_id uuid not null references tasks(id) on delete cascade,
  assigned_agent_id uuid references agents(id),
  assignment_reason text not null,
  assigned_by uuid not null references users(id),
  created_at timestamptz not null default now()
);
```

### 3.14 `task_approvals`

```sql
create table task_approvals (
  id uuid primary key,
  task_id uuid not null references tasks(id) on delete cascade,
  status text not null check (status in ('requested', 'approved', 'denied', 'expired')),
  requested_by uuid not null references users(id),
  approved_by uuid references users(id),
  decided_at timestamptz,
  comment text,
  created_at timestamptz not null default now()
);
```

### 3.15 `task_runs`

```sql
create table task_runs (
  id uuid primary key,
  task_id uuid not null references tasks(id) on delete cascade,
  run_number integer not null,
  state text not null check (
    state in ('created', 'preflight', 'tagged', 'executing', 'interrupted', 'retry_wait', 'completed', 'failed', 'rolled_back', 'aborted')
  ),
  main_session_id uuid references agent_sessions(id),
  started_by_agent_id uuid references agents(id),
  started_at timestamptz,
  ended_at timestamptz,
  retry_budget integer not null default 3,
  retry_count integer not null default 0,
  primary_workspace_id uuid references project_workspaces(id),
  created_at timestamptz not null default now(),
  unique (task_id, run_number)
);
```

### 3.16 `task_run_attempts`

```sql
create table task_run_attempts (
  id uuid primary key,
  task_run_id uuid not null references task_runs(id) on delete cascade,
  attempt_number integer not null,
  outcome text check (outcome in ('running', 'completed', 'failed', 'interrupted')),
  error_summary text,
  started_at timestamptz not null default now(),
  ended_at timestamptz,
  unique (task_run_id, attempt_number)
);
```

### 3.17 `task_acceptance`

```sql
create table task_acceptance (
  id uuid primary key,
  task_id uuid not null references tasks(id) on delete cascade,
  status text not null check (status in ('not_started', 'pending', 'accepted', 'rejected')),
  requested_at timestamptz,
  decided_at timestamptz,
  decided_by uuid references users(id),
  comment text,
  created_at timestamptz not null default now()
);
```

### 3.18 `task_rollbacks`

```sql
create table task_rollbacks (
  id uuid primary key,
  task_run_id uuid not null references task_runs(id) on delete cascade,
  initiated_by uuid not null references users(id),
  reason text,
  status text not null check (status in ('requested', 'running', 'completed', 'failed')),
  started_at timestamptz not null default now(),
  ended_at timestamptz
);
```

### 3.19 `audit_events`

```sql
create table audit_events (
  id uuid primary key,
  project_id uuid not null references projects(id) on delete cascade,
  task_id uuid references tasks(id),
  task_run_id uuid references task_runs(id),
  session_id uuid references agent_sessions(id),
  actor_type text not null check (actor_type in ('user', 'agent', 'system')),
  actor_id uuid,
  event_type text not null,
  severity text not null check (severity in ('info', 'important', 'warning', 'critical')),
  payload jsonb not null,
  created_at timestamptz not null default now()
);
```

### 3.20 `dangerous_action_events`

```sql
create table dangerous_action_events (
  id uuid primary key,
  audit_event_id uuid not null references audit_events(id) on delete cascade,
  action_type text not null,
  command_text text,
  target_path text,
  classification_source text not null check (classification_source in ('acp', 'command', 'mixed')),
  created_at timestamptz not null default now()
);
```

### 3.21 `git_tag_events`

```sql
create table git_tag_events (
  id uuid primary key,
  task_run_id uuid not null references task_runs(id) on delete cascade,
  repo_root text not null,
  head_sha text not null,
  branch_name text,
  dirty boolean not null default false,
  tag_name text not null,
  created_at timestamptz not null default now()
);
```

## 4. Recommended Indexes

```sql
create index idx_tasks_project_queue on tasks(project_id, status, queue_order);
create index idx_tasks_requested_agent on tasks(requested_agent_id, status, queue_order);
create index idx_audit_events_project_created on audit_events(project_id, created_at desc);
create index idx_dangerous_actions_created on dangerous_action_events(created_at desc);
create index idx_agent_sessions_run on agent_sessions(task_run_id, created_at);
create index idx_task_runs_task on task_runs(task_id, run_number desc);
```

## 5. Queue Allocation Logic

Recommended selection order for `pull-next`:

1. oldest `open` or `approved` task assigned to the requesting Agent
2. oldest eligible `open` or `approved` task in `public_queue`

Atomic update requirement:

- the selection and state transition to `auto_claimed` must happen in one transaction

## 6. Audit Event Taxonomy

Suggested `event_type` values:

- `task.created`
- `task.claimed`
- `task.auto_claimed`
- `task.approval_requested`
- `task.approved`
- `task.started`
- `task.failed`
- `task.retried`
- `task.agent_done`
- `task.accepted`
- `task.rejected`
- `task.rolled_back`
- `agent.session_created`
- `agent.session_resumed`
- `agent.session_reopened`
- `agent.auto_mode_changed`
- `git.tag_created`
- `dangerous_action.detected`

## 7. Optional Future Tables

Potential later additions:

- `artifacts`
- `pull_request_drafts`
- `checklists`
- `task_dependencies`
- `acceptance_templates`
