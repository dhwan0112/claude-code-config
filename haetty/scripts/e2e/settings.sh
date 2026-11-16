#!/usr/bin/env bash
# Inside xvfb.sh: the settings button and the settings panel (stage PS, DESIGN.md §2.4, §3.16). A shell tab whose
# program records every byte it reads and turns mouse reports on when asked; a Claude Code tab (a fake `claude`) asked
# for a permission through the hooks. Checks: the three ways to open the panel, the button over a shell tab's corner
# only near the pointer and gone on a key or under mouse reports, the theme preview and Esc, a font-size change (one
# session resize, saved with comments and unknown keys kept, a backup, kept after a restart), nothing reaching the
# program while the panel is open, a permission-waiting tab unchanged. Test isolation: writes only in $HAETTY_RUN.
# usage: settings.sh [PNG-DIR]
. "$(dirname "$0")/lib.sh"
OUT=${1:-}
REAL_CFG=$HOME/.config/haetty/config
real_stamp() { stat -c '%Y %s' "$REAL_CFG" 2>/dev/null || echo none; }
REAL0=$(real_stamp)
W=$HAETTY_RUN/settingsrun; haetty_guard $W; rm -rf $W; mkdir -p $W/rt $W/xrt $W/bin $W/home $W/claude $W/state $W/cfg
chmod 700 $W/rt $W/xrt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg XDG_RUNTIME_DIR=$W/xrt
export HAETTY_LOG=debug HAETTY_LOG_FILE=$W/gui.log HAETTY_DUMP_FRAME=$W/f.ppm CLAUDE_CONFIG_DIR=$W/claude HOME=$W/home
export PATH=$W/bin:$PATH EDITOR=
unset $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_[A-Z_]*SESSION[A-Z_]*)=' | tr -d =) 2>/dev/null
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
# The program: every byte it reads goes to $1; the file mouse-on makes it ask for mouse reports (?1000h).
cat > $W/rec.py <<'PY'
import os, select, sys, termios, tty
log, flag = sys.argv[1], sys.argv[2]
tty.setraw(0)
sys.stdout.write("ready\r\n"); sys.stdout.flush()
asked = False
with open(log, "ab", buffering=0) as f:
    while True:
        r, _, _ = select.select([0], [], [], 0.2)
        if r:
            b = os.read(0, 4096)
            if not b:
                break
            f.write(b)
        if not asked and os.path.exists(flag):
            sys.stdout.write("\033[?1000h"); sys.stdout.flush(); asked = True
