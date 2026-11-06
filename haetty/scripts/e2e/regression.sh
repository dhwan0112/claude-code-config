#!/usr/bin/env bash
# Regression run (inside xvfb.sh). usage: regression.sh TAG
# Isolated daemon (own socket and state dir). Checks: keys/Shift+Enter/paste/wheel/focus through the GUI
# (probe), OSC 52 copy, resize, Ctrl+C twice exit with Claude Code (6 s settle), IME preedit (ibus-hangul),
# frame dumps for Hangul two-cell and monochrome symbols (looked at by hand).
# Test isolation: everything below writes only in $HAETTY_RUN (scripts/test-guard.sh).
. "$(dirname "$0")/lib.sh"
TAG=$1
OUT=$HAETTY_RUN/reg-$TAG; haetty_guard $OUT; mkdir -p $OUT
W=$HAETTY_RUN/reg-$TAG-w; haetty_guard $W; mkdir -p $W/rt; chmod 700 $W/rt
export HAETTY_SOCKET=$W/rt/daemon.sock HAETTY_STATE_DIR=$W/state HAETTY_LOG=debug
pass=0; fail=0
ok() { echo "PASS $1"; pass=$((pass+1)); }
bad() { echo "FAIL $1 :: $2"; fail=$((fail+1)); }
sid() { $BIN status --json | python3 -Ic 'import json,sys; s=json.load(sys.stdin)["sessions"]; print(s[-1]["id"] if s else "")'; }
screen() { $BIN status --screen "$(sid)" 2>&1; }
wait_screen() { for i in $(seq 1 ${2:-50}); do screen | grep -qF -- "$1" && return 0; sleep 0.2; done; return 1; }
gui() { local r=(--restore-tabs false); [ "$RESTORE" = 1 ] && r=(); HAETTY_LOG_FILE=$OUT/$1.log setsid $BIN --cols 80 --rows 20 "${r[@]}" "${@:2}" > /dev/null 2>&1 < /dev/null & GP=$!; GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); }
# Focus a window and check that it took (Xvfb has no window manager).
focus() { for i in 1 2 3 4 5 6 7 8 9 10; do xdotool windowfocus --sync $1 2>/dev/null; [ "$(xdotool getwindowfocus 2>/dev/null)" = "$1" ] && return 0; sleep 0.3; done; echo "WARN focus did not take for $1"; return 1; }
# Ends the window AND the daemon: a killed window leaves its sessions in the daemon, and the next window would
# reattach them (wanted in use, not between independent sections).
stop() { kill $GP 2>/dev/null; wait $GP 2>/dev/null; haetty_stop_daemon "$BIN"; sleep 0.4; }
# The sessions' program PIDs (to end them by PID when the daemon is killed).
session_pids() { HAETTY_LOG=warn $BIN status --json 2>/dev/null | python3 -Ic 'import json,sys; print(" ".join(str(s["pid"]) for s in json.load(sys.stdin)["sessions"] if s.get("pid")))'; }

# 1. Keys, Shift+Enter, paste, wheel, focus — encodings as the probe receives them.
gui keys python3 $PROBE echo-input 1006 1000 2004 1004 kitty=1
if wait_screen READY; then
  focus $GW; sleep 0.3
  xdotool type --delay 40 'aZ'; xdotool key shift+Return
  printf 'pasted-text' | xclip -selection clipboard -i; sleep 0.2; xdotool key ctrl+shift+v
  xdotool mousemove --window $GW 60 60; sleep 0.1; xdotool click 4; sleep 0.5
  sc=$(screen); echo "$sc" > $OUT/keys-screen.txt
  for want in 'a' 'Z' '\e[13;2u' '\e[200~pasted-text\e[201~' '\e[<64;' '\e[I'; do
    echo "$sc" | grep -qF -- "$want" && ok "keys: $want" || bad "keys: $want" "not in probe output"
  done
else bad "keys" "probe not READY: $(screen | tail -3)"; fi
stop

