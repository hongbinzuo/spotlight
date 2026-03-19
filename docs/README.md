# Multi-Agent Collaboration Desktop

This folder contains the product and technical design documents for a cross-platform desktop app that combines a shared task queue with a Zed-style Agent workspace backed by local Codex CLI sessions over ACP.

## Document Index

- `product-constraints-v1.md`
  - Product goals, actors, permissions, task lifecycle, and MVP boundaries.
- `state-machine.md`
  - Canonical task, run, approval, acceptance, and rollback state models.
- `system-architecture.md`
  - End-to-end architecture for Tauri desktop, Rust services, ACP integration, and central sync service.
- `data-model.md`
  - Core entities, relational schema, indexes, and audit/event storage.
- `data-model-v2.md`
  - Platform-oriented data model that reduces hardcoded enums and adds workflow, runtime, tenant, and artifact abstractions.
- `api-design.md`
  - Service APIs, WebSocket events, and local desktop-to-core interfaces.
- `ui-information-architecture.md`
  - Desktop page layout, major views, interaction flows, and Zed-inspired Agent panel behavior.
- `acceptance-and-artifacts.md`
  - Output package, acceptance contract, and what a completed task must hand back for review.
- `provider-abstraction.md`
  - How to support multiple local model CLIs such as Codex, Claude, Kimi, and MiniMax without changing the product model.
- `runtime-protocol-decision.md`
  - Why Spotlight should use the native Codex app-server runtime first, while reserving standard ACP for future interoperability.
- `extensibility-and-compatibility.md`
  - Remaining hardcoded areas, long-term extension points, and compatibility principles for a category-defining product.
- `platform-abstractions-v1.md`
  - Stable long-term kernel abstractions for workflow, runtime, provider, policy, snapshot, artifacts, and events.
- `billing-and-deployment-model.md`
  - SaaS, managed private deployment, self-hosted, and recommended subscription plus usage-based charging strategy.
- `mobile-companion-strategy.md`
  - Mobile app scope for project/task visibility and Agent status monitoring, with optional lightweight approval and interruption actions.
- `admin-and-ops-console.md`
  - Back-office control plane for project settings, people and role management, monitoring, risk operations, and platform maintenance.
- `ai-management-and-insight-engine.md`
  - Multi-model AI control plane for management analysis, forecasting, summaries, and low-token project intelligence.
- `security-and-audit.md`
  - Workspace boundaries, dangerous action policy, git tag policy, rollback policy, and audit requirements.
- `delivery-plan.md`
  - MVP slices, milestones, and implementation order.

## Recommended Reading Order

1. `product-constraints-v1.md`
2. `state-machine.md`
3. `system-architecture.md`
4. `data-model.md`
5. `data-model-v2.md`
6. `platform-abstractions-v1.md`
7. `api-design.md`
8. `ui-information-architecture.md`
9. `acceptance-and-artifacts.md`
10. `provider-abstraction.md`
11. `runtime-protocol-decision.md`
12. `extensibility-and-compatibility.md`
13. `billing-and-deployment-model.md`
14. `mobile-companion-strategy.md`
15. `admin-and-ops-console.md`
16. `ai-management-and-insight-engine.md`
17. `security-and-audit.md`
18. `delivery-plan.md`
