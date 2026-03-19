# Extensibility and Compatibility

## 1. Why This Matters

This product has the shape of a platform, not just a single desktop app.

If it works, future pressure will come from:

- more model providers
- more runtime targets
- more workflow types
- more enterprise auth and compliance demands
- more ecosystem integrations

The main risk is not that the MVP is too small.
The real risk is that early assumptions become invisible platform constraints.

## 2. What Is Still Too Hardcoded

### 2.1 Database `check` Enums

Current schemas hardcode many values directly in table definitions:

- provider types
- provider modes
- visibility modes
- task statuses
- assignment modes
- acceptance types
- audit severities

Why this is risky:

- every new workflow or provider can require a database migration
- external extensions cannot add new values safely
- custom enterprise deployments may want organization-specific states

Recommendation:

- keep the current enums for MVP if needed
- move long-term to registry tables or workflow definitions
- allow custom values to be introduced through metadata instead of schema surgery

### 2.2 Task Workflow Is Fixed

The current task lifecycle is strong for MVP, but it is still a closed workflow.

Future pressure may require:

- sub-tasks
- task dependencies
- custom approval chains
- per-project workflow templates
- blocked or waiting states
- handoff states between specialized Agents

Recommendation:

- treat the current lifecycle as the default workflow template
- add `workflow_template_id` and `task_type` once the product starts supporting verticals
- keep audit events workflow-neutral

### 2.3 Git Is Assumed as the Only Safety Backbone

Right now rollback and pre-run safety depend on Git tags.

Why this is limiting:

- some workspaces may not be Git repositories
- some enterprise setups may use different VCS or generated environments
- some tasks may operate on docs, data, or design artifacts outside Git

Recommendation:

- introduce a `repository_adapter` or `snapshot_adapter` abstraction later
- support strategies like `git_tag`, `git_stash_snapshot`, `archive_snapshot`, `none`

### 2.4 Runtime Target Is Still Implicitly Local

The docs are much better now, but the product model still assumes:

- local desktop
- local provider process
- local filesystem

Future targets may include:

- containerized agents
- SSH-attached agents
- remote workers
- ephemeral cloud sandboxes

Recommendation:

- separate `agent identity` from `runtime location`
- add runtime target types such as `local`, `remote`, `container`, `ssh`

### 2.5 Approval and Acceptance Are Single-Step

This is good for MVP, but it will not hold forever.

Future needs may include:

- multiple approvers
- ordered approvals
- quorum approvals
- role-based acceptance templates
- conditional acceptance by task type

Recommendation:

- keep the MVP UX simple
- model approvals and acceptance as workflow instances, not permanent one-row assumptions

### 2.6 Artifact Handling Is Still Thin

Today artifacts are mostly implied through logs, summaries, and audit payloads.

Future needs may include:

- screenshots
- preview builds
- reports
- patch bundles
- benchmark outputs
- pull request links

Recommendation:

- promote artifacts to first-class entities before ecosystem integrations grow
- allow pluggable artifact storage backends

### 2.7 Roles Are Described, Not Fully Modeled

Current docs mention:

- admin
- approver
- acceptor
- team leader
- developer
- product manager
- tester

Why this is limiting:

- organizations will have custom role taxonomies
- one company may map acceptance to QA, another to PM, another to security review

Recommendation:

- separate `system capabilities` from `human job titles`
- store permission grants independently from display roles

### 2.8 Event Transport Is Too Narrow

Current coordination is based on HTTP + WebSocket.

That is fine for MVP, but future scale may need:

- SSE
- gRPC
- internal event bus
- message broker integration

Recommendation:

- define internal domain events first
- let HTTP/WebSocket be one delivery layer, not the domain model

### 2.9 ACP Is the UI Surface, but Provider Protocols Will Vary

Keeping ACP as the UI-facing protocol is a good decision.
The risk is assuming every provider can cleanly map to ACP.

Recommendation:

- keep ACP as the desktop UI contract
- do not assume ACP is the provider-native contract
- support degraded capability modes without breaking the product model

### 2.10 Auth and Org Model Are Still Small

The current design covers users, teams, and project visibility well for MVP.

Still missing for long-term expansion:

- organizations or tenants
- external identity providers
- SSO
- SCIM provisioning
- service accounts
- API tokens

Recommendation:

- add an `organizations` layer before enterprise rollout
- ensure every key entity can be tenant-scoped

## 3. Compatibility Principles

These should become platform rules, not just suggestions.

### 3.1 Version Everything

Version:

- HTTP API
- WebSocket event schemas
- normalized session event schemas
- provider capability contracts
- workflow templates

### 3.2 Prefer Registries Over Closed Enums

For long-lived platform concepts, prefer:

- registry tables
- metadata-driven catalogs
- capability flags

Instead of:

- hardcoded enum migrations for every new concept

### 3.3 Separate Intent From Execution

Keep these layers distinct:

- task intent
- policy evaluation
- runtime execution
- audit recording

This makes it easier to swap providers, sandboxes, and transport layers later.

### 3.4 Make Every Client Feature Capability-Gated

Do not assume:

- every provider supports tool calls
- every runtime supports rollback
- every environment supports session resume

Instead:

- negotiate capabilities
- degrade gracefully
- expose unsupported features clearly

### 3.5 Keep Audit and Workflow Provider-Neutral

The platform should answer:

- who did what
- where
- when
- under which policy

without caring whether the runtime was Codex, Claude, Kimi, or MiniMax.

## 4. Recommended Next Refactors

These are the highest-value future-proofing moves.

1. Replace long-term enum assumptions with registries and workflow templates.
2. Add `organization` and `tenant` scoping before enterprise adoption.
3. Introduce `runtime_target` abstraction for local, remote, container, and SSH execution.
4. Add first-class artifact entities and storage abstraction.
5. Add event schema versioning and capability negotiation everywhere.
6. Add repository/snapshot abstraction so rollback is not Git-only forever.

## 5. Recommendation for This Product

If you want this product to become a genuinely category-leading platform, the right mental model is:

- not "a GUI for one CLI"
- not "a task queue with a chat panel"
- but "a collaborative AI execution operating system"

That means the stable core should be:

- workflow engine
- runtime abstraction
- provider abstraction
- audit and policy kernel
- pluggable UI surfaces

Everything vendor-specific should sit at the edges.
