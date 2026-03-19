# Data Model v2

## 1. Purpose

This document evolves the MVP schema into a platform-oriented model.

Goals:

- reduce hardcoded enum pressure
- support multiple organizations and tenants
- support multiple runtime targets
- support workflow templates instead of one fixed state machine
- support provider-neutral capabilities
- make artifacts and snapshots first-class

The intent is not to replace the MVP immediately.
The intent is to provide the next stable model once the product begins to scale.

## 2. Core Modeling Shifts from v1

### 2.1 From Closed Enums to Registries

Where v1 hardcodes values in SQL `check` constraints, v2 prefers:

- registry tables
- template tables
- metadata payloads
- explicit schema versions

Examples:

- task statuses become workflow state definitions
- provider types become provider registrations
- runtime types become runtime target registrations
- audit severity remains normalized, but event kinds become catalog-driven

### 2.2 From Single-Product Scope to Platform Scope

New platform concerns:

- organizations
- principals
- role grants
- runtime targets
- provider capabilities
- snapshot strategies
- artifacts
- workflow templates

### 2.3 From Static Task Flow to Workflow Instances

Tasks should remain business-visible objects.
State progression should move into workflow instances so different task types can evolve without schema churn.

## 3. High-Level Domains

### 3.1 Identity and Tenant Domain

- `organizations`
- `users`
- `teams`
- `principals`
- `capabilities`
- `principal_capability_grants`

### 3.2 Project Domain

- `projects`
- `project_memberships`
- `project_visibility_bindings`
- `project_workspaces`
- `project_policies`

### 3.3 Runtime Domain

- `runtime_target_types`
- `runtime_targets`
- `agent_providers`
- `provider_capability_catalog`
- `agents`
- `agent_runtime_bindings`
- `agent_sessions`

### 3.4 Workflow Domain

- `workflow_templates`
- `workflow_states`
- `workflow_transitions`
- `workflow_instances`
- `workflow_instance_states`
- `workflow_actions`

### 3.5 Work Domain

- `tasks`
- `task_assignments`
- `task_runs`
- `task_attempts`
- `task_acceptance_reviews`
- `task_approval_reviews`

### 3.6 Artifact and Snapshot Domain

- `artifact_backends`
- `artifacts`
- `snapshot_strategies`
- `snapshots`
- `snapshot_objects`
- `rollback_operations`

### 3.7 Audit and Event Domain

- `event_schemas`
- `domain_events`
- `audit_events`
- `risk_events`

## 4. Core Registries

## 4.1 `organizations`