# 2. OSC 52: the application's copy reaches the clipboard.
printf '' | xclip -selection clipboard -i
gui osc52 sh -c "printf '\033]52;c;%s\007' \$(printf 'osc52-ok' | base64); sleep 3"
sleep 1.5; got=$(xclip -selection clipboard -o 2>/dev/null)
[ "$got" = osc52-ok ] && ok "osc52 copy" || bad "osc52 copy" "clipboard='$got'"
stop

# 3. Resize: the session's grid follows the window; padding frame dump.
HAETTY_DUMP_FRAME=$W/resize.ppm gui resize sh -c 'sleep 30'
sleep 1; xdotool windowsize --sync $GW 500 300; sleep 1
cr=$($BIN status --json | python3 -Ic 'import json,sys; s=json.load(sys.stdin)["sessions"][-1]; print(s["cols"], s["rows"])')
[ "$cr" != "80 20" ] && ok "resize -> grid $cr" || bad "resize" "grid stayed $cr"
cp $W/resize.ppm $OUT/resize.ppm 2>/dev/null
stop

# 4. Ctrl+C twice exits Claude Code (6 s after start), window closes.
( . "$E2E/claude-home.sh"; cd "$CCPROJ"
  [ -x "$CLAUDE" ] || { echo SKIP > $W/ctrlc.n; exit 0; }
  n=0
  for r in 1 2 3; do
    HAETTY_LOG_FILE=$OUT/ctrlc$r.log setsid $BIN --cols 100 --rows 30 --restore-tabs false "$CLAUDE" > /dev/null 2>&1 < /dev/null & P=$!
    WW=$(timeout 10 xdotool search --sync --pid $P | head -1)
    for i in $(seq 1 150); do case "$(xdotool getwindowname $WW 2>/dev/null)" in *"Claude Code"*) break;; esac; sleep 0.1; done
    sleep 6; xdotool windowfocus --sync $WW; sleep 0.3; xdotool key ctrl+c; sleep 0.3; xdotool key ctrl+c
    for i in $(seq 1 40); do kill -0 $P 2>/dev/null || break; sleep 0.1; done
    if kill -0 $P 2>/dev/null; then kill $P; else n=$((n+1)); fi; wait $P 2>/dev/null; haetty_stop_daemon "$BIN"; sleep 0.3
  done
  echo "$n" > $W/ctrlc.n )
case "$(cat $W/ctrlc.n)" in 3) ok "ctrl+c twice exits claude (3/3)";; SKIP) echo "SKIP ctrl+c (no claude on PATH)";; *) bad "ctrl+c" "$(cat $W/ctrlc.n)/3";; esac

# 5. IME: ibus-hangul over XIM, composing shown, commit reaches the program.
export XMODIFIERS=@im=ibus GTK_IM_MODULE=ibus
eval "$(dbus-launch --sh-syntax)"; ibus-daemon -rx > /dev/null 2>&1 & IBP=$!; sleep 3; ibus engine hangul 2>/dev/null
gui ime python3 $PROBE echo-input
if wait_screen READY; then
  focus $GW; sleep 0.3; ibus engine hangul 2>/dev/null
  xdotool key --delay 120 g k s; sleep 0.5; xdotool key space; sleep 0.8
  pre=$(grep -c 'ime preedit' $OUT/ime.log); com=$(grep -c 'ime commit' $OUT/ime.log)
  sc=$(screen)
  [ "$pre" -gt 0 ] && [ "$com" -gt 0 ] && echo "$sc" | grep -qF '\xed\x95\x9c' && ok "ime preedit $pre, commit $com, 한 sent" || bad "ime" "preedit=$pre commit=$com screen=$(echo "$sc" | tail -2)"
else bad "ime" "probe not READY"; fi
stop; kill $IBP 2>/dev/null; wait $IBP 2>/dev/null; kill $DBUS_SESSION_BUS_PID 2>/dev/null; unset XMODIFIERS GTK_IM_MODULE

