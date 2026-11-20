# usage: ide-client.py WORKDIR FILE — Claude Code's side of the editor connection, as Claude Code 2.1.329 speaks it
# (DESIGN.md §13.9), for scripts/e2e/ide.sh: connects to $CLAUDE_CODE_SSE_PORT with the lock file's token, then for
# each step N = 1, 2, 3 waits for WORKDIR/stepN, sends openDiff on FILE and writes the answer's texts to
# WORKDIR/answerN; step 3's answer is Claude Code's close_tab after WORKDIR/close. Logs to WORKDIR/client.log, its PID in WORKDIR/client.pid.
import base64, json, os, socket, struct, sys, time

work, path = sys.argv[1], sys.argv[2]
log = open(os.path.join(work, "client.log"), "a", buffering=1)
open(os.path.join(work, "client.pid"), "w").write(str(os.getpid()))
port = os.environ["CLAUDE_CODE_SSE_PORT"]
lock = json.load(open(os.path.join(os.environ["CLAUDE_CONFIG_DIR"], "ide", port + ".lock")))
s = socket.create_connection(("127.0.0.1", int(port)))
key = base64.b64encode(os.urandom(16)).decode()
s.sendall((f"GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
           f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: mcp\r\n"
           f"X-Claude-Code-Ide-Authorization: {lock['authToken']}\r\n\r\n").encode())
head = b""
while not head.endswith(b"\r\n\r\n"):
    head += s.recv(1)
log.write(head.split(b"\r\n")[0].decode() + "\n")


def send(obj):
    p = json.dumps(obj).encode()
    m = os.urandom(4)
    h = bytes([0x81]) + (bytes([0x80 | len(p)]) if len(p) < 126 else bytes([0x80 | 126]) + struct.pack(">H", len(p)))
    s.sendall(h + m + bytes(b ^ m[i % 4] for i, b in enumerate(p)))


def exact(n):
    b = b""
    while len(b) < n:
        c = s.recv(n - len(b))
        if not c:
            raise EOFError
        b += c
    return b


def recv():
    h = exact(2)
    n = h[1] & 0x7F
    if n == 126:
        n = struct.unpack(">H", exact(2))[0]
    elif n == 127:
        n = struct.unpack(">Q", exact(8))[0]
    return json.loads(exact(n))


def wait(name):
    while not os.path.exists(os.path.join(work, name)):
        time.sleep(0.05)


send({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-11-25"}})
log.write("initialize: %s\n" % recv()["result"]["protocolVersion"])
send({"jsonrpc": "2.0", "method": "notifications/initialized"})
for step in (1, 2, 3):
    wait("step%d" % step)
    tab = "✻ [Claude Code] %s (step%d) ⧉" % (os.path.basename(path), step)
    new = open(path).read().replace("one", "two %d" % step)
    send({"jsonrpc": "2.0", "id": 10 + step, "method": "tools/call", "params": {"name": "openDiff", "arguments": {
        "old_file_path": path, "new_file_path": path, "new_file_contents": new, "tab_name": tab}}})
    if step == 3:
        wait("close")
        send({"jsonrpc": "2.0", "id": 99, "method": "tools/call", "params": {"name": "close_tab", "arguments": {"tab_name": tab}}})
    r = recv()
    texts = [c["text"] for c in r["result"]["content"]]
    with open(os.path.join(work, "answer%d.new" % step), "w") as f:
        f.write("%d %s\n" % (r["id"], texts[0]))
    os.rename(os.path.join(work, "answer%d.new" % step), os.path.join(work, "answer%d" % step))
    log.write("step %d: %s\n" % (step, texts[0]))
time.sleep(600)