PY
cat > $W/bin/claude <<EOF
#!/bin/sh
exec python3 -I $W/rec.py $W/claude-in.log $W/never
EOF
chmod +x $W/bin/claude
printf '# my settings\nbogus-key = 1\n' > $W/cfg/config
cp $W/cfg/config $W/config.orig
J() { HAETTY_LOG=warn $BIN status --json; }
sid_of() { J | python3 -Ic "import json,sys; s=[x for x in json.load(sys.stdin)['sessions'] if '$1' in x['program']]; print(s[-1]['id'] if s else '')"; }
field() { J | python3 -Ic "import json,sys; s=[x for x in json.load(sys.stdin)['sessions'] if x['id']=='$1']; print(s[0]['$2'] if s else '')"; }
hk() { echo "$3" | HAETTY_SESSION_ID=$1 $BIN hook $2 > /dev/null; }
key() { xdotool key --clearmodifiers "$@"; sleep 0.5; }
pal() { key ctrl+shift+p; xdotool type --delay 30 "$1"; sleep 0.3; key Return; }
size_of() { stat -c %s "$1" 2>/dev/null || echo 0; }
opened() { grep -c 'settings: opened' $W/gui.log; }
geom() { xdotool getwindowgeometry --shell $GW | awk -F= '/WIDTH|HEIGHT/{print $2}' | paste -sd' '; }
# Non-background pixels in the frame's bottom-right cell area (the frame's top-left pixel is the background).
ink() {
  sleep 0.6
  python3 -I - $W/f.ppm "$1" "$2" <<'PY'
import sys
d = open(sys.argv[1], "rb").read(); p = d.split(maxsplit=4); w, h = int(p[1]), int(p[2]); px = p[4]
cols, rows = int(sys.argv[2]), int(sys.argv[3])
cw, ch = w // cols, h // rows
bg = px[0:3]
n = 0
for y in range(h - ch - 6, h):
    for x in range(w - cw - 10, w):
        i = 3 * (y * w + x)
        if px[i:i + 3] != bg:
            n += 1
print(n)
PY
}
# The frame's background (top-left pixel) as #rrggbb.
bgcol() { sleep 0.6; python3 -I -c "import sys; d=open('$W/f.ppm','rb').read(); p=d.split(maxsplit=4); print('#%02x%02x%02x' % tuple(p[4][0:3]))"; }
shot() { [ -n "$OUT" ] && sleep 0.5 && pnmtopng $W/f.ppm > "$OUT/settings-$1.png" 2> /dev/null; }
start() {
  setsid $BIN --cols 80 --rows 20 --restore-tabs false "$@" > /dev/null 2>&1 < /dev/null & GP=$!
  GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2
  for i in 1 2 3 4 5 6 7 8 9 10; do xdotool windowfocus --sync $GW 2>/dev/null; [ "$(xdotool getwindowfocus)" = "$GW" ] && break; sleep 0.3; done
}
stop() { kill $GP 2>/dev/null; wait $GP 2>/dev/null; haetty_stop_daemon "$BIN"; sleep 0.4; }

# --- A shell tab ------------------------------------------------------------------------------------------------
start python3 -I $W/rec.py $W/shell-in.log $W/mouse-on
read -r WW WH <<< "$(geom)"
corner() { xdotool mousemove --window $GW $((WW - 3)) $((WH - 3)); sleep 0.4; }
away() { xdotool mousemove --window $GW $((WW / 2)) $((WH / 3)); sleep 0.4; }
away
[ "$(ink 80 20)" = 0 ] && ok "shell tab: no button while the pointer is away" || bad "button away" "$(ink 80 20) px"
corner; n=$(ink 80 20); shot corner
[ "$n" -gt 0 ] && ok "shell tab: the button near the corner ($n px)" || bad "button near" "0 px"
xdotool type a; sleep 0.3
[ "$(ink 80 20)" = 0 ] && ok "a key hides the button" || bad "key hides" "$(ink 80 20) px"
away; corner
r0=$(grep -c 'resize session' $W/gui.log)
# 1. Open by the button.
b0=$(size_of $W/shell-in.log)
xdotool click 1; sleep 0.6
[ "$(opened)" = 1 ] && ok "a click on the button opens the panel" || bad "button click" "$(opened) opens"
shot panel
# Typed into the panel: nothing reaches the program.
xdotool type --delay 40 "abc/xyz"; key BackSpace; key Escape; key Down; key Up; key Tab; key shift+Tab
xdotool click 1; sleep 0.3; xdotool click 4; xdotool click 5; sleep 0.3
key Escape
grep -q 'settings: closed' $W/gui.log && ok "Esc closes the panel" || bad "Esc" "no close"
b1=$(size_of $W/shell-in.log)
[ "$b1" = "$b0" ] && ok "nothing reached the program while the panel was open ($b0 bytes before and after)" || bad "bytes" "$b0 -> $b1"
# 2. Ctrl+, and 3. the palette.
key ctrl+comma; [ "$(opened)" = 2 ] && ok "Ctrl+, opens the panel" || bad "Ctrl+," "$(opened)"
key ctrl+comma; [ "$(grep -c 'settings: closed' $W/gui.log)" = 2 ] && ok "Ctrl+, closes it again" || bad "Ctrl+, close" "-"
pal "settings"; [ "$(opened)" = 3 ] && ok "the palette's Settings opens the panel" || bad "palette" "$(opened)"
# Theme preview: the list moves, the window shows it; Esc goes back; nothing written.
c0=$(bgcol)
key Return; key Down; c1=$(bgcol); shot preview
key Escape; c2=$(bgcol)
[ "$c1" != "$c0" ] && [ "$c2" = "$c0" ] && ok "theme preview ($c0 -> $c1) and Esc back ($c2)" || bad "preview" "$c0 $c1 $c2"
cmp -s $W/cfg/config $W/config.orig && ok "a preview writes nothing" || bad "preview wrote" "$(cat $W/cfg/config)"
# Font size: three steps quickly; one resize; the file keeps its other lines.
cols0=$(field "$(sid_of python3)" cols)
key Tab; key Down; xdotool key --delay 40 Right Right Right; sleep 1
cols1=$(field "$(sid_of python3)" cols)
r1=$(grep -c 'resize session' $W/gui.log)
[ "$cols1" -lt "$cols0" ] && ok "font size 11 -> 14: the grid $cols0 -> $cols1 columns" || bad "grid" "$cols0 -> $cols1"
[ $((r1 - r0)) = 1 ] && ok "three quick steps: one session resize" || bad "resizes" "$((r1 - r0))"
shot font
grep -q '^font-size = 14$' $W/cfg/config && grep -q '^# my settings$' $W/cfg/config && grep -q '^bogus-key = 1$' $W/cfg/config \
  && ok "saved: font-size = 14, the comment and the unknown key kept" || bad "file" "$(cat $W/cfg/config | tr '\n' '|')"