# 7. Tabs: new tab gets the input, Alt+1 switches back, close needs two presses, palette runs actions,
#    config-file keybind works; tab bar frame dump.
mkdir -p $W/cfg; printf '# test config\nkeybind = ctrl+shift+n=new_tab\nwindow-padding = 4\n' > $W/cfg/config
nth() { HAETTY_LOG=warn $BIN status --json | python3 -Ic "import json,sys; s=json.load(sys.stdin)['sessions']; i=$B+$1; print(s[i]['id'] if len(s)>i else '')"; }
count() { HAETTY_LOG=warn $BIN status --json | python3 -Ic 'import json,sys; print(len(json.load(sys.stdin)["sessions"]) - '$B')'; }
scr() { HAETTY_LOG=warn $BIN status --screen "$(nth $1)" 2>&1; }
B=0; B=$(count)  # sessions left by earlier sections (their GUIs were killed: sessions survive)
HAETTY_CONFIG_DIR=$W/cfg HAETTY_DUMP_FRAME=$W/tabs.ppm gui tabs python3 $PROBE echo-input
if wait_screen READY; then
  focus $GW; sleep 0.3
  xdotool key ctrl+shift+t; sleep 1.5
  [ "$(count)" = 2 ] && ok "new tab (2 sessions)" || bad "new tab" "sessions=$(count)"
  for i in $(seq 1 25); do scr 1 | grep -q READY && break; sleep 0.2; done
  xdotool type --delay 40 'x2'; sleep 0.5
  scr 1 | grep -qF 'GOT x' && ! scr 0 | grep -qF 'GOT x' && ok "input goes to the new tab" || bad "tab input" "tab1=$(scr 0 | tail -1) tab2=$(scr 1 | tail -1)"
  xdotool key alt+1; sleep 0.3; xdotool type --delay 40 'y1'; sleep 0.5
  scr 0 | grep -qF 'GOT y' && ! scr 1 | grep -qF 'GOT y' && ok "alt+1 selects tab 1" || bad "alt+1" "$(scr 0 | tail -2)"
  cp $W/tabs.ppm $OUT/tabs-two.ppm
  xdotool key ctrl+shift+w; sleep 0.5
  [ "$(count)" = 2 ] && ok "first ctrl+shift+w only warns" || bad "close confirm" "sessions=$(count)"
  cp $W/tabs.ppm $OUT/tabs-close-hint.ppm
  xdotool key ctrl+shift+w; sleep 1
  [ "$(count)" = 1 ] && ok "second ctrl+shift+w closes" || bad "close" "sessions=$(count)"
  xdotool key ctrl+shift+p; sleep 0.4; cp $W/tabs.ppm $OUT/tabs-palette.ppm
  xdotool type --delay 40 'new'; sleep 0.2; xdotool key Return; sleep 1.5
  [ "$(count)" = 2 ] && ok "palette: new tab" || bad "palette" "sessions=$(count)"
  xdotool key ctrl+shift+n; sleep 1.5
  [ "$(count)" = 3 ] && ok "config keybind ctrl+shift+n" || bad "config keybind" "sessions=$(count)"
  cp $W/tabs.ppm $OUT/tabs-three.ppm
else bad "tabs" "probe not READY"; fi
stop

# 8. Tab sets: "last" follows the tabs; a new window reopens them (set "previous"); restore-tabs false does not.
#    install-desktop files; X11 WM_CLASS and window icon.
mkdir -p $W/proj1 $W/proj2
B=0; B=$(count)
( cd $W/proj1; gui ts1 sh -c 'exec sleep 600'; sleep 1; focus $GW; sleep 0.3
  cd $W/proj2; xdotool key ctrl+shift+t; sleep 1.5
  rows=$(grep -vc '^#' $HAETTY_STATE_DIR/tabs/last.tsv 2>/dev/null)
  [ "$rows" = 2 ] && echo "PASS tab set last has 2 tabs" || echo "FAIL tab set last :: rows=$rows"
  wm=$(xprop -id $GW WM_CLASS 2>/dev/null); icon=$(xprop -id $GW _NET_WM_ICON 2>/dev/null | head -c 60)
  echo "$wm" | grep -q '"io.github.dhwan0112.haetty", "io.github.dhwan0112.haetty"' && echo "PASS WM_CLASS $wm" || echo "FAIL WM_CLASS :: $wm"
  echo "$icon" | grep -q 'CARDINAL' && echo "PASS _NET_WM_ICON set" || echo "FAIL _NET_WM_ICON :: $icon"
  stop )
