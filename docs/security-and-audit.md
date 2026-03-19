# Security and Audit

## 1. Security Goal

The system is designed for collaborative local execution with strong traceability rather than hard zero-trust sandboxing.

The security model must:

- limit write scope to project workspaces and approved temp directories
- permit read-only access to external directories
- strongly audit dangerous actions
- preserve rollback points through git tags

## 2. Filesystem Policy

### 2.1 Writable Locations

Writable by Agent:

- project workspace roots marked writable
- temp directories such as `/tmp` and `C:\tmp`

### 2.2 Read-Only Locations

Readable but not writable by Agent:

- all directories outside project workspaces

Forbidden actions in external directories:

- create
- modify
- overwrite
- delete
- rename

## 3. Dangerous Action Classification

Dangerous actions for MVP:

- file deletion
- file overwrite
- git history rewrite
- script execution

Not dangerous by default in MVP:

- reading external directories
- installing dependencies
- network access

## 4. Interception Strategy

Recommended strategy:

- classify at ACP action level first
- augment with command-level categorization where useful

Rationale:

- ACP actions are closer to platform-independent intent
- command strings differ heavily across Windows and macOS
- audit visualization is easier with normalized action categories

## 5. Audit Severity Rules

Recommended severity mapping:

- `info`
  - normal reads, non-destructive navigation, status updates
- `important`
  - file writes, dependency installs, session reopen, retry events
- `warning`
  - script execution, overwrite operations, cross-workspace writes
- `critical`
  - delete operations, git history rewrite, rollback execution

## 6. Git Tag Safety

Before execution:

- enumerate writable repositories in active project workspaces
- create a pre-run tag for each repository
- record current HEAD, branch, dirty state, and tag result

Standard tag shape:

- `task/<task_id>/pre-run/<timestamp>`

If repository is dirty:

- do not block execution
- persist `dirty=true` in tag audit
- surface dirty status in the UI

## 7. Rollback Policy

Rollback is manual in MVP.

Rules:

- user stops the current run or chooses rollback from failure/manual-review flow
- rollback targets the relevant pre-run tag
- rollback is available to authorized operators
- rollback always emits audit records to the server

Recommended UI support:

- list of repositories affected
- corresponding pre-run tags
- HEAD before rollback
- rollback result per repository

## 8. Audit Storage Model

Audit storage should be append-only.

Required fields:

- project
- task
- run
- session
- actor
- event type
- severity
- timestamp
- structured payload

Audit events should never be silently rewritten.

## 9. Dangerous Action Monitoring Panel

The server must support a visual panel for dangerous operations.

Recommended filters:

- project
- task
- action type
- severity
- actor
- time range

Recommended columns:

- timestamp
- project
- task
- Agent
- action type
- target path
- command summary
- session link

## 10. Presence and Auto Mode Audit

The system must record:

- Agent online/offline transitions
- auto mode enabled/disabled changes
- task allocation results
- reconnect and auto-resume events

This supports operational debugging and accountability.

## 11. Abuse and Failure Scenarios

The design should explicitly cover:

- Agent goes offline after task allocation
- local session disconnects mid-run
- rollback fails in one repository but succeeds in another
- assigned Agent remains offline
- task gets rejected after a long run
- dangerous action occurs in a dirty repository

## 12. Future Hardening Options

Not required for MVP, but worth planning for:

- signed audit event forwarding
- stricter allowlists for external reads
- configurable dangerous action catalogs
- secret redaction in command output
- policy-as-code execution guards
