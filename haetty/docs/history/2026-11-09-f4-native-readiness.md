# 2026-11-09 — F4: native-only readiness

Stage f4 (DESIGN.md section 10.15).

- Dist sizes (Linux, Windows) and the MSVC build without WSL: section 10.15.
- Key to frame latency: native 1.07-1.08 ms median, libghostty 0.94-0.98 ms (budget 16 ms): not met.
- Throughput, five runs per engine, quiet: lines 117 %, wrapped 132 %, SGR 80.0 % (F3: 82 %).
- Installer self-tests: Linux 19/19, Windows 18/18 (default build). No native-only installer option.
- e2e (run-all.sh): libghostty sessions, native sessions, native-only build: all pass.
- A first run of the e2e suite skipped (no Xvfb on PATH); the Xvfb unpacked at ~/m2/xroot is used via HAETTY_XROOT.
