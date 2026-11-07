#!/usr/bin/env bash
# E2E_ENGINE=native scripts/e2e/run-all.sh: HAETTY_HEADLESS_BIN points here (see engine-gui.sh).
exec "$HAETTY_E2E_REAL_HL" --engine "$E2E_ENGINE" "$@"
