#!/usr/bin/env bash
set -euo pipefail

log() {
  printf '[post-start] %s\n' "$*"
}

on_error() {
  local exit_code=$?
  log "FAILED (exit=${exit_code}) at line ${BASH_LINENO[0]}: ${BASH_COMMAND}"
  exit "$exit_code"
}

trap on_error ERR

log "post-start complete"
