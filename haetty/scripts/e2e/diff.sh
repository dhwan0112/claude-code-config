#!/usr/bin/env bash
# Inside xvfb.sh: the diff overlay (stage PM, P3b; DESIGN.md §13.4). A Claude Code tab (a fake claude sending the hooks
# of two edits in a git repository) in a wide window with the sidebar docked: a click on a file of the sidebar's Files
# opens the overlay over the terminal (side by side from diff-split-cols), the keys n/p ]/[ v m j/k move and switch,
# Esc closes and the frame is the one before; the grid never changes. diff-after-edit opens it on an edit by itself.
# A narrow window: the sidebar over the terminal, Enter on a file; top and bottom. Frame dumps (docs/look/p3b-*) when
# a PNG folder is given. Writes only in $HAETTY_RUN.
# usage: diff.sh [PNG-DIR]
. "$(dirname "$0")/lib.sh"
OUT=${1:-}
W=$HAETTY_RUN/diffrun; haetty_guard $W; rm -rf $W; mkdir -p $W/rt $W/xrt $W/bin $W/home $W/claude $W/state $W/cfg $W/work/src
chmod 700 $W/rt $W/xrt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg XDG_RUNTIME_DIR=$W/xrt
export HAETTY_LOG=debug HAETTY_LOG_FILE=$W/gui.log HAETTY_DUMP_FRAME=$W/f.ppm CLAUDE_CONFIG_DIR=$W/claude HOME=$W/home
export PATH=$W/bin:$PATH SHELL=/bin/sh ENV= PS1='$ ' GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
unset $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_[A-Z_]*SESSION[A-Z_]*)=' | tr -d =) 2>/dev/null
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
SID=4d1f0c55-6a43-4b5e-9a0e-3f1c2b7d8e90
R=$W/work; G="git -C $R -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false"
$G init -q -b main
for i in $(seq 1 40); do echo "fn f$i() -> u32 { $i }"; done > $R/src/main.rs
for i in $(seq 1 30); do echo "pub const C$i: u32 = $i;"; done > $R/src/lib.rs
for i in $(seq 1 60); do echo "pub fn u$i() {}"; done > $R/src/util.rs
$G add .; $G commit -qm base
KEY=$(echo "$R" | sed 's/[^A-Za-z0-9]/-/g'); TP=$W/claude/projects/$KEY/$SID.jsonl
mkdir -p $(dirname $TP); echo '{"type":"user","message":{"content":"hi"}}' > $TP
# The fake real claude: SessionStart, a prompt, two edits (the files change between PreToolUse and PostToolUse, as
# Claude Code's Edit does), then a third edit on step2, and waits for a line.
cat > $W/bin/claude <<EOS
#!/bin/sh
h() { printf '%s' "\$2" | $BIN hook \$1 > /dev/null 2>&1; }
base='"session_id":"$SID","transcript_path":"$TP","cwd":"$R"'
edit() { # file, old text, new text, id: Claude Code's Edit (the file changes between PreToolUse and PostToolUse)
  in="{\"file_path\":\"$R/\$1\",\"old_string\":\"\$2\",\"new_string\":\"\$3\"}"
  h PreToolUse "{\$base,\"tool_name\":\"Edit\",\"tool_use_id\":\"e\$4\",\"tool_input\":\$in}"
  sed -i "s/\$2/\$3/" "$R/\$1"
  h PostToolUse "{\$base,\"tool_name\":\"Edit\",\"tool_use_id\":\"e\$4\",\"tool_input\":\$in,\"tool_response\":{}}"
}
h SessionStart "{\$base,\"source\":\"startup\",\"model\":\"claude-sonnet-5-5\"}"
h UserPromptSubmit "{\$base}"
edit src/util.rs "u4() {}" "u4() { u3() }" 6
edit src/util.rs "u30() {}" "u30() { u29() }" 7
edit src/util.rs "u55() {}" "u55() { u54() }" 8
edit src/lib.rs "C3: u32 = 3;" "C3: u32 = 33;" 1
edit src/lib.rs "C20: u32 = 20;" "C20: u64 = 20;" 2
edit src/main.rs "f5() -> u32 { 5 }" "f5() -> u32 { 50 }" 3
edit src/main.rs "f30() -> u32 { 30 }" "f30() -> u64 { 30 }" 4
h Stop "{\$base,\"last_assistant_message\":\"done\"}"
echo "fake claude: two edits"
while [ ! -e $W/step2 ]; do sleep 0.1; done
h UserPromptSubmit "{\$base}"
edit src/lib.rs "C10: u32 = 10;" "C10: u32 = 100;" 5
h Stop "{\$base,\"last_assistant_message\":\"done\"}"
echo "fake claude: third edit"
read line
h SessionEnd "{\$base,\"reason\":\"prompt_input_exit\"}"
EOS
chmod +x $W/bin/claude
J() { HAETTY_LOG=warn $BIN status --json; }
sess() { J | python3 -Ic "import json,sys; s=json.load(sys.stdin)['sessions']; print(s[-1]['$1'] if s else '')"; }
key() { xdotool key --clearmodifiers "$@"; sleep 0.5; }
typ() { xdotool type --delay 20 "$1"; sleep 0.2; key Return; }
shot() { [ -n "$OUT" ] && sleep 0.8 && pnmtopng $W/f.ppm > "$OUT/p3b-$1.png" 2> /dev/null; }
frame() { sleep 0.8; cp $W/f.ppm $W/$1.ppm; }
ng() { python3 -I "$E2E/nongrey.py" $W/f.ppm; }
start() {
  setsid $BIN "$@" > /dev/null 2>&1 < /dev/null & GP=$!
  GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2
  for i in 1 2 3 4 5 6 7 8 9 10; do xdotool windowfocus --sync $GW 2>/dev/null; [ "$(xdotool getwindowfocus)" = "$GW" ] && break; sleep 0.3; done
  eval $(xdotool getwindowgeometry --shell $GW)
}
stop() { kill $GP 2>/dev/null; sleep 0.5; haetty_stop_daemon "$BIN"; sleep 0.6; }
cd $R
cells() { # the window's cell grid, as links.sh computes it
  CW=$(( (WIDTH - 20) / $1 )); CH=$(( (HEIGHT - 12) / $2 )); OX=$(( (WIDTH - $1*CW) / 2 )); OY=$(( (HEIGHT - $2*CH) / 2 ))
}
cx() { echo $(( OX + $1*CW + CW/2 )); }; cy() { echo $(( OY + $1*CH + CH/2 )); }
click() { xdotool mousemove --window $GW $(cx $1) $(cy $2); sleep 0.3; xdotool click 1; sleep 1; }
same() { # the terminal area ($TC columns from the left) of two frames, byte for byte: the docked sidebar's ages tick
  python3 -I - $W/$1.ppm $W/$2.ppm $(( OX + TC*CW )) <<'EOP'
import sys
def rows(f, w0):
    d = open(f, "rb").read(); p = d.split(maxsplit=4); w, h = int(p[1]), int(p[2]); px = p[4]
    return [px[y*w*3:(y*w + w0)*3] for y in range(h)]
sys.exit(0 if rows(sys.argv[1], int(sys.argv[3])) == rows(sys.argv[2], int(sys.argv[3])) else 1)
EOP
}
grid() { echo "$(sess cols)x$(sess rows)"; }