B=0; B=$(count)
RESTORE=1 gui ts2 sh -c 'exec sleep 600'; sleep 3
n=$(count); [ "$n" = 3 ] && ok "restart reopened 2 tabs (+1 requested)" || bad "restore" "new sessions=$n"
[ -f $HAETTY_STATE_DIR/tabs/previous.tsv ] && ok "set previous written" || bad "previous" "missing"
grep -q 'reopened 2 tab' $OUT/ts2.log 2>/dev/null; stop
B=0; B=$(count)
gui ts3 --restore-tabs false sh -c 'exec sleep 600'; sleep 3
n=$(count); [ "$n" = 1 ] && ok "restore-tabs false: only the requested tab" || bad "restore off" "new sessions=$n"
stop
XDG_DATA_HOME=$W/xdg $BIN install-desktop > $OUT/install.txt 2>&1
[ -f $W/xdg/applications/io.github.dhwan0112.haetty.desktop ] && [ -f $W/xdg/icons/hicolor/256x256/apps/io.github.dhwan0112.haetty.png ] && [ -f $W/xdg/icons/hicolor/scalable/apps/io.github.dhwan0112.haetty.svg ] && ok "install-desktop files" || bad "install-desktop" "$(cat $OUT/install.txt)"
cp $W/xdg/applications/io.github.dhwan0112.haetty.desktop $W/xdg/icons/hicolor/256x256/apps/io.github.dhwan0112.haetty.png $OUT/ 2>/dev/null
command -v desktop-file-validate >/dev/null && { desktop-file-validate $W/xdg/applications/io.github.dhwan0112.haetty.desktop && ok "desktop-file-validate" || bad "desktop-file-validate" "see above"; }

