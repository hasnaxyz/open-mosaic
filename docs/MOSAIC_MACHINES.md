# Open Mosaic Machines And Transports

Open Mosaic machine support is optional. The core product works on one local
machine without any private registry. Multi-machine workflows use portable
machine registry files and transports such as SSH.

Machine output uses the control envelope `schema_version:
"mosaic.control.v1"`. Machine registry files use `schema_version:
"mosaic.machine.v1"`.

## CLI

```sh
mosaic machines local
mosaic machines list
mosaic machines list --file machines.json
mosaic machines validate --file machines.json
mosaic --dry-run machines exec --file machines.json --machine dev -- sessions list
```

`machines local` returns a portable descriptor for the current machine.
`machines list` always includes the built-in `local` descriptor. If
`${XDG_CONFIG_HOME:-$HOME/.config}/open-mosaic/machines.json` exists, it also
loads configured machines. Pass `--file` to use a specific registry file.

`machines validate` checks a registry without connecting to any machine.

`machines exec` runs a remote Mosaic command through the machine transport. Use
top-level `--dry-run` to inspect the command plan without connecting:

```sh
mosaic --dry-run machines exec --file machines.json --machine dev -- panes list --all
```

The command after `--` is a Mosaic command without the `mosaic` binary name.
Open Mosaic prepends the configured `mosaic_bin` value or `mosaic`.
For the built-in `local` machine, pass `--mosaic-bin` if the binary you want to
exercise is not on `PATH`:

```sh
mosaic machines exec --machine local --mosaic-bin ./target/debug/mosaic -- --version
```

## Registry Shape

```json
{
  "schema_version": "mosaic.machine.v1",
  "machines": [
    {
      "id": "dev",
      "name": "Development host",
      "transport": {
        "kind": "ssh",
        "host": "dev.example.org",
        "user": "alice",
        "port": 22,
        "mosaic_bin": "/usr/local/bin/mosaic"
      },
      "tags": ["linux", "ci"]
    }
  ]
}
```

Required machine fields:

- `id`: ASCII letters, numbers, dots, underscores, and hyphens only
- `transport.kind`: `local` or `ssh`

Required SSH fields:

- `transport.host`: SSH host name or address

Optional SSH fields:

- `transport.user`
- `transport.port`
- `transport.mosaic_bin`

The SSH host and user must not start with `-` and must not contain whitespace,
control characters, or NUL bytes. The host must not contain `@`; use
`transport.user` for SSH users. Registry files should contain routing metadata,
not passwords, API keys, or prompt bodies. Open Mosaic does not execute adapter
commands or registry hooks while validating a registry file.

## SSH Execution

For SSH transports, Open Mosaic builds an argument-vector call to local `ssh`
with `BatchMode=yes`, optional `-p`, the validated target, and one POSIX-quoted
remote Mosaic command string. It does not concatenate local shell strings.

Example dry-run output includes a command plan:

```json
{
  "event": "machines.exec",
  "status": "dry_run",
  "transport": "ssh",
  "command": {
    "argv": [
      "ssh",
      "-o",
      "BatchMode=yes",
      "-p",
      "22",
      "alice@dev.example.org",
      "/usr/local/bin/mosaic sessions list"
    ]
  }
}
```

Immediate execution returns `stdout`, `stderr`, `exit_code`, and `status`.
If stdout is valid JSON, `stdout_json` is included for callers that want to
consume the remote Mosaic envelope.

## Prompt Safety

Machine support does not add a separate remote prompt API. To prompt a remote
pane, run the normal Mosaic prompt command on that machine:

```sh
mosaic --dry-run machines exec --file machines.json --machine dev -- \
  prompt send --pane-id terminal_1 --text "status?" --no-submit
```

Use dry-run first, then inspect the remote pane with `observe pane` or
`subscribe` before sending follow-up prompts. Receipts still mean the target
Mosaic server accepted the requested action; they do not prove that the remote
terminal process consumed or completed the prompt.

Use `--redact-command` when a dry-run or execution command contains prompt
bodies or prompt file paths:

```sh
mosaic --dry-run machines exec --redact-command --file machines.json --machine dev -- \
  prompt send --pane-id terminal_1 --text "private prompt"
```

Audit records for `machines.exec` redact command plans by default and do not
store remote stdout or stderr.

## Optional Integrations

Open Machines or organization-specific registries can integrate by producing
ordinary `mosaic.machine.v1` registry files or adapter outputs. They must remain
optional. Local sessions, queues, observation, dashboard, and prompt delivery
must continue to work without them.
