# Mobile Companion Strategy

## 1. Goal

The mobile app is a companion surface for visibility and lightweight control.

Primary mobile outcomes:

- view project task lists
- inspect task status
- view Agent run state
- receive alerts when an Agent needs attention
- perform limited safe actions while away from desktop

The mobile app should not attempt to replace the full desktop Agent workspace in the first phase.

## 2. Core Use Cases

### 2.1 Task Visibility

Users should be able to:

- switch among visible projects
- view task queues
- filter by status
- open a task detail view
- see approver and acceptance state

### 2.2 Agent Monitoring

Users should be able to:

- see whether an Agent is online
- see whether it is idle, running, waiting, failed, or needs review
- see current task, current run, and last update time
- view recent dangerous actions and rollback state summaries

### 2.3 Lightweight Operational Actions

Good mobile actions for MVP+:

- approve a task
- accept or reject a finished task
- pause or stop a running task
- disable Agent auto mode
- trigger rollback after explicit confirmation

Not recommended for initial mobile scope:

- full ACP chat composer
- full file diff review
- complex workspace editing
- multi-pane desktop-equivalent debugging

## 3. Product Positioning

The mobile app should be treated like an operational cockpit, not a miniature IDE.

That means the design emphasis should be:

- task and Agent status
- alerts
- approvals
- quick interventions

## 4. Recommended MVP Scope

I recommend the first mobile release support:

- project list
- project task list
- task detail
- Agent list
- Agent status detail
- push notifications for approval-needed, failed, and manual-review events

This gives immediate value without overcommitting to a difficult small-screen ACP experience.

## 5. Mobile Navigation

Suggested top-level tabs:

- Projects
- Tasks
- Agents
- Alerts
- Me

Suggested task detail sections:

- summary
- state timeline
- assignment
- approval
- acceptance
- recent run status

Suggested Agent detail sections:

- current task
- status
- auto mode
- last heartbeat
- recent events

## 6. API Requirements

The mobile client should rely on server-normalized APIs, not raw provider streams.

Needed APIs:

- project summaries
- mobile task list summaries
- task detail summaries
- Agent status summaries
- alert feeds
- safe action endpoints for approve, stop, disable auto mode

## 7. Push Notification Model

Push notifications are a major source of mobile value.

Important notification types:

- approval requested
- acceptance pending
- task failed
- task moved to manual review
- dangerous action detected
- rollback completed or failed

## 8. Security Model

Mobile permissions should be narrower than desktop by default.

Suggested default posture:

- read-mostly
- explicit confirmation for stop/rollback
- no destructive workspace-level operations
- stronger session expiration and device auth

## 9. Relationship to Desktop

Desktop remains the primary execution surface.
Mobile is the remote observation and intervention surface.

This is similar to the value seen in the open-source `happy` project, which presents mobile and web access for Codex and Claude usage and emphasizes checking session progress away from the desk, device switching, and alerts. Source: [slopus/happy](https://github.com/slopus/happy)

For this product, the right interpretation is:

- keep mobile strong for monitoring and lightweight control
- keep desktop strong for full ACP execution and deep task handling

## 10. Technical Recommendation

Recommended stack options:

- React Native / Expo for fastest cross-platform mobile iteration
- or Flutter if the team later wants stronger custom rendering control

Given the current desktop direction, React Native is likely the faster organizational fit for a mobile companion.

## 11. Long-Term Direction

Later mobile expansions could include:

- richer run logs
- artifact previews
- voice summaries
- approval bundles
- device handoff into a web session

But the first win is simple:

- "Can I see what my Agents and tasks are doing when I am away from my computer?"
