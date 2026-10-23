#!/bin/sh
# Record HTTP/auth/affinity callbacks against read-only CLIProxyAPI v8.0.35.
# Use Go 1.26 on PATH; Go 1.27 changes encoding/json's base64 error text.
set -eu
here=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
full=$(mktemp)
trap 'rm -f "$full"' EXIT
sh "$here/gen.sh" "${1:?usage: gen-callbacks.sh /path/to/CLIProxyAPI-v8.0.35}" "$full"
# The first scenario supplies HTTP operations/streams. The final two scenarios
# supply proxy overrides and auth-manager callbacks. Registration is setup here;
# the original golden still checks the complete host capability sequence.
jq '{reference: "v8.0.35", steps: (.steps |
  ([.[0], .[1], .[4], .[7], .[8]] + .[159:]) |
  map(select(.op != "records")) |
  map(if .op == "apply" or .op == "register_management" then del(.result) else . end))}' \
  "$full" > "$here/../../fixtures/host_callbacks_v8035.json"
