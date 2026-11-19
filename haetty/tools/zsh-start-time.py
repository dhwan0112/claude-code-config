#!/usr/bin/env python3
# zsh to the first prompt with and without the shell integration (stage PI): 200 runs each, interleaved, in a PTY,
# in an isolated home under DIR (a .zshrc that puts ~/.local/bin first). Writes only in DIR.
# usage: python3 -I tools/zsh-start-time.py DIR crates/haetty-cc/src/shell/zshenv.zsh
import os, pty, time, sys, statistics, shutil
base = sys.argv[1]
home = os.path.join(base, "home"); zd = os.path.join(base, "rt/zsh"); proxy = os.path.join(base, "rt/bin")
os.makedirs(os.path.join(home, ".local/bin"), exist_ok=True); os.makedirs(zd, exist_ok=True); os.makedirs(proxy, exist_ok=True)
shutil.copy(sys.argv[2], os.path.join(zd, ".zshenv"))
open(os.path.join(home, ".zshenv"), "w").write("")
open(os.path.join(home, ".zshrc"), "w").write('PATH=$HOME/.local/bin:$PATH\nPS1="READY%% "\n')
def once(integ):
    env = {"HOME": home, "TERM": "xterm-256color", "PATH": proxy + ":/usr/bin:/bin", "HAETTY_PROXY_DIR": proxy}
    if integ: env["ZDOTDIR"] = zd
    t = time.perf_counter()
    pid, fd = pty.fork()
    if pid == 0:
        os.execve("/usr/bin/zsh", ["zsh"], env)
    buf = b""
    while b"READY" not in buf:
        buf += os.read(fd, 4096)
    dt = time.perf_counter() - t
    os.write(fd, b"exit\r")
    try:
        while os.read(fd, 4096): pass
    except OSError: pass
    os.waitpid(pid, 0); os.close(fd)
    return dt * 1000
for _ in range(10): once(True); once(False)
a, b = [], []
for _ in range(200):
    a.append(once(False)); b.append(once(True))
print("off: median %.2f ms mean %.2f" % (statistics.median(a), statistics.mean(a)))
print("on:  median %.2f ms mean %.2f" % (statistics.median(b), statistics.mean(b)))
print("diff median %.2f ms" % (statistics.median(b) - statistics.median(a)))
