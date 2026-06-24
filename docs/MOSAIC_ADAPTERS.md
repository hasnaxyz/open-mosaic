# Open Mosaic Adapter Manifests

Open Mosaic adapter manifests describe optional integrations without making
the core product depend on private services. The manifest format is JSON and
uses `schema_version: "mosaic.adapter.v1"`.

Manifests are data. `mosaic adapters validate` reads and validates a manifest
but does not execute `command`.

## CLI

```sh
mosaic adapters list
mosaic adapters list --kind agent
mosaic adapters validate --file adapter.json
```

`adapters list` returns built-in portable interface descriptors for these
kinds:

- `agent`
- `project_registry`
- `task_system`
- `identity`
- `machine_registry`
- `transport`

Built-in descriptors use `source: "builtin"` and `mode: "interface"`. They
document the stable capability names Open Mosaic understands; they do not
require Hasna, open-* services, or a specific machine.

## Manifest Shape

```json
{
  "schema_version": "mosaic.adapter.v1",
  "id": "example.agent",
  "kind": "agent",
  "name": "Example Agent",
  "version": "1.0.0",
  "description": "Optional agent adapter",
  "capabilities": ["pane.detect", "prompt.send", "observe.pane"],
  "command": ["example-agent", "--stdio"]
}
```

Required fields:

- `schema_version`: must be `mosaic.adapter.v1`
- `id`: ASCII letters, numbers, dots, underscores, and hyphens only
- `kind`: one of the supported adapter kinds
- `version`: adapter manifest or adapter implementation version

Optional fields:

- `name`: human-readable adapter name
- `description`: short adapter description
- `capabilities`: array of capability strings
- `command`: `null`, a string, or an array of strings

## Capability Names

Capability names are stable strings. Open Mosaic currently exposes descriptors
for:

- `pane.detect`
- `pane.metadata`
- `prompt.send`
- `prompt.queue`
- `prompt.submit.enter`
- `prompt.submit.tab`
- `observe.pane`
- `project.detect`
- `repo.metadata`
- `task.reference`
- `task.link_ref`
- `task.status`
- `identity.local_user`
- `audit.actor`
- `machine.local`
- `machine.context`
- `transport.local_process`
- `transport.local_socket`
- `transport.ssh`
- `machine.remote`

Optional Hasna or open-* integrations should be represented as ordinary
manifests with these portable fields. They must remain optional and must not
be required for local session, pane, prompt, observation, queue, or audit
commands to work.
