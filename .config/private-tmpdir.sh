#!/bin/sh
# Nextest setup script: gives the plugin tests a private, empty TMPDIR before they start,
# so native shadow copies and their stale-process cleanup never touch the machine's
# shared temp directory, and nothing is left there afterwards.
set -eu
dir="$PWD/target/nextest-tmp"
rm -rf "$dir"
mkdir -p "$dir"
chmod 700 "$dir"
echo "TMPDIR=$dir" >> "$NEXTEST_ENV"
