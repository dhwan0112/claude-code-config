# Stage PD: put one recording into the replay set: the raw output and its timing, the edit tools' hook input reduced
# to what the inline diff uses, personal strings replaced by filler of the same length. usage: pack.py SRC DST COLS ROWS RENDERER
import json, os, re, sys
src, dst, cols, rows, renderer = sys.argv[1:6]
os.makedirs(dst, exist_ok=True)
raw = open(os.path.join(src, [f for f in os.listdir(src) if f.startswith("raw")][0]), "rb").read()
raw = re.sub(rb"Claude (Pro|Max|Ultra|Team|Enterprise)\b", lambda m: b"Claude X" + b"x" * (len(m.group(1)) - 1), raw)
open(os.path.join(dst, "screen.raw"), "wb").write(raw)
tim = [f for f in os.listdir(src) if f.startswith("timing")][0]
open(os.path.join(dst, "timing"), "w").write(open(os.path.join(src, tim)).read())
keep = []
for l in open(os.path.join(src, "hooks.jsonl")):
    d = json.loads(l)
    r = d.get("tool_response") or {}
    out = {k: d[k] for k in ("hook_event_name", "tool_name", "tool_use_id", "tool_input", "_t") if k in d}
    if d["hook_event_name"] == "PostToolUse":
        out["tool_response"] = {k: r[k] for k in ("type", "structuredPatch", "originalFile", "old_source", "new_source") if k in r}
    keep.append(json.dumps(out, ensure_ascii=False))
text = "\n".join(keep) + "\n"
assert "/home/" not in text
open(os.path.join(dst, "hooks.jsonl"), "w").write(text)
open(os.path.join(dst, "meta"), "w").write(f"cols {cols}\nrows {rows}\nrenderer {renderer}\nclaude {os.environ.get('CLAUDE_VERSION', '?')}\nmodel haiku\n")
assert b"/home/" not in raw, "a home path in the screen output"
print(dst, len(raw), len(text))
