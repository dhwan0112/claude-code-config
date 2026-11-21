#!/usr/bin/env bash
# Inside xvfb.sh: text selection and copy (stage PX; DESIGN.md §2.1, §3.7). A zsh tab: drag then Ctrl+Shift+C into
# the clipboard; copy-on-select into the primary selection; a wide character from either cell; double click (a word),
# triple click (the whole wrapped line), Shift+click, Alt+drag (a rectangle); Esc and typing clear it; a middle click
# pastes the primary selection; a drag onto the tab bar scrolls into the scrollback. A program with mouse reporting on
# the alternate screen gets a plain drag, not a Shift+drag, and a selection goes when it rewrites the row. A selection
# inside the diff overlay. OSC 52 still reaches the clipboard. The grid never changes while selecting. Only Xvfb's
# clipboard is used. Frame dumps (docs/look/px-real-*) when a PNG folder is given. Writes only in $HAETTY_RUN.
# usage: select.sh [PNG-DIR]
. "$(dirname "$0")/lib.sh"
OUT=${1:-}
W=$HAETTY_RUN/selrun; haetty_guard $W; rm -rf $W; mkdir -p $W/rt $W/xrt $W/bin $W/home $W/claude $W/state $W/cfg $W/work/src
chmod 700 $W/rt $W/xrt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg XDG_RUNTIME_DIR=$W/xrt
export HAETTY_LOG=debug HAETTY_LOG_FILE=$W/gui.log HAETTY_DUMP_FRAME=$W/f.ppm CLAUDE_CONFIG_DIR=$W/claude HOME=$W/home
export PATH=$W/bin:$PATH SHELL=$(command -v zsh) ENV= GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
unset HAETTY_IDE_PORT HAETTY_PROXIED HAETTY_PROXY_DIR HAETTY_CLAUDE_SETTINGS HAETTY_SESSION_ID ZDOTDIR
[ -x "$SHELL" ] || { echo "SKIP select.sh: no zsh"; exit 0; }
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
printf "PROMPT='\$ '\nunsetopt PROMPT_SP\n" > $W/home/.zshrc
printf 'restore-tabs = false\n' > $W/cfg/config
LONG="long-$(printf 'x%.0s' $(seq 1 140))-end"
printf 'alpha beta-gamma (delta) end\n한글 텍스트 abc\n%s\nrect11 rect12 rect13\nrect21 rect22 rect23\n' "$LONG" > $W/work/lines.txt
SID=7c3e1a90-2b4d-4f6a-8e1c-5d9b0a2f3e41
R=$W/work; G="git -C $R -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false"
$G init -q -b main
for i in $(seq 1 20); do echo "fn f$i() -> u32 { $i }"; done > $R/src/main.rs
$G add src; $G commit -qm base
KEY=$(echo "$R" | sed 's/[^A-Za-z0-9]/-/g'); TP=$W/claude/projects/$KEY/$SID.jsonl
mkdir -p $(dirname $TP); echo '{"type":"user","message":{"content":"hi"}}' > $TP
cat > $W/bin/claude <<EOS
#!/bin/sh
h() { printf '%s' "\$2" | $BIN hook \$1 > /dev/null 2>&1; }
base='"session_id":"$SID","transcript_path":"$TP","cwd":"$R"'
in='{"file_path":"$R/src/main.rs","old_string":"f5() -> u32 { 5 }","new_string":"f5() -> u32 { 50 }"}'
h SessionStart "{\$base,\"source\":\"startup\",\"model\":\"claude-sonnet-5-5\"}"
h UserPromptSubmit "{\$base}"
h PreToolUse "{\$base,\"tool_name\":\"Edit\",\"tool_use_id\":\"e1\",\"tool_input\":\$in}"
sed -i "s/f5() -> u32 { 5 }/f5() -> u32 { 50 }/" "$R/src/main.rs"
h PostToolUse "{\$base,\"tool_name\":\"Edit\",\"tool_use_id\":\"e1\",\"tool_input\":\$in,\"tool_response\":{}}"
h Stop "{\$base,\"last_assistant_message\":\"done\"}"
echo "fake claude: one edit"
read line
h SessionEnd "{\$base,\"reason\":\"prompt_input_exit\"}"
EOS
chmod +x $W/bin/claude
J() { HAETTY_LOG=warn $BIN status --json; }
sess() { J | python3 -Ic "import json,sys; s=json.load(sys.stdin)['sessions']; print(s[-1]['$1'] if s else '')"; }
grid() { echo "$(sess cols)x$(sess rows)"; }
key() { xdotool key --clearmodifiers "$@"; sleep 0.4; }
typ() { xdotool type --delay 15 "$1"; sleep 0.2; key Return; }
shot() { [ -n "$OUT" ] && sleep 0.8 && pnmtopng $W/f.ppm > "$OUT/px-real-$1.png" 2> /dev/null; }
clip() { xclip -o -selection "${1:-clipboard}" 2>/dev/null; }
setclip() { printf '%s' "$2" | xclip -i -selection "$1"; sleep 0.3; }
cells() { CW=$(( (WIDTH - 20) / $1 )); CH=$(( (HEIGHT - 12) / $2 )); OX=$(( (WIDTH - $1*CW) / 2 )); OY=$(( (HEIGHT - $2*CH) / 2 )); }
cx() { echo $(( OX + $1*CW + CW/2 )); }; cy() { echo $(( OY + $1*CH + CH/2 )); }
to() { xdotool mousemove --window $GW $(cx $1) $(cy $2); sleep 0.15; }
drag() { # col row col row [modifier]
  [ -n "${5:-}" ] && xdotool keydown $5
  to $1 $2; xdotool mousedown 1; sleep 0.15; to $(( ($1 + $3) / 2 )) $(( ($2 + $4) / 2 )); to $3 $4; xdotool mouseup 1
  [ -n "${5:-}" ] && xdotool keyup $5
  sleep 0.5
}
clicks() { to $1 $2; xdotool click --repeat $3 --delay 80 1; sleep 0.6; }
copy() { key ctrl+shift+c; sleep 0.3; }
resizes() { grep -c "resize session" $W/gui.log; }
start() {
  setsid $BIN "$@" > /dev/null 2>&1 < /dev/null & GP=$!
  GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2
  for i in 1 2 3 4 5 6 7 8 9 10; do xdotool windowfocus --sync $GW 2>/dev/null; [ "$(xdotool getwindowfocus)" = "$GW" ] && break; sleep 0.3; done
  eval $(xdotool getwindowgeometry --shell $GW)
}
stop() { kill $GP 2>/dev/null; sleep 0.5; haetty_stop_daemon "$BIN"; sleep 0.6; }
cd $W/work

