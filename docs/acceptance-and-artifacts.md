# Acceptance and Artifacts

## 1. Goal

A task should not be considered truly complete just because the Agent stops talking or exits successfully.

The system needs a consistent delivery package so that an acceptor can decide:

- what changed
- what was attempted
- what risks were introduced
- whether the task meets the requested outcome

## 2. Acceptance Contract

The canonical completion path is:

```text
running -> agent_done -> pending_acceptance -> accepted | rejected
```

Definitions:

- `agent_done`
  - the Agent has finished its execution for the current run
- `pending_acceptance`
  - a human acceptor is reviewing the output package
- `accepted`
  - the task is complete for MVP
- `rejected`
  - the output package did not satisfy expectations and the task returns to `open`

## 3. Minimum Output Package

Every completed task run should produce a structured output package, even if some fields are empty.

Required sections:

- `summary`
  - short human-readable explanation of what the Agent did
- `changes`
  - files touched, commands executed, and major actions
- `artifacts`
  - links or references to generated files, logs, reports, or patches
- `risks`
  - warnings, dangerous actions, dirty repo notes, or known limitations
- `verification`
  - what checks the Agent ran and the result
- `rollback_reference`
  - the pre-run git tag or rollback target

## 4. Artifact Types

MVP should support these artifact categories:

- `message`
  - plain explanation or conclusion
- `patch`
  - code diff or file modification set
- `file`
  - generated document, config, or output file
- `log`
  - command output, test log, or run transcript
- `report`
  - structured summary for product, QA, or engineering review
- `rollback_ref`
  - git tag or repository rollback pointer

## 5. Acceptance Roles

Acceptance can be performed by different kinds of roles depending on the task:

- developer
- product manager
- tester
- test manager
- engineering manager
- architect
- business or operations owner

Project-level defaults:

- a project may define a default acceptor
- a project may define a default acceptor role

Task-level override:

- each task may override the project default

## 6. Acceptance Checklist

Recommended acceptance questions:

1. Did the Agent address the requested requirement?
2. Are the produced changes understandable and reviewable?
3. Are verification results included?
4. Are risks and limitations clearly disclosed?
5. Are rollback points available if needed?

If any critical answer is no, the acceptor should reject the task.

## 7. Rejection Behavior

When a task is rejected:

- the task returns to `open`
- prior sessions and runs remain visible
- the previous output package remains attached for comparison
- the next Agent run should be able to see the rejection comment

Minimum rejection fields:

- rejecting user
- rejection time
- rejection comment
- rejected run reference

## 8. UI Requirements for Acceptance

The acceptance view should show:

- task description
- Agent summary
- diff or file change list
- test/check results
- dangerous action summary
- git tag / rollback reference
- acceptance comment box
- `Accept` and `Reject` actions

## 9. Data Modeling Suggestion

If artifacts need explicit persistence in MVP+, add:

```sql
create table task_artifacts (
  id uuid primary key,
  task_run_id uuid not null references task_runs(id) on delete cascade,
  artifact_type text not null,
  title text not null,
  uri text,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);
```

For MVP, artifact references may live inside:

- `audit_events.payload`
- `task_acceptance.comment`
- task run summary records

## 10. Recommendation for MVP

To keep implementation tractable:

- require every task run to emit a completion summary
- require every task run to emit a verification summary
- require every task run to emit rollback references
- show these three sections directly in `pending_acceptance`

This gives acceptors enough information without building a full artifact repository on day one.
