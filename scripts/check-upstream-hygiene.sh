#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

failures=0

fail() {
  printf 'ERROR: %s\n' "$*" >&2
  failures=$((failures + 1))
}

ok() {
  printf 'ok: %s\n' "$*"
}

require_file() {
  if [ ! -f "$1" ]; then
    fail "missing required file: $1"
  else
    ok "found $1"
  fi
}

require_contains() {
  file=$1
  pattern=$2
  label=$3
  if grep -Eq "$pattern" "$file"; then
    ok "$label"
  else
    fail "$label"
  fi
}

require_cargo_include() {
  path=$1
  if grep -Fq "\"$path\"" Cargo.toml; then
    ok "Cargo.toml includes $path"
  else
    fail "Cargo.toml package.include is missing $path"
  fi
}

require_deb_doc_asset() {
  path=$1
  name=$(basename "$path")
  if grep -Fq "[\"$path\", \"usr/share/doc/open-mosaic/$name\", \"644\"]" Cargo.toml; then
    ok "Debian metadata installs $path"
  else
    fail "Debian metadata does not install $path under usr/share/doc/open-mosaic"
  fi
}

require_file LICENSE.md
require_file NOTICE.md
require_file docs/UPSTREAM_MAINTENANCE.md
require_file docs/RELEASE.md

require_contains LICENSE.md 'Copyright \(c\) 2020 Zellij contributors' \
  "LICENSE.md preserves upstream Zellij copyright"
require_contains LICENSE.md '^MIT License$' \
  "LICENSE.md preserves MIT license title"
require_contains NOTICE.md 'derived from' \
  "NOTICE.md states Open Mosaic is a derivative"
require_contains NOTICE.md 'Zellij' \
  "NOTICE.md names upstream Zellij"
require_contains NOTICE.md 'upstream Zellij features unless they are accepted upstream' \
  "NOTICE.md avoids claiming fork features are upstream"
require_contains docs/UPSTREAM_MAINTENANCE.md 'git remote set-url --push upstream DISABLED' \
  "upstream maintenance guide documents disabled upstream push URL"
require_contains docs/UPSTREAM_MAINTENANCE.md 'scripts/check-upstream-hygiene.sh' \
  "upstream maintenance guide references this check"

if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  upstream_url=$(git remote get-url upstream 2>/dev/null || true)
  if [ "$upstream_url" = "https://github.com/zellij-org/zellij.git" ]; then
    ok "upstream fetch URL points to zellij-org/zellij"
  else
    fail "upstream fetch URL should be https://github.com/zellij-org/zellij.git (got ${upstream_url:-missing})"
  fi

  upstream_push_url=$(git remote get-url --push upstream 2>/dev/null || true)
  case "$upstream_push_url" in
    ""|"DISABLED")
      ok "upstream push URL is disabled"
      ;;
    "https://github.com/zellij-org/zellij.git"|"git@github.com:zellij-org/zellij.git")
      fail "upstream push URL points at upstream Zellij; run: git remote set-url --push upstream DISABLED"
      ;;
    *)
      ok "upstream push URL does not point at upstream Zellij ($upstream_push_url)"
      ;;
  esac
else
  fail "not running inside a git worktree"
fi

for path in \
  docs/UPSTREAM_MAINTENANCE.md \
  docs/RELEASE.md \
  docs/OPEN_MOSAIC.md \
  docs/MOSAIC_SCHEMAS.md \
  NOTICE.md
do
  require_cargo_include "$path"
  require_deb_doc_asset "$path"
done

require_cargo_include schemas/mosaic.control.v1.schema.json
if grep -Fq '["schemas/*.json", "usr/share/doc/open-mosaic/schemas/", "644"]' Cargo.toml; then
  ok "Debian metadata installs Mosaic JSON schemas"
else
  fail "Debian metadata does not install Mosaic JSON schemas under usr/share/doc/open-mosaic/schemas"
fi

require_cargo_include scripts/check-upstream-hygiene.sh
require_deb_doc_asset scripts/check-upstream-hygiene.sh
require_cargo_include scripts/mosaic-agent-workflow-smoke.sh
require_deb_doc_asset scripts/mosaic-agent-workflow-smoke.sh

private_scan_paths="README.md NOTICE.md Cargo.toml docs src/bin/mosaic.rs src/bin/mosaic"
private_patterns='(/home/hasna|Spark[0-9]*|spark[0-9]*)'

if command -v rg >/dev/null 2>&1; then
  private_matches=$(rg -n "$private_patterns" $private_scan_paths || true)
else
  private_matches=$(grep -RInE "$private_patterns" $private_scan_paths 2>/dev/null || true)
fi

private_matches=$(printf '%s\n' "$private_matches" | grep -Ev 'assert!\(!.*contains' || true)

if [ -n "$private_matches" ]; then
  printf '%s\n' "$private_matches" >&2
  fail "product docs/core contain private machine names or local developer paths"
else
  ok "product docs/core avoid private machine names and local developer paths"
fi

if [ "$failures" -ne 0 ]; then
  printf '\n%d upstream hygiene check(s) failed.\n' "$failures" >&2
  exit 1
fi

printf '\nOpen Mosaic upstream hygiene checks passed.\n'