start --cols 100 --rows 24 --restore-tabs false; cells 100 24
typ "clear; cat lines.txt"; sleep 0.8
g0=$(grid); r0=$(resizes)
setclip clipboard sentinel; setclip primary sentinel
drag 0 0 4 0; copy
[ "$(clip)" = alpha ] && ok "drag then Ctrl+Shift+C: the clipboard has \"alpha\"" || bad "drag copy" "$(clip)"
[ "$(clip primary)" = alpha ] && ok "copy-on-select: the primary selection has it too" || bad "primary" "$(clip primary)"
shot drag
drag 0 1 3 1
[ "$(clip primary)" = "한글" ] && ok "a drag over two Hangul characters gives them" || bad "hangul" "$(clip primary)"
drag 1 1 2 1
[ "$(clip primary)" = "한글" ] && ok "from the second cell of 한 to the first of 글: both whole" || bad "wide cells" "$(clip primary)"
clicks 12 0 2
[ "$(clip primary)" = "beta-gamma" ] && ok "double click: the word beta-gamma (- is a word character)" || bad "double" "$(clip primary)"
clicks 30 3 3
[ "$(clip primary)" = "$LONG" ] && ok "triple click on a wrapped line's second row: the whole line, no newline (${#LONG} chars)" || bad "triple" "$(clip primary | head -c 80)"
drag 0 0 4 0; to 9 0; xdotool keydown shift click 1 keyup shift; sleep 0.5
[ "$(clip primary)" = "alpha beta" ] && ok "Shift+click extends the selection to \"alpha beta\"" || bad "shift+click" "$(clip primary)"
drag 0 4 5 5 alt
[ "$(clip primary)" = "$(printf 'rect11\nrect21')" ] && ok "Alt+drag: a rectangle, one line per row" || bad "rect" "$(clip primary | od -c | head -3)"
shot rect
typ "(sleep 1.5; seq 1 30) &"; sleep 0.3
drag 0 0 4 0; sleep 2.5; setclip clipboard sentinel; copy
[ "$(clip)" = alpha ] && ok "output pushed the selected row up into the scrollback: it still copies \"alpha\"" || bad "follow" "$(clip)"
typ "clear; cat lines.txt"; sleep 0.8
drag 0 0 4 0; key Escape; setclip clipboard sentinel; copy
[ "$(clip)" = sentinel ] && ok "Esc clears the selection: Ctrl+Shift+C copies nothing" || bad "esc" "$(clip)"
drag 0 0 4 0; xdotool type q; sleep 0.3; key BackSpace; setclip clipboard sentinel; copy
[ "$(clip)" = sentinel ] && ok "typing clears the selection" || bad "typing" "$(clip)"
setclip clipboard sentinel; copy
[ "$(clip)" = sentinel ] && ok "Ctrl+Shift+C without a selection does nothing" || bad "no selection" "$(clip)"
drag 0 0 4 0
typ "cat > $W/paste.txt"; sleep 0.3; to 10 10; xdotool click 2; sleep 0.4; key Return; key ctrl+d
[ "$(cat $W/paste.txt 2>/dev/null)" = alpha ] && ok "a middle click pastes the primary selection" || bad "middle" "$(cat $W/paste.txt 2>&1)"
typ "printf '\\033]52;c;%s\\a' \$(printf osc52-ok | base64)"; sleep 0.5
[ "$(clip)" = osc52-ok ] && ok "OSC 52 from the program still reaches the clipboard" || bad "osc52" "$(clip)"
[ "$(grid)" = "$g0" ] && [ "$(resizes)" = "$r0" ] && ok "while selecting: the grid unchanged ($g0), 0 resize lines" || bad "resize" "$(grid) $(resizes) vs $r0"
grep -qE "alpha|beta-gamma|rect11|osc52-ok" $W/gui.log && bad "log" "selected text in the GUI log" || ok "no selected text in the GUI log"

