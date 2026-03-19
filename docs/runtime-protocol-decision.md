# Runtime Protocol Decision

## 1. Decision Summary

For the first real long-session implementation, Spotlight should not use a generic external ACP standard as the direct runtime control protocol for Codex.

Instead, Spotlight should:

- use `codex app-server` as the native long-session runtime for Codex
- expose a normalized ACP-like event surface to the UI and platform core
- reserve a future interoperability layer for standard ACP adapters

This is a deliberate compatibility decision, not a shortcut.

## 2. Why Not Use Standard ACP as the First Runtime Layer

The reason is not that ACP is unimportant.
The reason is that the first implementation needs the richest possible session control for Codex.

Current product requirements include:

- local long-running sessions
- thread creation and resume
- turn start
- turn interrupt
- user supplemental prompt injection
- turn resume or steer
- streaming runtime events
- approval visibility
- tool and command activity visibility

These are runtime-control requirements, not only cross-agent interoperability requirements.

A generic standard ACP layer is valuable for multi-vendor interoperability, but it may force the product to collapse down to the lowest common denominator too early.

That would create avoidable risk in the most important MVP experience:

- pause
- add prompt
- resume
- preserve the same task session context

## 3. Why `codex app-server` Is the Right First Integration

`codex app-server` already exposes the long-session primitives Spotlight needs for a real runtime bridge.

The observed protocol shape is a JSON-RPC style evented session protocol with methods such as:

- `initialize`
- `thread/start`
- `thread/resume`
- `turn/start`
- `turn/interrupt`
- `turn/steer`

It also emits lifecycle and streaming notifications such as:

- `thread/started`
- `turn/started`
- `turn/completed`
- token usage updates
- message deltas
- command execution output deltas

That makes it a stronger fit for Codex-native deep control than a prematurely generalized protocol boundary.

## 4. Product Positioning

Spotlight should distinguish three layers clearly.

### 4.1 Native Provider Runtime Layer

This layer speaks the provider's strongest native protocol.

Examples:

- `codex app-server`
- future Claude-native runtime adapter
- future Kimi-native runtime adapter
- future MiniMax-native runtime adapter

### 4.2 Spotlight Runtime Kernel

This layer owns normalized product semantics:

- task-bound sessions
- thread identity
- turn lifecycle
- pause and resume
- supplemental user steering
- audit events
- dangerous action classification
- artifacts
- acceptance flow

This is the stable product core.

### 4.3 Interoperability Layer

This layer is where standard ACP support should live in the future.

It can serve two purposes:

- ingest third-party agents into Spotlight
- expose Spotlight-managed agents to external ecosystems

This means ACP remains strategically important, but not necessarily the correct first internal runtime transport for Codex.

## 5. Architectural Rule

The correct design rule is:

- use native provider runtime protocols when they expose materially richer execution semantics
- normalize those semantics into Spotlight runtime events and capabilities
- add ACP adapters above or beside that boundary for ecosystem compatibility

In other words:

- do not make the UI provider-specific
- do not make the runtime kernel Codex-specific
- do not force the provider runtime layer to use ACP when a better native protocol exists

## 6. Tradeoff Analysis

### 6.1 Benefits of the Chosen Direction

- faster delivery of a real Codex long-session runtime
- better support for pause, steer, resume, and session continuity
- richer runtime telemetry for audit and UX
- less risk of flattening provider-specific high-value capabilities
- cleaner path to a production-quality Codex experience

### 6.2 Costs of the Chosen Direction

- the first provider implementation is not fully protocol-uniform at the transport layer
- ACP support becomes a second-step interoperability feature rather than the first runtime dependency
- adapter design discipline becomes more important to avoid leaking native protocol details upward

### 6.3 Why the Tradeoff Is Acceptable

The first product risk is not lack of standards purity.
The first product risk is failing to ship a compelling real runtime.

A weak first runtime would damage:

- user trust
- self-bootstrap usefulness
- multi-agent collaboration credibility
- acceptance of the platform architecture

So the first milestone should optimize for actual runtime quality, while keeping the abstraction seams clean.

## 7. Required Guardrails

To keep this decision future-safe, Spotlight must not hardcode Codex behavior above the provider adapter boundary.

Required guardrails:

- define a provider-neutral runtime session model
- define normalized event types for UI and audit
- record provider-native payloads only as supplemental metadata
- keep capability negotiation explicit
- ensure task and workflow state never depend on one provider's raw event names

## 8. Future ACP Strategy

ACP should still be part of the long-term roadmap.

Recommended order:

1. ship Codex with its native runtime bridge
2. prove the normalized runtime kernel in production-like usage
3. add ACP adapter support for external interoperability
4. evaluate whether some future providers can use ACP natively without losing critical functionality

## 9. Final Rule for Spotlight v0.02+

For Spotlight `v0.02` and the first real agent runtime slice:

- `codex app-server` is the native runtime transport for Codex
- Spotlight exposes an ACP-like normalized event surface internally
- standard ACP is a future interoperability layer, not the mandatory first runtime dependency

This is the preferred design because it maximizes real execution capability now without sacrificing future extensibility.
