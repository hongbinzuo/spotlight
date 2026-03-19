# Admin and Ops Console

## 1. Goal

The platform needs a dedicated control plane beyond the desktop and mobile clients.

This console is for:

- project configuration
- people and role management
- runtime and Agent visibility
- audit and risk review
- system monitoring
- billing and deployment administration

Without this layer, the product will feel powerful for end users but hard to operate at team and enterprise scale.

## 2. Primary Personas

- `Org Admin`
  - manages organization-level settings, users, teams, and policies
- `Project Admin`
  - manages project configuration, visibility, workspaces, default approvers, and acceptors
- `Ops / SRE`
  - monitors service health, Agent connectivity, queues, and failures
- `Security / Compliance`
  - reviews audit logs, dangerous actions, retention, and policy events
- `Billing Admin`
  - manages subscriptions, licenses, usage visibility, invoices, and deployment entitlements

## 3. Core Console Areas

### 3.1 Organization Settings

Should support:

- organization profile
- tenant settings
- auth and identity provider config
- default policy packs
- retention settings
- deployment metadata
- AI provider and budget policy defaults

### 3.2 People and Role Management

Should support:

- user directory
- team directory
- role and capability grants
- project memberships
- approver and acceptor assignment
- suspension and deactivation

Important design rule:

- manage capabilities, not only titles

### 3.3 Project Administration

Should support:

- create/edit/archive projects
- workspace root configuration
- visibility rules
- default workflow template
- default acceptance configuration
- allowed runtime targets and provider policies

### 3.4 Agent and Runtime Operations

Should support:

- Agent registry
- online/offline state
- auto mode visibility
- runtime target health
- session counts
- current running tasks
- manual disable or quarantine of an Agent/runtime

### 3.5 Monitoring and Operations

Should support:

- service health dashboard
- queue depth by project
- failed task trends
- retry spikes
- disconnected session counts
- notification delivery health
- storage usage and retention health

### 3.6 Audit and Risk Center

Should support:

- audit search
- dangerous action review
- rollback history
- policy decision history
- approval and acceptance history
- export for compliance review

### 3.7 Billing and Deployment

Should support:

- plan and subscription view
- usage meter view
- invoice view
- deployment mode
- license or entitlement state
- private deployment package metadata

### 3.8 AI Control Plane

Should support:

- configure one or more AI providers and keys
- select default models by analysis scenario
- define project or org-level spend budgets
- view cache hit rates and token usage
- enable or disable insight scenarios
- choose BYO key, vendor-managed key, or hybrid mode
- review generated analyses and forecast history

## 4. Recommended Product Shape

The admin console should be a separate web control plane, not embedded inside the desktop client.

Reasons:

- easier for admins who do not run local Agents
- better for centralized ops and compliance workflows
- cleaner permission boundaries
- easier to expose to enterprise buyers

Recommended first implementation:

- responsive web admin console
- server-rendered or SPA, whichever fits team preference
- backed by the same central API and event system

## 5. Key Screens

Recommended first screens:

- organization overview
- projects list
- project detail / settings
- users and teams
- agents and runtimes
- tasks ops dashboard
- audit and dangerous actions
- billing and deployment
- AI insights and model routing
- system health

## 6. Monitoring Requirements

Important monitoring signals:

- API latency and error rate
- WebSocket connection count and churn
- Agent heartbeat freshness
- task allocation latency
- run failure rate
- retry exhaustion rate
- rollback frequency
- dangerous action frequency
- push notification delivery status
- AI analysis latency, failure rate, cache rate, and token burn

## 7. Operational Controls

The console should expose safe control actions such as:

- disable auto mode for an Agent
- pause task allocation for a project
- quarantine a runtime target
- force session cleanup
- mark a project read-only
- adjust retention or audit export settings

These actions must be audited.

## 8. Deployment-Aware Behavior

The console should adapt to deployment mode:

- SaaS
  - full billing, org, and platform-health views
- managed private
  - tenant-specific operational visibility plus vendor support hooks
- self-hosted private
  - customer-admin focused controls, with optional vendor support overlay

## 9. Architecture Fit

The console should consume:

- normalized domain events
- audit records
- queue summaries
- runtime and Agent health summaries
- billing entitlements and meter summaries

It should not depend on desktop-only state or provider-native payloads.

## 10. Long-Term Value

This console is not just an admin afterthought.
It is one of the reasons enterprises will trust and pay for the platform.

The more the product evolves toward:

- many projects
- many teams
- many Agents
- private deployments
- compliance requirements

the more important this control plane becomes.