cmp -s $W/cfg/config.bak $W/config.orig && ok "config.bak is the file before the first change" || bad "backup" "$(ls $W/cfg)"
key Escape
fw0=$(head -c 20 $W/f.ppm | python3 -Ic "import sys; print(sys.stdin.buffer.read().split()[1].decode())")
# Mouse reports: the button stays away from the program's clicks.
touch $W/mouse-on; sleep 0.6; away; corner
[ "$(ink 80 20)" = 0 ] && ok "no button while the program reads the mouse" || bad "mouse reports" "$(ink 80 20) px"
stop
# Kept after a restart: the same 80x20 grid needs a wider window at 14 pt.
start sh -c 'exec sleep 600'
read -r WW2 WH2 <<< "$(geom)"
[ "$WW2" -gt "$WW" ] && ok "after a restart: font size 14 kept (window $WW -> $WW2 px wide for 80 columns)" || bad "restart" "$WW -> $WW2"
stop

# --- A Claude Code tab waiting for a permission -----------------------------------------------------------------
start claude
S=$(sid_of claude)
xdotool mousemove --window $GW 20 20; sleep 0.3
n=$(ink 80 20)
[ "$n" -gt 0 ] && ok "Claude Code tab: the button in the status row without the pointer ($n px)" || bad "claude button" "0 px"
hk $S PermissionRequest '{"session_id":"cc-1","tool_name":"Bash"}'; sleep 0.6
a0=$(field $S attention); c0=$(size_of $W/claude-in.log)
key ctrl+comma; xdotool type --delay 40 "1y"; key Return; key Escape; key Escape
a1=$(field $S attention); c1=$(size_of $W/claude-in.log)
[ "$a0" = Ask ] && [ "$a1" = Ask ] && ok "permission wait: Ask before and after the panel" || bad "attention" "$a0 -> $a1"
[ "$c0" = "$c1" ] && ok "permission wait: nothing reached Claude Code" || bad "claude bytes" "$c0 -> $c1"
stop

[ "$(real_stamp)" = "$REAL0" ] && ok "the real config file is untouched" || bad "real config" "$REAL0 -> $(real_stamp)"
echo "TOTAL settings.sh: $P passed, $F failed"