# 9. Reattach after a crashed window (DESIGN.md §3.8): a fake `claude` logs every start with its arguments.
#    Kill -9 the GUI -> restart: running sessions are reattached, nothing restarted, no --resume; with an explicit
#    program one new tab; daemon gone too -> restored from the tab set with --resume once per conversation;
#    palette "attach detached".
FC=$W/fakebin; mkdir -p $FC $W/ccfg
printf '#!/bin/sh\necho "$$ $*" >> %s/claude-starts.log\nexec sleep 600\n' "$W" > $FC/claude; chmod +x $FC/claude
: > $W/claude-starts.log
export CLAUDE_CONFIG_DIR=$W/ccfg
info() { HAETTY_LOG=warn $BIN status --json | python3 -Ic '
import json,sys
s=[x for x in json.load(sys.stdin)["sessions"] if x["state"]=="running"]
print(len(s), sum(1 for x in s if x["attached"]>0), " ".join(x["claude_session"] or "-" for x in s))'; }
starts() { wc -l < $W/claude-starts.log; }
resumes() { grep -o -- "--resume [0-9a-f-]*" $W/claude-starts.log | sort | uniq -c | awk '{print $1"x"$3}' | tr '\n' ' '; }
haetty_stop_daemon "$BIN"; sleep 0.5
haetty_guard $HAETTY_STATE_DIR/tabs; rm -rf $HAETTY_STATE_DIR/tabs   # a fresh start: no tab set from the sections before
mkdir -p $W/proj9; cd $W/proj9
KEY=$(python3 -Ic "import sys; print(''.join(c if c.isalnum() else '-' for c in sys.argv[1]))" "$W/proj9")
mkdir -p "$W/ccfg/projects/$KEY"; touch "$W/ccfg/projects/$KEY/aaaa-1111.jsonl" "$W/ccfg/projects/$KEY/bbbb-2222.jsonl"
RESTORE=1 gui r1 $FC/claude; sleep 1.5; focus $GW; xdotool key ctrl+shift+t
for i in $(seq 1 50); do
  ids=($(HAETTY_LOG=warn $BIN status --json | python3 -Ic 'import json,sys; print(" ".join(x["id"] for x in json.load(sys.stdin)["sessions"]))'))
  [ ${#ids[@]} -ge 2 ] && break; sleep 0.2
done
echo "  sessions: ${ids[*]}"
echo '{"session_id":"aaaa-1111"}' | HAETTY_SESSION_ID=${ids[0]} $BIN statusline
echo '{"session_id":"bbbb-2222"}' | HAETTY_SESSION_ID=${ids[1]} $BIN statusline
sleep 1
grep -q "aaaa-1111" $HAETTY_STATE_DIR/tabs/last.tsv && grep -q "bbbb-2222" $HAETTY_STATE_DIR/tabs/last.tsv && ok "tab set has both conversation ids" || bad "last.tsv" "$(cat $HAETTY_STATE_DIR/tabs/last.tsv)"
echo "  before the crash: sessions/attached/ids = $(info); claude starts $(starts)"
kill -9 $GP; wait $GP 2>/dev/null; sleep 0.5
echo "  GUI killed: $(info)"
# Restart without a program: reattach both, open nothing new.
RESTORE=1 gui r2 ; sleep 2
I=$(info); echo "  restart (no program): $I; claude starts $(starts); resumes: [$(resumes)]"
[ "$I" = "2 2 aaaa-1111 bbbb-2222" ] && [ "$(starts)" = 2 ] && [ -z "$(resumes)" ] && ok "reattached 2 tabs, no Claude started, no --resume" || bad "reattach" "$I starts=$(starts) [$(resumes)]"
grep -q "reattaching session" $OUT/r2.log && ok "GUI log: reattaching" || bad "reattach log" "-"
kill -9 $GP; wait $GP 2>/dev/null; sleep 0.5
# Restart with an explicit program: reattach both + one new tab.
RESTORE=1 gui r3 $FC/claude; sleep 2
I=$(info); echo "  restart (explicit program): $I; claude starts $(starts); resumes: [$(resumes)]"
[ "${I%% *}" = 3 ] && [ "$(echo $I | cut -d' ' -f2)" = 3 ] && [ "$(starts)" = 3 ] && [ -z "$(resumes)" ] && ok "explicit program: 2 reattached + 1 new, no --resume" || bad "explicit" "$I starts=$(starts)"
# The daemon dies too: restore from the tab set, each conversation resumed once.
kill -9 $GP; wait $GP 2>/dev/null; kids=$(session_pids); haetty_stop_daemon "$BIN" KILL; for p in $kids; do kill $p 2>/dev/null; done; sleep 0.5
: > $W/claude-starts.log
RESTORE=1 gui r4 ; sleep 3
I=$(info); R=$(resumes); echo "  daemon gone, restart: $I; claude starts $(starts); resumes: [$R]"
[ "$R" = "1xaaaa-1111 1xbbbb-2222 " ] && [ "${I%% *}" = 3 ] && ok "restored 3 tabs, each conversation resumed exactly once" || bad "restore" "$I [$R]"
# Palette: attach a session whose window was killed.
W4=$GW; G4=$GP
gui r5 sh -c 'exec sleep 601'; sleep 1; kill -9 $GP; wait $GP 2>/dev/null; sleep 0.5
GP=$G4; GW=$W4; focus $GW; xdotool key ctrl+shift+p; sleep 0.3; xdotool type --delay 30 'attach det'; xdotool key Return; sleep 1.5
I=$(info); echo "  palette attach: $I"
[ "${I%% *}" = 4 ] && [ "$(echo $I | cut -d' ' -f2)" = 4 ] && ok "palette 'Attach detached sessions' took the orphan" || bad "palette attach" "$I"
kids=$(session_pids); stop; haetty_stop_daemon "$BIN"; for p in $kids; do kill $p 2>/dev/null; done; unset CLAUDE_CONFIG_DIR; cd /

# 6. Frame dumps: Hangul two-cell, symbols monochrome first (looked at by hand).
HAETTY_FONT_DIR=${HAETTY_E2E_FONT_DIR:-} bash "$E2E/dump-sample.sh" $BIN $OUT/hangul-emoji 14 > /dev/null
echo "dump: $OUT/hangul-emoji.png"
echo "TOTAL $TAG: $pass passed, $fail failed"
haetty_stop_daemon "$BIN"