key ctrl+shift+t; sleep 1.5
typ "clear; seq 1 400"; sleep 0.8
setclip primary sentinel
to 0 3; xdotool mousedown 1; sleep 0.2; to 0 2; to 1 0; sleep 2; to 2 0; sleep 0.3; xdotool mouseup 1; sleep 0.6
n=$(clip primary | wc -l); second=$(clip primary | sed -n 2p); penult=$(clip primary | sed -n "${n}p")
[ "$n" -gt 30 ] && [ "${second:-999}" -lt 370 ] && [ "$((penult - second))" = "$((n - 2))" ] \
  && ok "a drag onto the tab bar scrolls into the scrollback: whole lines $second to $penult in order" \
  || bad "autoscroll" "$n lines, '$second' to '$penult'"
key ctrl+shift+w; sleep 0.5; key ctrl+shift+w; sleep 1

typ "python3 -I $E2E/select-prog.py $W"; sleep 1.2
setclip primary sentinel
drag 0 0 2 0
[ -s $W/mouse.bin ] && grep -q $'\e\\[<' $W/mouse.bin && ok "mouse reporting on: a plain drag goes to the program" || bad "to the program" "$(od -c $W/mouse.bin | head -2)"
[ "$(clip primary)" = sentinel ] && ok "...and selects nothing" || bad "plain drag selected" "$(clip primary)"
s0=$(stat -c %s $W/mouse.bin)
drag 0 0 6 0 shift
[ "$(stat -c %s $W/mouse.bin)" = "$s0" ] && ok "Shift+drag: no byte reaches the program" || bad "shift to program" "$(stat -c %s $W/mouse.bin) vs $s0"
[ "$(clip primary)" = "ALT one" ] && ok "Shift+drag on the alternate screen selects \"ALT one\"" || bad "alt select" "$(clip primary)"
shot alt
touch $W/rewrite; sleep 0.8; setclip clipboard sentinel; copy
[ "$(clip)" = sentinel ] && ok "the program rewrote the selected row: the selection is gone" || bad "rewrite" "$(clip)"
touch $W/quit; sleep 0.8

typ "claude"; sleep 2
key ctrl+shift+d; sleep 1
setclip clipboard sentinel
drag 0 2 99 8; copy
clip | grep -q "f5() -> u32 { 50 }" && ok "a selection inside the diff overlay copies its text" || bad "diff" "$(clip | head -5)"
shot diff
key Escape; key Escape
[ "$(grid)" = "$g0" ] && ok "the grid is still $g0" || bad "grid end" "$(grid)"
stop
echo "select: $P passed, $F failed"
[ $F = 0 ]
