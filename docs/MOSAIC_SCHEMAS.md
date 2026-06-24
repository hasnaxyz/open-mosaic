# Open Mosaic Schemas

Open Mosaic machine-readable output is versioned. The first public contract is
`mosaic.control.v1`, published as:

```sh
schemas/mosaic.control.v1.schema.json
```

The schema file uses JSON Schema draft 2020-12 and is packaged with release
artifacts under `usr/share/doc/open-mosaic/schemas/`. It is a contract for
agents and integrations that consume `mosaic` JSON or NDJSON without scraping
terminal text.

## Versioning

- `schema_version: "mosaic.control.v1"` identifies top-level control events.
- `mosaic_agent.schema_version: "mosaic.agent.v1"` identifies pane metadata.
- Adapter, machine, goals, and web helper sub-surfaces keep their own
  `mosaic.*.v1` version fields inside the control envelope.
- Unknown JSON fields are reserved for compatible additions. Consumers should
  ignore fields they do not understand.
- Breaking changes require a new schema version and documentation migration
  notes.

## Covered Events

The v1 schema includes definitions for:

- delivery receipts from `prompt send`, `prompt queue`, session, pane, tab, and
  queue operations
- `panes list --all` agent metadata via the `agentMetadata` and `paneEntry`
  definitions
- local `queued_prompt`, `queue.list`, and `audit.list` records
- replayable `audit.export`, `audit.export.record`, and `audit.verify` records
- `observe.pane` structured activity capture
- `subscribe` NDJSON `pane_update` and `pane_closed` events
- `dashboard.snapshot`
- `web.link`
- adapter list/validation descriptors
- machine and goals registry envelopes
- machine-readable `error` events

The schema is intentionally permissive where upstream Zellij-derived fields can
vary, for example raw session, pane, and tab objects. Open Mosaic-specific
fields are stricter where agents depend on them for routing, receipts,
redaction, and observation safety.

## Security Notes

Schemas describe shape, not trust. A valid receipt means Mosaic accepted or
queued an operation; it does not prove that the process inside the pane
consumed the bytes or completed work. Controllers should pair receipts with
`observe pane`, `capture`, or `subscribe` evidence before sending follow-up
work. `audit.verify` only proves that an exported audit stream is structurally
and cryptographically self-consistent; it does not prove that a terminal
process consumed the original prompt.

Redacted values may appear as the literal `[redacted]`. Audit export hashes are
computed after redaction when `--redact` is used, so a redacted stream can be
verified without access to the original prompt bodies. Audit and observation
records are local user-state evidence and should be treated as sensitive unless
the caller explicitly redacts them.
