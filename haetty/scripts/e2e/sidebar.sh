#!/usr/bin/env bash
# Inside xvfb.sh: the sidebar and the claude proxy (stage PH, H1 and H2; DESIGN.md §13). A shell tab in a wide window;
# `claude` typed in it runs through the proxy (a fake claude that records its arguments and sends recorded-shape
# hooks: SessionStart, a prompt, five agents of which one nested and one in the background, a question of one agent),
# the tab becomes a Claude Code tab with the sidebar's Agents, `haetty agents` lists them, the grid never changes, a
# `q` ends it and the tab is a shell again; the tab set keeps the conversation and a restart resumes it in the shell.
# A narrow window: the sidebar over the terminal (Ctrl+Shift+S), an agent's record (Enter). Idle CPU with five agents
# running. Frame dumps (docs/look/ph-*) when a PNG folder is given. Writes only in $HAETTY_RUN.
# usage: sidebar.sh [PNG-DIR]
. "$(dirname "$0")/lib.sh"
OUT=${1:-}
W=$HAETTY_RUN/sidebarrun; haetty_guard $W; rm -rf $W; mkdir -p $W/rt $W/xrt $W/bin $W/home $W/claude $W/state $W/cfg $W/work
chmod 700 $W/rt $W/xrt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg XDG_RUNTIME_DIR=$W/xrt
export HAETTY_LOG=debug HAETTY_LOG_FILE=$W/gui.log HAETTY_DUMP_FRAME=$W/f.ppm CLAUDE_CONFIG_DIR=$W/claude HOME=$W/home
export PATH=$W/bin:$PATH SHELL=/bin/sh ENV= PS1='$ '
unset $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_[A-Z_]*SESSION[A-Z_]*)=' | tr -d =) 2>/dev/null
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
SID=0f0fc5c9-9b5a-4538-81fb-def23b2a894b
KEY=$(echo "$W/work" | sed 's/[^A-Za-z0-9]/-/g')
TP=$W/claude/projects/$KEY/$SID.jsonl
mkdir -p $W/claude/projects/$KEY/$SID/subagents
echo '{"type":"user","message":{"content":"hi"}}' > $TP
cat > $W/claude/projects/$KEY/$SID/subagents/agent-a1.jsonl <<'EOF'
{"type":"user","timestamp":"2026-11-17T12:00:00.000Z","message":{"role":"user","content":"List the files of the parser crate"}}
{"type":"assistant","timestamp":"2026-11-17T12:00:02.000Z","message":{"content":[{"type":"tool_use","name":"Glob","input":{"pattern":"src/**/*.rs"}}],"usage":{"input_tokens":3,"cache_read_input_tokens":14000,"output_tokens":120}}}
{"type":"assistant","timestamp":"2026-11-17T12:00:04.000Z","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/w/src/parser.rs"}}],"usage":{"input_tokens":2,"output_tokens":80}}}
{"type":"assistant","timestamp":"2026-11-17T12:00:09.000Z","message":{"content":[{"type":"text","text":"The parser crate has 4 files; parser.rs holds the loop."}],"usage":{"input_tokens":1,"output_tokens":40}}}
EOF
printf 'observe-new-tabs = true\n' > $W/cfg/config
# The fake real claude: its arguments to a file, then hooks in Claude Code's order, in steps (files), then waits for a line.
cat > $W/bin/claude <<EOF
#!/bin/sh
printf '%s\n' "\$@" > $W/claude-args
h() { printf '%s' "\$2" | $BIN hook \$1 > /dev/null 2>&1; }
base='"session_id":"$SID","transcript_path":"$TP","cwd":"$W/work"'
h SessionStart "{\$base,\"source\":\"startup\",\"model\":\"claude-haiku-5-5\"}"
h UserPromptSubmit "{\$base}"
echo "fake claude ready"
while [ ! -e $W/step2 ]; do sleep 0.1; done
for a in a1:Explore a2:general-purpose a4:Plan a5:Explore; do
  id=\${a%%:*}; ty=\${a#*:}
  h PreToolUse "{\$base,\"tool_name\":\"Agent\",\"tool_use_id\":\"t\$id\",\"tool_input\":{\"subagent_type\":\"\$ty\"}}"
  h SubagentStart "{\$base,\"agent_id\":\"\$id\",\"agent_type\":\"\$ty\"}"
  bg=completed; [ \$id = a5 ] && bg=async_launched
  h PostToolUse "{\$base,\"tool_name\":\"Agent\",\"tool_use_id\":\"t\$id\",\"tool_response\":{\"status\":\"\$bg\",\"agentId\":\"\$id\",\"resolvedModel\":\"claude-haiku-5-5\"}}"
done
h PreToolUse "{\$base,\"agent_id\":\"a2\",\"agent_type\":\"general-purpose\",\"tool_name\":\"Agent\",\"tool_use_id\":\"ta3\",\"tool_input\":{\"subagent_type\":\"Explore\",\"isolation\":\"worktree\"}}"
h SubagentStart "{\$base,\"agent_id\":\"a3\",\"agent_type\":\"Explore\"}"
h PostToolUse "{\$base,\"agent_id\":\"a2\",\"agent_type\":\"general-purpose\",\"tool_name\":\"Agent\",\"tool_use_id\":\"ta3\",\"tool_response\":{\"status\":\"async_launched\",\"agentId\":\"a3\",\"resolvedModel\":\"claude-sonnet-5-5\"}}"
h PreToolUse "{\$base,\"agent_id\":\"a1\",\"agent_type\":\"Explore\",\"tool_name\":\"Read\",\"tool_input\":{\"file_path\":\"$W/work/src/very/long/folder/parser.rs\"}}"
h PreToolUse "{\$base,\"agent_id\":\"a3\",\"agent_type\":\"Explore\",\"tool_name\":\"Grep\",\"tool_input\":{\"pattern\":\"fn parse\"}}"
h PostToolUse "{\$base,\"agent_id\":\"a4\",\"agent_type\":\"Plan\",\"tool_name\":\"Edit\",\"tool_input\":{\"file_path\":\"$W/work/src/lib.rs\",\"old_string\":\"a\",\"new_string\":\"b\"}}"
echo "agents running"
while [ ! -e $W/step3 ]; do sleep 0.1; done
h PermissionRequest "{\$base,\"agent_id\":\"a3\",\"agent_type\":\"Explore\",\"tool_name\":\"Bash\"}"
echo "asking"
read line
h SessionEnd "{\$base,\"reason\":\"prompt_input_exit\"}"
echo "fake claude ended"
EOF
chmod +x $W/bin/claude
J() { HAETTY_LOG=warn $BIN status --json; }
sess() { J | python3 -Ic "import json,sys; s=json.load(sys.stdin)['sessions']; print(s[-1]['$1'] if s else '')"; }
key() { xdotool key --clearmodifiers "$@"; sleep 0.5; }
typ() { xdotool type --delay 20 "$1"; sleep 0.2; key Return; }
shot() { [ -n "$OUT" ] && sleep 0.8 && pnmtopng $W/f.ppm > "$OUT/ph-$1.png" 2> /dev/null; }
waitfor() { for i in $(seq 1 50); do grep -q "$1" "$2" 2>/dev/null && return 0; sleep 0.2; done; return 1; }
cd $W/work
start() {
  setsid $BIN "$@" > /dev/null 2>&1 < /dev/null & GP=$!
  GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2
  for i in 1 2 3 4 5 6 7 8 9 10; do xdotool windowfocus --sync $GW 2>/dev/null; [ "$(xdotool getwindowfocus)" = "$GW" ] && break; sleep 0.3; done
}
stop() { kill $GP 2>/dev/null; sleep 0.5; haetty_stop_daemon "$BIN"; sleep 0.6; }
cpu() { # ticks of PID over $2 seconds -> percent of one core
  local a b; a=$(awk '{print $14+$15}' /proc/$1/stat); sleep $2; b=$(awk '{print $14+$15}' /proc/$1/stat)
  python3 -Ic "print(round(($b-$a)*100/$(getconf CLK_TCK)/$2, 2))"
}

# --- A wide window: a shell tab with the sidebar ------------------------------------------------------------------
start --cols 130 --rows 30
cols0=$(sess cols); rows0=$(sess rows); [ -n "$cols0" ] && [ "$cols0" -le 90 ] && ok "a shell tab with the sidebar docked: $cols0 terminal columns of 130" || bad "docked" "cols=$cols0"
shot 4-shell-tab
typ "claude --model haiku"
sleep 1.5
grep -qx -- "--settings" $W/claude-args && grep -qx -- "--model" $W/claude-args && ok "the proxy added --settings and kept --model haiku" || bad "args" "$(tr '\n' ' ' < $W/claude-args 2>/dev/null)"
[ "$(sess claude_session)" = "$SID" ] && ok "the shell tab is a Claude Code tab at SessionStart" || bad "claude tab" "$(sess claude_session)"
[ "$(sess cols)x$(sess rows)" = "${cols0}x$rows0" ] && ok "the grid is unchanged (${cols0}x$rows0)" || bad "grid" "$(sess cols)x$(sess rows)"
shot 1-claude-no-agents
touch $W/step2; sleep 1.5
A=$(HAETTY_LOG=warn $BIN agents --json)
echo "$A" | python3 -Ic "
import json,sys; d=json.load(sys.stdin)['sessions'][0]['agents']
ids={a['id']:a for a in d}
assert len(d)==5, d
assert ids['a3']['parent']=='a2' and ids['a3']['depth']==2 and ids['a3']['worktree'] and ids['a3']['model']=='claude-sonnet-5-5', ids['a3']
assert ids['a5']['state']=='background' and ids['a1']['tool']=='Read' and ids['a1']['target']=='parser.rs'
" 2>&1 && ok "haetty agents: 5 agents, a3 nested under a2 (worktree, sonnet), a5 in the background" || bad "agents" "$A"
T=$(HAETTY_LOG=warn $BIN agents); echo "$T" | grep -q "◌ Explore haiku" && ok "haetty agents (text): $(echo "$T" | wc -l) lines" || bad "agents text" "$T"
[ "$(sess cols)x$(sess rows)" = "${cols0}x$rows0" ] && ok "agents starting changed no grid" || bad "grid" "$(sess cols)x$(sess rows)"
shot 2-claude-agents-nested
# Idle CPU with five agents running (the sidebar redraws their elapsed times once a second).
c_gui=$(cpu $GP 10)
DP=$(J | sed -n 's/.*"daemon_pid":\([0-9]*\).*/\1/p'); c_d=$(cpu $DP 5)
echo "INFO idle CPU with 5 agents running: GUI ${c_gui}% (10 s), daemon ${c_d}% (5 s)"
python3 -Ic "import sys; sys.exit(0 if $c_gui < 3.0 else 1)" && ok "GUI idle CPU ${c_gui}% < 3% with agents running" || bad "idle CPU" "$c_gui%"
touch $W/step3; sleep 1.2
[ "$(sess attention)" = Ask ] && ok "an agent's question: the tab asks (attention Ask)" || bad "ask" "$(sess attention)"
shot 3-claude-agent-asks
# Back to a shell: the fake reads a line and ends.
typ "q"; sleep 1.2
[ -z "$(sess claude_session)" ] && ok "claude ended: a shell tab again" || bad "shell again" "$(sess claude_session)"
[ "$(sess cols)x$(sess rows)" = "${cols0}x$rows0" ] && ok "back to a shell: the grid unchanged" || bad "grid" "$(sess cols)x$(sess rows)"
shot 5-back-to-shell
# The tab set keeps a running conversation: start it again, leave it running, restart haetty.
rm -f $W/step2 $W/step3
typ "claude"; sleep 1.5
stop
rm -f $W/claude-args
start --cols 130 --rows 30
for i in $(seq 1 30); do [ -s $W/claude-args ] && break; sleep 0.2; done
grep -qx -- "--resume" $W/claude-args && grep -qx -- "$SID" $W/claude-args && ok "restored: the shell ran claude --resume $SID" || bad "restore" "$(tr '\n' ' ' < $W/claude-args 2>/dev/null)"
grep -qx -- "--settings" $W/claude-args && ok "the resumed claude went through the proxy too" || bad "resume proxy" "-"
touch $W/step2; sleep 1
stop

# --- A narrow window: the sidebar over the terminal, an agent's record ----------------------------------------------
rm -f $W/step2 $W/step3 $W/state/tabs/last.tsv
start --cols 100 --rows 30 --restore-tabs false
c0=$(sess cols); [ "$c0" = 100 ] && ok "narrow: no sidebar, the terminal has all 100 columns" || bad "narrow" "$c0"
typ "claude"; sleep 1.2; touch $W/step2; sleep 1.2
key ctrl+shift+s
[ "$(sess cols)" = 100 ] && ok "the sidebar over the terminal: the grid stays 100 columns" || bad "overlay grid" "$(sess cols)"
shot 6-narrow-overlay
key Down; key Return; sleep 1
shot 7-agent-record
key Escape
key ctrl+shift+s
stop
# Themes: the wide Claude Code tab with agents in the two other bundled themes.
for th in "haetty classic" "IBM 5153 CGA (Black)"; do
  rm -f $W/step2 $W/step3 $W/state/tabs/last.tsv
  printf 'observe-new-tabs = true\ntheme = %s\n' "$th" > $W/cfg/config
  start --cols 130 --rows 30 --restore-tabs false
  typ "claude"; sleep 1.2; touch $W/step2; sleep 1.2
  shot "8-theme-$(echo "$th" | tr 'A-Z ()' 'a-z-' | tr -s - | sed 's/-$//')"
  stop
done
echo "TOTAL sidebar.sh: $P passed, $F failed"
[ $F = 0 ]
