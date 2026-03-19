# Billing and Deployment Model

## 1. Goal

This product should support three commercial shapes at the same time:

- cloud SaaS subscription
- managed private deployment
- self-hosted private deployment

The pricing model should reflect the platform's real value:

- workflow orchestration
- multi-agent collaboration
- audit and governance
- provider abstraction
- runtime control

It should not depend only on reselling model tokens.

## 2. Core Packaging Principle

The healthiest long-term packaging is:

- charge a platform subscription for collaboration and governance
- optionally charge usage for premium execution and storage features
- support BYO provider credentials, bundled model access, or a hybrid model

This protects margin and keeps the business from being trapped by upstream model vendors.

## 3. Deployment Modes

### 3.1 Cloud SaaS

Best for:

- startups
- individual builders
- fast onboarding
- teams that do not need private infrastructure

Commercial shape:

- monthly or annual subscription
- self-serve plans
- optional overages for premium usage

### 3.2 Managed Private Deployment

Best for:

- enterprises
- regulated teams
- customers needing dedicated infrastructure or region control

Commercial shape:

- annual contract
- setup or deployment fee
- support/SLA fee
- optional storage or premium-connector overages

### 3.3 Self-Hosted Private Deployment

Best for:

- air-gapped or high-compliance environments
- organizations with strong internal platform teams

Commercial shape:

- annual license or subscription
- seat band or org minimum
- optional paid support and upgrade channel

## 4. Recommended Pricing Structure

I recommend a hybrid model:

1. base subscription
2. included usage
3. overages or premium add-ons

This is easier to sell than pure usage pricing and fairer than pure seat pricing for automation-heavy customers.

## 5. What to Charge For

### 5.1 Base Subscription

Charge for platform value:

- seats
- projects and shared workflows
- approvals and acceptance
- audit and governance
- provider integrations
- mobile monitoring access

Good primary anchors:

- per seat
- per active organization
- per active Agent pool

### 5.2 Usage-Based Add-Ons

Charge for scarce or premium features:

- hosted runtime minutes
- artifact storage
- long audit retention
- premium connectors
- advanced analytics
- AI insight packs and forecast scenarios
- dedicated environments

### 5.3 Enterprise Add-Ons

Charge separately for:

- SSO / SCIM
- advanced compliance
- dedicated VPC or region pinning
- private deployment package
- premium support / SLA
- migration and onboarding services

## 6. Recommended Pricing Models

### 6.1 Seat-Based

Pros:

- easy to explain
- predictable
- familiar to buyers

Cons:

- can underprice heavy automation use

### 6.2 Active-Agent Based

Pros:

- aligns with the multi-agent thesis
- closer to orchestration value

Cons:

- less intuitive for some customers

### 6.3 Usage-Based

Pros:

- scales with real consumption
- works well for bursty workloads

Cons:

- less predictable
- harder to budget

### 6.4 Recommended Mix

For this product, my recommendation is:

- `Pro / Team SaaS`
  - per seat, with included Agents and task-run quota
- `Enterprise Cloud`
  - annual minimum contract plus premium features
- `Private Deployment`
  - annual platform license plus deployment/support fee

## 7. Model-Cost Strategy

Support three modes:

### 7.1 BYO Provider

Customer brings their own provider credentials or local CLI runtime.

Best for:

- cost-sensitive teams
- enterprise customers with existing provider contracts

### 7.2 Bundled Provider

You include model access in some plans.

Best for:

- trials
- easy onboarding

Risk:

- lower or more volatile margin

### 7.3 Hybrid

Base plan is BYO, with optional hosted credits.

This is the strongest long-term default.

## 8. Recommended Meters

Meters should be product-shaped, not only infra-shaped.

Recommended first meters:

- `active_seat`
- `active_agent`
- `task_run`
- `runtime_minute`
- `artifact_storage_gb`
- `audit_retention_gb_month`
- `premium_connector`
- `mobile_push_alert`
- `ai_insight_run`
- `ai_input_token`
- `ai_output_token`
- `ai_cached_result`

Rule:

- every billable meter should be derivable from append-only events or runtime telemetry
- do not rely on opaque client-only counters

## 9. Plan Packaging Suggestion

### 9.1 Free / Trial

- limited seats
- limited projects
- limited task runs
- basic audit
- local providers only
- mobile read-only monitoring

### 9.2 Pro

- individual power users
- more projects
- more task runs
- multiple local Agents
- mobile monitoring and alerts

### 9.3 Team

- team seats
- approvals and acceptance
- shared queue controls
- richer audit retention
- role and policy features
- basic AI management summaries

### 9.4 Enterprise Cloud

- SSO / SCIM
- advanced audit
- retention upgrades
- policy controls
- optional dedicated environment
- mobile operational alerts
- advanced AI forecasting and project intelligence

### 9.5 Private Deployment

- self-hosted or managed private
- contract pricing
- optional offline entitlement package
- enterprise support

## 10. Technical Requirements for Billing

To support these models, the platform needs:

- organization-level billing accounts
- plan catalog
- subscriptions
- usage meter registry
- append-only meter events
- invoices
- entitlement checks

Important rule:

- billing must be a separate bounded context from workflow execution

## 11. Strategic Recommendation

Do not sell this as "access to a model" or "tokens in a GUI".

Sell it as:

- collaborative AI execution platform
- workflow and governance layer
- multi-agent operating system for engineering and operations teams

That gives you stronger pricing power because the customer is paying for:

- control
- traceability
- coordination
- deployment flexibility
- enterprise operability

## 12. Recommended Starting Model

Practical first model:

1. launch SaaS first
2. price by seat with included active Agents and included task-run volume
3. support BYO provider from the start
4. sell enterprise cloud and private deployment as contract offerings
5. charge extra for storage, retention, and premium connectors
6. package advanced AI insight scenarios as higher-tier or usage-based features
