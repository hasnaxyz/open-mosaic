# Open Mosaic Goals And Tasks

Open Mosaic can include goal and task context in its local control surface
without depending on a private task service. The portable file format is
`mosaic.goals.v1`; optional adapters can translate external systems into that
format.

## CLI

```sh
mosaic goals list
mosaic goals list --file goals.json --redact
mosaic goals validate --file goals.json
mosaic --dry-run goals todos-plan --project /work/repo --plan plan-id --redact
mosaic goals todos-plan --project /work/repo --plan plan-id > goals-envelope.json
mosaic dashboard --goals-file goals.json --redact
```

`goals list` reads `$XDG_CONFIG_HOME/open-mosaic/goals.json`, or
`~/.config/open-mosaic/goals.json`, when the file exists. If the default file
is absent, the command returns an empty registry with `configured: false`.
Passing `--file` makes the file explicit; invalid JSON or invalid schema data
is reported as a machine-readable Mosaic error.

`--redact` replaces task/goal titles, descriptions, links, and local paths with
`[redacted]` in command output. It does not mutate the registry file.

## Registry Shape

```json
{
  "schema_version": "mosaic.goals.v1",
  "source": {
    "kind": "file",
    "adapter": "generic",
    "configured": true,
    "project_path": "/work/open-mosaic"
  },
  "goals": [
    {
      "id": "plan-1",
      "title": "Open Mosaic workspace",
      "description": "Build the agentic workspace surface",
      "status": "active",
      "priority": "high"
    }
  ],
  "tasks": [
    {
      "id": "task-1",
      "goal_id": "plan-1",
      "title": "Add goal context",
      "description": "Expose portable local task context",
      "status": "in_progress",
      "priority": "high",
      "agent": "cli",
      "blocked": false,
      "tags": ["goals", "adapters"]
    }
  ]
}
```

Required fields are:

- `schema_version`: must be `mosaic.goals.v1`
- `goals`: array of goal objects
- `tasks`: array of task objects
- each goal: `id`, `title`, `status`
- each task: `id`, `title`, `status`

Optional fields include `description`, `priority`, `goal_id`, `agent`,
`blocked`, `tags`, `references`, `source`, and `metadata`. Unknown fields are
reserved for compatible additions; consumers should ignore fields they do not
understand.

## Optional todos Adapter

`mosaic goals todos-plan` is an explicit adapter command. It runs:

```sh
todos --project <path> --json plans --show <plan-id>
```

and normalizes the returned plan/tasks into `mosaic.goals.v1` inside a
`goals.todos_plan` Mosaic envelope. Open Mosaic does not call `todos`
automatically, does not require it to be installed, and does not require Hasna
infrastructure for sessions, panes, prompts, queues, observations, audit, or
dashboard snapshots.

Use top-level `--dry-run` to inspect the argv without running `todos`:

```sh
mosaic --dry-run goals todos-plan --project /work/repo --plan plan-id --redact
```

The subprocess is executed with an argument vector, not a local shell command.
Audit records redact the project path in the command plan and store counts and
status, not task bodies.

## Dashboard

`mosaic dashboard` includes a `goals` section. By default it checks the local
goals config file and reports `goals.status: "not_configured"` if the file is
missing. Use `--goals-file` to point at a specific registry.

If a goals file is invalid, the dashboard still returns queue, audit, session,
and live summaries with `partial: true` and an error entry whose section is
`goals`.
