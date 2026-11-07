#!/usr/bin/env bash
# E2E_ENGINE=native scripts/e2e/run-all.sh: HAETTY_BIN points here. Starts the real `haetty` with
# `--engine $E2E_ENGINE` (a config setting) in front of its options; subcommands pass unchanged (exec: same PID).
real=$HAETTY_E2E_REAL_BIN
case "${1:-}" in
  daemon | status | events | screen | completions | install-desktop | hook | statusline | --version | --help | -h) exec "$real" "$@" ;;
  gui) shift; exec "$real" gui --engine "$E2E_ENGINE" "$@" ;;
  *) exec "$real" --engine "$E2E_ENGINE" "$@" ;;
esac
