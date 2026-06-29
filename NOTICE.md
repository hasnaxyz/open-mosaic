# Open Mosaic Fork Notice

Open Mosaic is an OSS-first agentic terminal workspace derived from
[Zellij](https://github.com/zellij-org/zellij). Zellij's MIT license and
copyright notices are preserved in `LICENSE.md`. The Hasna Open Mosaic
distribution is licensed under Apache-2.0; see `LICENSE`.

The Open Mosaic project adds Mosaic-specific control surfaces, agent workflow
primitives, structured observation, prompt delivery receipts, audit records,
and optional integration adapters. These additions should not be read as
upstream Zellij features unless they are accepted upstream.

Public Open Mosaic names introduced by this fork use `Open Mosaic` for the
product and `mosaic` for the agent-oriented CLI. Existing `zellij-*` crate,
module, socket, config, plugin URI, and compatibility binary names remain an
implementation compatibility layer unless a later migration explicitly renames
or isolates them.

Do not add private Hasna machine names, local paths, services, credentials, or
package registries to the product core. Hasna/open-* integrations belong behind
optional adapters or plugins.
