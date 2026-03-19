# Platform Abstractions v1

## 1. Goal

This document defines the stable kernel abstractions that should survive:

- new model vendors
- new runtime environments
- new workflow types
- new enterprise requirements
- new UI surfaces

If these abstractions stay clean, the product can grow without repeated rewrites.

## 2. Stable Core

The long-term platform core should be:

- workflow engine
- runtime abstraction
- provider abstraction
- insight engine abstraction
- policy kernel
- snapshot and rollback abstraction
- artifact abstraction
- event and audit backbone

Everything else should plug into those layers.

## 3. Workflow Engine

### 3.1 Responsibility

The workflow engine decides:

- current state
- allowed transitions
- who can perform which actions
- what side effects happen after transitions

### 3.2 Must Not Depend On

It must not depend on:

- one specific provider
- one specific UI
- one specific transport
- one specific storage backend

### 3.3 Core Types

- `WorkflowTemplate`
- `WorkflowInstance`
- `WorkflowState`
- `WorkflowTransition`
- `WorkflowAction`

## 4. Runtime Abstraction

### 4.1 Responsibility

A runtime is where work actually executes.

Possible runtimes:

- local desktop
- remote host
- ssh-attached environment
- container
- ephemeral sandbox

### 4.2 Core Types

- `RuntimeTarget`
- `RuntimeSession`
- `RuntimePolicy`
- `RuntimeCapabilities`

### 4.3 Key Rule

Agent identity and runtime location must remain separate.

That prevents the platform from assuming:

- every Agent is local
- every Agent is tied to one machine
- every Agent has filesystem access in the same way

## 5. Provider Abstraction

### 5.1 Responsibility

A provider is the model-facing execution adapter.

Examples:

- Codex
- Claude
- Kimi
- MiniMax

### 5.2 Core Types

- `ProviderAdapter`
- `ProviderSession`
- `ProviderCapabilities`
- `ProviderEvent`

### 5.3 Key Rule

Provider-specific behavior should terminate at the adapter boundary.
Everything above that line should remain provider-neutral.

## 6. Policy Kernel

### 6.1 Responsibility

The policy kernel evaluates:

- access control
- filesystem scope
- dangerous action classification
- approval requirements
- rollback authorization

### 6.2 Core Types

- `Principal`
- `Capability`
- `PolicyDecision`
- `PolicyContext`
- `PolicyEffect`

### 6.3 Key Rule

Permissions should be capability-driven, not title-driven.

This avoids encoding one company's org chart into the product core.

## 6.5 Insight Engine Abstraction

### 6.5.1 Responsibility

The insight engine turns structured project signals into summaries, forecasts, warnings, and management recommendations.

Typical outputs:

- daily or weekly summaries
- build or deploy incident explanations
- milestone risk forecasts
- cost or capacity projections
- acceptance assistance

### 6.5.2 Core Types

- `InsightScenario`
- `InsightRun`
- `InsightProvider`
- `InsightBudget`
- `InsightCache`
- `InsightResult`

### 6.5.3 Key Rule

The insight layer should consume normalized project signals and task data, not raw ad hoc logs by default.
It must remain provider-neutral and budget-aware.

## 7. Snapshot and Rollback Abstraction

### 7.1 Responsibility

Before risky execution, the platform needs a recoverable point.

This may be implemented through:

- Git tags
- repository snapshots
- archive snapshots
- filesystem clones

### 7.2 Core Types

- `SnapshotStrategy`
- `Snapshot`
- `SnapshotObject`
- `RollbackOperation`

### 7.3 Key Rule

Git is the first implementation, not the abstraction itself.

## 8. Artifact Abstraction

### 8.1 Responsibility

Artifacts are the durable outputs of work.

Examples:

- reports
- logs
- patches
- screenshots
- preview links
- bundles

### 8.2 Core Types

- `Artifact`
- `ArtifactBackend`
- `ArtifactReference`
- `ArtifactMetadata`

### 8.3 Key Rule

Acceptance and audit should refer to artifacts by stable references, not only embedded text blobs.

## 9. Event and Audit Backbone

### 9.1 Responsibility

The event layer explains what happened across the system.

It should power:

- UI updates
- audit trails
- analytics
- integration hooks
- replay and debugging

### 9.2 Core Types

- `DomainEvent`
- `AuditEvent`
- `RiskEvent`
- `EventSchema`

### 9.3 Key Rule

Internal domain events should come first.
HTTP, WebSocket, and future transports should be delivery mechanisms layered on top.

## 10. UI Surface Abstraction

### 10.1 Responsibility

The current UI is a desktop app, but future UI surfaces may include:

- web admin console
- ops dashboard
- mobile monitoring view
- embedded IDE panel

### 10.2 Key Rule

The UI should consume normalized events, capabilities, workflow state, and artifacts.
It should not reconstruct business truth from provider-specific output text.

## 11. Practical Design Rules

These rules should guide future implementation reviews.

1. Never couple business state directly to one provider's event format.
2. Never make a new product feature depend on local-only execution assumptions if a runtime abstraction can hold it.
3. Never encode a new enterprise role as a hardcoded enum when a capability grant can express it.
4. Never make rollback exclusively Git-shaped in the service layer.
5. Never expose a UI action without checking runtime and provider capabilities first.
6. Never let audit semantics drift by provider.

## 12. Recommended Near-Term Refactors

If you want the architecture to stay ambitious and durable, prioritize:

1. `Principal + Capability` authorization model
2. `WorkflowTemplate + WorkflowInstance` engine
3. `RuntimeTarget` abstraction
4. `SnapshotStrategy` abstraction
5. `Artifact` first-class storage
6. event schema registry and versioning
7. insight engine contracts for scenario routing, budgeting, and caching

## 13. Product North Star

The durable version of this product is best understood as:

- a collaborative AI execution platform
- with pluggable runtimes
- pluggable model providers
- auditable workflow orchestration
- and multiple user-facing surfaces

That framing is what preserves long-term compatibility without flattening the ambition.