```sql
create table organizations (
  id uuid primary key,
  slug text not null unique,
  name text not null,
  status text not null default 'active',
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

## 4.2 `principals`

Principals unify users, teams, service accounts, and future automation actors.

```sql
create table principals (
  id uuid primary key,
  organization_id uuid not null references organizations(id) on delete cascade,
  principal_type text not null,
  external_ref text,
  display_name text not null,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

Examples of `principal_type`:

- `user`
- `team`
- `service_account`
- `automation`

## 4.3 `capabilities`

Capabilities should model permission atoms, not job titles.

```sql
create table capabilities (
  id uuid primary key,
  key text not null unique,
  description text,
  created_at timestamptz not null default now()
);
```

Examples:

- `project.view`
- `task.create`
- `task.approve`
- `task.accept`
- `task.rollback`
- `agent.manage`

## 4.4 `principal_capability_grants`

```sql
create table principal_capability_grants (
  id uuid primary key,
  organization_id uuid not null references organizations(id) on delete cascade,
  principal_id uuid not null references principals(id) on delete cascade,
  resource_type text not null,
  resource_id uuid,
  capability_id uuid not null references capabilities(id),
  effect text not null default 'allow',
  conditions jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

This replaces overloading roles and visibility flags.

## 5. Project and Workspace Model

## 5.1 `projects`

```sql
create table projects (
  id uuid primary key,
  organization_id uuid not null references organizations(id) on delete cascade,
  key text not null,
  name text not null,
  description text,
  metadata jsonb not null default '{}'::jsonb,
  created_by_principal_id uuid references principals(id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  unique (organization_id, key)
);
```

## 5.2 `project_visibility_bindings`

```sql
create table project_visibility_bindings (
  id uuid primary key,
  project_id uuid not null references projects(id) on delete cascade,
  principal_id uuid references principals(id),
  binding_type text not null,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

Examples of `binding_type`:

- `public`
- `member`
- `viewer`
- `team_scope`
- `explicit_user`

## 5.3 `project_workspaces`

```sql
create table project_workspaces (
  id uuid primary key,
  project_id uuid not null references projects(id) on delete cascade,
  label text not null,
  path text not null,
  workspace_kind text not null default 'project_root',
  access_policy jsonb not null default '{}'::jsonb,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

`access_policy` can express:

- writable
- read_only
- temp_writable
- external_read_only

## 6. Runtime and Provider Model

## 6.1 `runtime_target_types`

```sql
create table runtime_target_types (
  id uuid primary key,
  key text not null unique,
  description text,
  created_at timestamptz not null default now()
);
```

Seed examples:

- `local_desktop`
- `remote_host`
- `ssh_host`
- `container`
- `ephemeral_sandbox`

## 6.2 `runtime_targets`

```sql
create table runtime_targets (
  id uuid primary key,
  organization_id uuid not null references organizations(id) on delete cascade,
  runtime_target_type_id uuid not null references runtime_target_types(id),
  owner_principal_id uuid references principals(id),
  label text not null,
  connectivity jsonb not null default '{}'::jsonb,
  policy jsonb not null default '{}'::jsonb,
  status jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

This separates Agent identity from where it runs.

## 6.3 `agent_providers`

```sql
create table agent_providers (
  id uuid primary key,
  key text not null unique,
  display_name text not null,
  adapter_mode text not null,
  versioning_strategy text,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

Examples:

- `codex`
- `claude`
- `kimi`
- `minimax`
- `custom`

## 6.4 `provider_capability_catalog`

```sql
create table provider_capability_catalog (
  id uuid primary key,
  provider_id uuid not null references agent_providers(id) on delete cascade,
  capability_key text not null,
  version_range text,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  unique (provider_id, capability_key, version_range)
);
```

## 6.5 `agents`

```sql
create table agents (
  id uuid primary key,
  project_id uuid not null references projects(id) on delete cascade,
  owner_principal_id uuid references principals(id),
  provider_id uuid not null references agent_providers(id),
  runtime_target_id uuid not null references runtime_targets(id),
  name text not null,
  desired_mode jsonb not null default '{}'::jsonb,
  actual_capabilities jsonb not null default '{}'::jsonb,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

## 7. Workflow Model

## 7.1 `workflow_templates`

```sql
create table workflow_templates (
  id uuid primary key,
  organization_id uuid references organizations(id),
  key text not null,
  display_name text not null,
  version integer not null,
  template_kind text not null,
  definition jsonb not null,
  created_at timestamptz not null default now(),
  unique (organization_id, key, version)
);
```

Template kinds:

- `task_lifecycle`
- `approval_flow`
- `acceptance_flow`

## 7.2 `workflow_states`

```sql
create table workflow_states (
  id uuid primary key,
  workflow_template_id uuid not null references workflow_templates(id) on delete cascade,
  state_key text not null,
  display_name text not null,
  state_class text not null,
  metadata jsonb not null default '{}'::jsonb,
  unique (workflow_template_id, state_key)
);
```

Examples of `state_class`:

- `start`
- `active`
- `waiting`
- `terminal_success`
- `terminal_failure`

## 7.3 `workflow_transitions`

```sql
create table workflow_transitions (
  id uuid primary key,
  workflow_template_id uuid not null references workflow_templates(id) on delete cascade,
  from_state_id uuid not null references workflow_states(id) on delete cascade,
  to_state_id uuid not null references workflow_states(id) on delete cascade,
  transition_key text not null,
  guard_definition jsonb not null default '{}'::jsonb,
  effect_definition jsonb not null default '{}'::jsonb
);
```

## 7.4 `workflow_instances`

```sql
create table workflow_instances (
  id uuid primary key,
  workflow_template_id uuid not null references workflow_templates(id),
  resource_type text not null,
  resource_id uuid not null,
  current_state_id uuid references workflow_states(id),
  context jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
```

This is the key table that removes task status from hardcoded schema.

## 7.5 `workflow_actions`

```sql
create table workflow_actions (
  id uuid primary key,
  workflow_instance_id uuid not null references workflow_instances(id) on delete cascade,
  action_key text not null,
  actor_principal_id uuid references principals(id),
  input jsonb not null default '{}'::jsonb,
  result jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

## 8. Task and Run Model

## 8.1 `tasks`

```sql
create table tasks (
  id uuid primary key,
  project_id uuid not null references projects(id) on delete cascade,
  workflow_instance_id uuid not null references workflow_instances(id),
  task_type text not null default 'general',
  title text not null,
  description text not null,
  queue_key text not null default 'default',
  priority integer not null default 100,
  requested_by_principal_id uuid references principals(id),
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
```

Important change:

- current state is derived from workflow, not duplicated as a hardcoded task enum

## 8.2 `task_assignments`

```sql
create table task_assignments (
  id uuid primary key,
  task_id uuid not null references tasks(id) on delete cascade,
  assignment_kind text not null,
  target_agent_id uuid references agents(id),
  target_principal_id uuid references principals(id),
  queue_scope jsonb not null default '{}'::jsonb,
  status jsonb not null default '{}'::jsonb,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

Assignment kinds:

- `public_queue`
- `direct_agent`
- `team_queue`
- `runtime_pool`

## 8.3 `task_runs`

```sql
create table task_runs (
  id uuid primary key,
  task_id uuid not null references tasks(id) on delete cascade,
  agent_id uuid references agents(id),
  primary_workspace_id uuid references project_workspaces(id),
  snapshot_id uuid references snapshots(id),
  run_policy jsonb not null default '{}'::jsonb,
  state jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  started_at timestamptz,
  ended_at timestamptz
);
```

Keep run state in structured json or a smaller registry, not another giant enum list.

## 8.4 `task_attempts`

```sql
create table task_attempts (
  id uuid primary key,
  task_run_id uuid not null references task_runs(id) on delete cascade,
  attempt_number integer not null,
  status jsonb not null default '{}'::jsonb,
  summary jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  ended_at timestamptz,
  unique (task_run_id, attempt_number)
);
```

## 8.5 `task_approval_reviews`

```sql
create table task_approval_reviews (
  id uuid primary key,
  task_id uuid not null references tasks(id) on delete cascade,
  workflow_instance_id uuid references workflow_instances(id),
  reviewer_principal_id uuid references principals(id),
  decision_key text not null,
  payload jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

## 8.6 `task_acceptance_reviews`

```sql
create table task_acceptance_reviews (
  id uuid primary key,
  task_id uuid not null references tasks(id) on delete cascade,
  workflow_instance_id uuid references workflow_instances(id),
  reviewer_principal_id uuid references principals(id),
  decision_key text not null,
  payload jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

## 9. Snapshot and Rollback Model

## 9.1 `snapshot_strategies`

```sql
create table snapshot_strategies (
  id uuid primary key,
  key text not null unique,
  description text,
  metadata jsonb not null default '{}'::jsonb
);
```

Examples:

- `git_tag`
- `git_stash_snapshot`
- `archive_snapshot`
- `filesystem_clone`
- `none`

## 9.2 `snapshots`

```sql
create table snapshots (
  id uuid primary key,
  strategy_id uuid not null references snapshot_strategies(id),
  project_id uuid not null references projects(id) on delete cascade,
  task_run_id uuid,
  status jsonb not null default '{}'::jsonb,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

## 9.3 `snapshot_objects`

```sql
create table snapshot_objects (
  id uuid primary key,
  snapshot_id uuid not null references snapshots(id) on delete cascade,
  object_type text not null,
  object_ref text not null,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

For Git this can store:

- repo root
- HEAD SHA
- tag name
- dirty state

## 9.4 `rollback_operations`

```sql
create table rollback_operations (
  id uuid primary key,
  task_run_id uuid not null references task_runs(id) on delete cascade,
  snapshot_id uuid not null references snapshots(id),
  initiated_by_principal_id uuid references principals(id),
  status jsonb not null default '{}'::jsonb,
  result jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  completed_at timestamptz
);
```

## 10. Artifact Model

## 10.1 `artifact_backends`

```sql
create table artifact_backends (
  id uuid primary key,
  key text not null unique,
  display_name text not null,
  config jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

Examples:

- `db`
- `filesystem`
- `s3`
- `blob_store`

## 10.2 `artifacts`

```sql
create table artifacts (
  id uuid primary key,
  task_run_id uuid references task_runs(id) on delete cascade,
  backend_id uuid references artifact_backends(id),
  artifact_type text not null,
  title text not null,
  uri text,
  content_hash text,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

Artifact types are intentionally not locked to a tiny enum.

## 11. Event and Audit Model

## 11.1 `event_schemas`

```sql
create table event_schemas (
  id uuid primary key,
  schema_key text not null,
  version integer not null,
  definition jsonb not null,
  created_at timestamptz not null default now(),
  unique (schema_key, version)
);
```

## 11.2 `domain_events`

```sql
create table domain_events (
  id uuid primary key,
  organization_id uuid not null references organizations(id) on delete cascade,
  schema_id uuid not null references event_schemas(id),
  topic text not null,
  resource_type text not null,
  resource_id uuid not null,
  payload jsonb not null,
  created_at timestamptz not null default now()
);
```

## 11.3 `audit_events`

```sql
create table audit_events (
  id uuid primary key,
  organization_id uuid not null references organizations(id) on delete cascade,
  project_id uuid references projects(id),
  task_id uuid references tasks(id),
  task_run_id uuid references task_runs(id),
  session_id uuid references agent_sessions(id),
  actor_principal_id uuid references principals(id),
  event_key text not null,
  severity text not null,
  payload jsonb not null,
  created_at timestamptz not null default now()
);
```

Keep severity normalized, but allow event keys to grow without schema churn.

## 12. Migration Strategy from v1

Recommended order:

1. introduce `organizations` and backfill all current records into one default organization
2. introduce `principals` and map existing users/teams into it
3. add `agent_providers` and `runtime_targets` while keeping current agent tables alive
4. add `workflow_templates` and `workflow_instances`
5. dual-write current task status into workflow state during migration
6. add `snapshots` and `artifacts`
7. deprecate hardcoded enum-heavy columns once UI and services are fully moved

## 13. What v1 Should Keep

Not everything should change immediately.

Keep in v1/MVP:

- clear task queue semantics
- explicit approval and acceptance behavior
- strong audit requirements
- git-based safety for code projects
- simple Agent auto mode

v2 is about protecting the future, not sacrificing the current clarity.
