# Mobile Placeholder

This directory will host the mobile companion app for task visibility, agent status, and alerts.

The current minimum system starts with server-side APIs that a future mobile client can consume.

Current recommended summary entry:

- `GET /api/v1/projects/{project_id}/summary`

This summary endpoint is intended to provide a compact project snapshot for mobile surfaces, admin overviews, and future autonomy loops without depending on desktop-local state.
