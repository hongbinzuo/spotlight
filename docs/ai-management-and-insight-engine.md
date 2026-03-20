# AI Management and Insight Engine

## 1. Goal

The platform should support a configurable AI management layer that helps teams analyze and manage projects more intelligently.

This layer is different from execution Agents.

- execution Agents do the work
- management AI interprets the work, the signals, and the project state

The system should support switching among different models and providers with minimal configuration friction.

## 2. Core Design Principles

### 2.1 Multi-Model by Default

The insight engine should support:

- different providers
- different models for different scenarios
- different models at org, project, or scenario scope

Examples:

- small model for classification
- medium model for summaries
- large model for milestone and cost analysis

### 2.2 Low-Friction Configuration

Admin setup should be simple:

1. add provider
2. enter key or endpoint
3. validate connection
4. choose default model pack
5. optionally override by scenario

The common case should work in a few clicks.

### 2.3 Token Efficiency First

The management AI should not consume raw logs by default.

Preferred pipeline:

1. collect raw signals
2. summarize into structured facts
3. route to the smallest sufficient model
4. cache the result
5. escalate to a larger model only when needed

## 3. Architecture

The insight engine should sit behind the admin control plane and the central service.

Main responsibilities:

- provider routing
- scenario-to-model mapping
- prompt templating
- budget enforcement
- caching
- result persistence
- traceability and audit

## 4. AI Provider Flexibility

The management AI layer should support:

- OpenAI-compatible endpoints
- Anthropic-compatible endpoints
- local model gateways
- enterprise internal gateways
- future custom adapters

Configuration model:

- one organization can configure many providers
- one project can inherit org defaults
- one scenario can override provider and model

## 5. Recommended Admin UX

### 5.1 Provider Setup

Each provider config should allow:

- provider name
- endpoint
- API key or secret reference
- available models
- model capabilities
- default timeout
- max budget
- region or deployment tag

### 5.2 Scenario Setup

Each insight scenario should allow:

- enabled or disabled
- model selection
- trigger mode
- input window size
- max tokens
- cache TTL
- escalation model

### 5.3 Quick Start Presets

To avoid tedious setup, ship presets like:

- `Lean`
  - low cost, mostly small models
- `Balanced`
  - default mixed routing
- `Deep Analysis`
  - larger models for complex scenarios
- `Private / BYO`
  - optimized for customer-managed endpoints

## 6. Best Scenarios

### 6.1 Project Daily and Weekly Summaries

Input:

- task deltas
- build and deploy deltas
- risk events
- milestone changes

Output:

- concise status summary
- blockers
- recommended next actions

Low-token strategy:

- pre-aggregate changes before calling AI

### 6.2 Build and Deploy Failure Explanation

Input:

- normalized failure signature
- top errors
- affected tasks or repos

Output:

- likely reason
- impact
- recommended next checks

Low-token strategy:

- only send top errors and metadata, not entire logs

### 6.3 Acceptance Assistance

Input:

- task description
- output summary
- changed files summary
- test summary
- risk summary

Output:

- checklist for acceptor
- likely uncovered areas
- review recommendations

### 6.4 Delivery ETA and Milestone Risk Forecast

Input:

- current open tasks
- remaining effort estimate
- completion velocity
- historical cycle time
- failure and retry rates
- acceptance queue wait time
- people load
- build and deploy stability

Output:

- likely delivery date range
- risk score
- likely delay factors
- confidence range

### 6.5 Cost and Capacity Analysis

Input:

- effort entries
- task volume
- platform usage
- active Agents

Output:

- cost projection
- overloaded areas
- likely staffing pressure

### 6.6 Token Efficiency Analysis

Input:

- insight run history
- scenario-level token usage
- cache hit and miss deltas
- escalation frequency
- repeated low-value prompts

Output:

- recommended model downgrades or upgrades
- cache and TTL tuning suggestions
- reusable prompt and summary patterns
- projected token savings

## 7. Token-Saving Strategies

These rules should be mandatory.

### 7.1 Rules Before LLM

Use deterministic logic first for:

- threshold crossing
- repeated failures
- queue backlog spikes
- retry exhaustion
- milestone date comparisons

Call AI only to explain or prioritize.

### 7.2 Structured Inputs Only

Default to:

- counts
- deltas
- top-k errors
- summaries
- typed signal payloads

Avoid:

- giant raw logs
- full chat transcripts
- full diffs unless explicitly needed

### 7.3 Scenario Routing

Suggested routing:

- small model
  - classification, grouping, triage
- medium model
  - daily summaries, explainers
- large model
  - milestone risk, cost projection, deep management analysis

### 7.4 Caching

Cache should be keyed by:

- scenario
- project
- time window
- normalized input hash
- provider/model version

If the same project data has not changed, reuse the previous result.

### 7.5 Incremental Windows

Prefer:

- "changes since last report"
- "last 24 hours"
- "last 7 days"

Instead of:

- full-history reanalysis

### 7.6 Escalation Path

Good pattern:

1. cheap model or rules
2. confidence check
3. larger model only for uncertain or high-value cases

## 8. Insight Scenarios as First-Class Config

Scenarios should be explicit system objects.

Recommended built-in scenarios:

- `daily_summary`
- `weekly_summary`
- `build_failure_explainer`
- `deploy_failure_explainer`
- `acceptance_assistant`
- `delivery_eta_forecast`
- `milestone_risk_forecast`
- `capacity_analysis`
- `cost_projection`
- `token_efficiency_advisor`
- `task_deduplication`
- `project_health_explainer`

Each scenario should define:

- input contract
- trigger mode
- model routing
- budget policy
- cache policy
- output schema

## 9. Trigger Modes

Support three trigger modes:

- `manual`
- `scheduled`
- `event_driven`

Examples:

- daily summary: scheduled
- build failure explainer: event-driven
- delivery ETA forecast: scheduled or manual
- token efficiency advisor: scheduled or manual
- milestone risk review: scheduled or manual

## 10. Result Quality and Safety

Insight results should include:

- provider
- model
- scenario
- generated time
- input window
- confidence
- supporting signals
- result summary

Important rule:

- forecasts and interpretations must be labeled as analysis, not source truth

## 11. Budget and Governance

Every org and project should have:

- monthly AI budget
- scenario-level token ceilings
- provider allowlist
- model allowlist
- external-data policy

The system should allow:

- hard stop when budget is exceeded
- degrade to cheaper models
- queue non-urgent insight jobs

## 12. Data Model Recommendations

Recommended future entities:

- `ai_providers`
- `ai_provider_credentials`
- `ai_model_catalog`
- `ai_scenarios`
- `ai_scenario_bindings`
- `ai_insight_runs`
- `ai_insight_cache`
- `ai_budgets`

Key rule:

- keep execution-provider config and management-AI config related but separate

Execution Agents and management AI have different workloads and economics.

## 13. Billing Fit

AI management can be packaged as:

- basic summaries included in Team
- advanced forecasts in Enterprise
- usage-based overages for heavy analysis

Good billable units:

- insight runs
- input tokens
- output tokens
- premium forecast scenarios

## 14. Best Initial Rollout

I recommend shipping in this order:

1. daily summary
2. build failure explainer
3. acceptance assistant
4. delivery ETA forecast
5. token efficiency advisor
6. project health explainer
7. milestone risk forecast

This gives strong value quickly without exploding token cost.

## 15. Product Value

This layer helps the platform move from:

- task execution

to:

- execution plus understanding

That is a meaningful product jump, especially for:

- project managers
- engineering managers
- technical leads
- operations teams
- enterprise admins
