#!/usr/bin/env bash
# usage: scripts/e2e/xvfb.sh CMD [ARGS] — runs CMD with a private Xvfb display (96 dpi, 1600x1000, no window manager).
# Xvfb from PATH when /tmp/.X11-unix is writable; else an Xvfb unpacked without root ($HAETTY_XROOT: a folder
# holding usr/bin/Xvfb, usr/bin/xkbcomp, their libraries and misc fonts — see README.md here) inside a private mount
# namespace where xkbcomp is overlaid on /usr/bin and /tmp/.X11-unix is writable. Exit 77: no Xvfb.
# Test isolation: everything is written only in $HAETTY_RUN (scripts/test-guard.sh).
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../test-guard.sh"; haetty_guard_init HAETTY_TEST_TMP || exit 3
if command -v Xvfb > /dev/null && [ -w /tmp/.X11-unix ]; then
  n=77; while [ -e /tmp/.X11-unix/X$n ] || [ -e /tmp/.X$n-lock ]; do n=$((n + 1)); done
  Xvfb :$n -screen 0 1600x1000x24 -dpi 96 -nolisten tcp > "$HAETTY_RUN/xvfb.log" 2>&1 &
  XP=$!
  unset WAYLAND_DISPLAY; export DISPLAY=:$n WINIT_X11_SCALE_FACTOR=1
  for i in $(seq 1 20); do xdotool getdisplaygeometry > /dev/null 2>&1 && break; sleep 0.25; done
  "$@"; rc=$?
  kill $XP 2>/dev/null; wait $XP 2>/dev/null; exit $rc
fi
X=${HAETTY_XROOT:-}
[ -n "$X" ] && [ -x "$X/usr/bin/Xvfb" ] || { echo "xvfb.sh: no usable Xvfb (install xvfb, or set HAETTY_XROOT)" >&2; exit 77; }
export XNS_UID=$(id -u) XNS_GID=$(id -g) XNS_X=$X XNS_RUN=$HAETTY_RUN
exec unshare -rm sh -c '
  mkdir -p $XNS_RUN/xns-up; cp $XNS_X/usr/bin/xkbcomp $XNS_RUN/xns-up/
  mount -t overlay overlay -o lowerdir=$XNS_RUN/xns-up:/usr/bin /usr/bin || { echo OVERLAY_FAIL; exit 1; }
  mount -t tmpfs none "$(readlink -f /tmp/.X11-unix)" || echo X11UNIX_FAIL
  for d in $XNS_HIDE; do mount -t tmpfs none "$d" || echo "HIDE_FAIL $d"; done
  exec unshare -U --map-user=$XNS_UID --map-group=$XNS_GID sh -c '"'"'
    LD_LIBRARY_PATH=$XNS_X/usr/lib/x86_64-linux-gnu $XNS_X/usr/bin/Xvfb :77 -screen 0 1600x1000x24 -dpi 96 -nolisten tcp -fp $XNS_X/usr/share/fonts/X11/misc > $XNS_RUN/xvfb.log 2>&1 &
    XP=$!
    unset WAYLAND_DISPLAY
    export DISPLAY=:77 WINIT_X11_SCALE_FACTOR=1
    for i in 1 2 3 4 5 6 7 8 9 10 11 12; do xdotool getdisplaygeometry >/dev/null 2>&1 && break; sleep 0.25; done
    "$@"; rc=$?
    kill $XP 2>/dev/null; exit $rc
  '"'"' xns "$@"
' xns "$@"