# --- A wide window: the sidebar docked, the overlay side by side -------------------------------------------------
start --cols 170 --rows 34 --restore-tabs false; cells 170 34
typ "claude"; sleep 2
g0=$(grid); TC=${g0%x*}
[ "$(sess claude_session)" = "$SID" ] && ok "a Claude Code tab with edits of three files (grid $g0)" || bad "claude tab" "$(sess claude_session)"
frame before
[ "$(ng)" = 0 ] && ok "§2.3: the idle tab with the sidebar has 0 non-grey pixels" || bad "idle grey" "$(ng)"
# The sidebar: Lead (row 0), a blank row, Files (2), src/main.rs (3), src/lib.rs (4), src/util.rs (5).
click 140 4; frame split
n=$(ng); [ "$n" -gt 0 ] && ok "a click on src/lib.rs in the sidebar's Files opens the overlay (diff colours: $n px)" || bad "click open" "$n"
[ "$(grid)" = "$g0" ] && ok "the overlay open: the grid unchanged ($g0)" || bad "grid open" "$(grid)"
shot split
key v; frame stacked
same split stacked && bad "v" "the frame did not change" || ok "v: top and bottom"
shot stacked
key v; frame split2
same split split2 && ok "v again: side by side, the same frame" || bad "v back" "frames differ"
key bracketright; frame fits
same split2 fits && ok "]: one file's two hunks fit on the screen, nothing to scroll" || bad "] fits" "the view moved"
key n; frame next
same split2 next && bad "n" "the file did not change" || ok "n: the next file"
key p; frame prev
same split2 prev && ok "p: back to the first file" || bad "p" "frames differ"
key j; key k; frame jk
same split2 jk && ok "j then k: the same place" || bad "j/k" "frames differ"
key Escape; frame after
same before after && ok "Esc: the frame is exactly the one before the overlay" || bad "close" "frames differ"
[ "$(grid)" = "$g0" ] && ok "closed: the grid unchanged" || bad "grid closed" "$(grid)"
[ "$(ng)" = 0 ] && ok "closed: 0 non-grey pixels again" || bad "grey again" "$(ng)"
key ctrl+shift+d; frame last
[ "$(ng)" -gt 0 ] && ok "Ctrl+Shift+D: the last edit" || bad "ctrl+shift+d" "$(ng)"
key m; frame head
same last head && bad "m" "the view did not change" || ok "m: the next view (vs HEAD)"
key m; frame tree
same head tree && bad "m" "the view did not change" || ok "m again: the working tree (three files, taller than the screen)"
key bracketright; frame hunk2
same tree hunk2 && bad "]" "no move to the next hunk" || ok "]: the next hunk"
key bracketleft; frame hunk1
same hunk2 hunk1 && bad "[" "no move back" || ok "[: back to the previous hunk"
key g; frame top
same tree top && ok "g: the top again, the same frame" || bad "g" "frames differ"
shot tree
key Escape
# diff-after-edit: an edit opens the overlay by itself.
printf 'diff-after-edit = true\nrestore-tabs = false\n' > $W/cfg/config
key ctrl+shift+p; xdotool type --delay 20 "reload config"; sleep 0.3; key Return; sleep 0.5
touch $W/step2; sleep 2
n=$(ng); [ "$n" -gt 0 ] && ok "diff-after-edit: the third edit opened the overlay by itself ($n px)" || bad "after edit" "$n"
[ "$(grid)" = "$g0" ] && ok "after-edit: the grid unchanged" || bad "grid after-edit" "$(grid)"
shot after-edit
key Escape
grep -qE "C10|C3: u32|f5\(\)" $W/gui.log && bad "log" "file contents in the debug log" || ok "no file contents in the debug log"
stop

# --- A narrow window: the sidebar over the terminal, Enter on a file, top and bottom ------------------------------
$G checkout -q -- .; rm -f $W/step2 $W/state/tabs/last.tsv; printf 'restore-tabs = false\n' > $W/cfg/config
start --cols 90 --rows 30 --restore-tabs false; cells 90 30
typ "claude"; sleep 2
g1=$(grid); TC=${g1%x*}; [ "${g1%x*}" = 90 ] && ok "narrow: the terminal has all 90 columns ($g1)" || bad "narrow grid" "$g1"
frame nbefore
key ctrl+shift+s; key Down; key Return; frame narrow
n=$(ng); [ "$n" -gt 0 ] && ok "narrow: the sidebar over the terminal, Enter on a file opens the overlay ($n px)" || bad "narrow open" "$n"
[ "$(grid)" = "$g1" ] && ok "narrow: the grid unchanged" || bad "narrow grid open" "$(grid)"
shot narrow
key Escape; frame nafter
same nbefore nafter && ok "narrow: Esc leaves the frame as before" || bad "narrow close" "frames differ"
stop
# Themes: the side-by-side view in the two other bundled themes.
for th in "haetty classic" "IBM 5153 CGA (Black)"; do
  $G checkout -q -- .; rm -f $W/step2 $W/state/tabs/last.tsv
  printf 'restore-tabs = false\ntheme = %s\n' "$th" > $W/cfg/config
  start --cols 170 --rows 34 --restore-tabs false; cells 170 34
  typ "claude"; sleep 2; click 140 4
  shot "theme-$(echo "$th" | tr 'A-Z ()' 'a-z-' | tr -s - | sed 's/-$//')"
  stop
done
echo "TOTAL diff.sh: $P passed, $F failed"
[ $F = 0 ]
