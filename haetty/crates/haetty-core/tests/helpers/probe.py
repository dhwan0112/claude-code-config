#!/usr/bin/env python3
"""Terminal capability probe used by ccterm's integration tests.

Modes:
  queries      send the queries Claude Code and other TUIs use at start-up and
               print each reply (escaped) on its own line: NAME=REPLY
  echo-input   put the tty in raw mode, enable the modes given as arguments
               (e.g. 1006 1003 2004 1004 kitty=1), print READY, then print
               every byte received, escaped, one read per line, until 'q'.
"""
import os, select, sys, termios, tty

def esc(b):
    out = []
    for c in b:
        if c == 0x1b: out.append("\\e")
        elif c == 0x0d: out.append("\\r")
        elif c == 0x0a: out.append("\\n")
        elif c == 0x07: out.append("\\a")
        elif 0x20 <= c < 0x7f: out.append(chr(c))
        else: out.append("\\x%02x" % c)
    return "".join(out)

def read_reply(fd, timeout=1.0, until=None):
    buf = b""
    while True:
        r, _, _ = select.select([fd], [], [], timeout)
        if not r:
            return buf
        buf += os.read(fd, 4096)
        if until and until(buf):
            return buf
        timeout = 0.15

def emit(s):
    sys.stdout.write(s + "\r\n")
    sys.stdout.flush()

def queries(fd):
    qs = [
        ("DA1", b"\x1b[c", lambda b: b.endswith(b"c")),
        ("DA2", b"\x1b[>c", lambda b: b.endswith(b"c")),
        ("XTVERSION", b"\x1b[>0q", lambda b: b.endswith(b"\x1b\\")),
        ("DECRQM_2026", b"\x1b[?2026$p", lambda b: b.endswith(b"y")),
        ("DECRQM_2004", b"\x1b[?2004$p", lambda b: b.endswith(b"y")),
        ("DECRQM_1006", b"\x1b[?1006$p", lambda b: b.endswith(b"y")),
        ("KITTY_Q", b"\x1b[?u", lambda b: b.endswith(b"u")),
        ("CPR", b"\x1b[6n", lambda b: b.endswith(b"R")),
        ("SIZE_CELLS", b"\x1b[18t", lambda b: b.endswith(b"t")),
        ("SIZE_PX", b"\x1b[14t", lambda b: b.endswith(b"t")),
        ("CELL_PX", b"\x1b[16t", lambda b: b.endswith(b"t")),
        ("COLOR_SCHEME", b"\x1b[?996n", lambda b: b.endswith(b"n")),
        ("OSC10_FG", b"\x1b]10;?\x1b\\", lambda b: b.endswith(b"\x1b\\") or b.endswith(b"\x07")),
        ("OSC11_BG", b"\x1b]11;?\x1b\\", lambda b: b.endswith(b"\x1b\\") or b.endswith(b"\x07")),
    ]
    for name, q, done in qs:
        os.write(1, q)
        emit("%s=%s" % (name, esc(read_reply(fd, 1.0, done))))
    # Kitty keyboard push, then query again.
    os.write(1, b"\x1b[>1u")
    os.write(1, b"\x1b[?u")
    emit("KITTY_AFTER_PUSH=%s" % esc(read_reply(fd, 1.0, lambda b: b.endswith(b"u"))))
    os.write(1, b"\x1b[<u")

def echo_input(fd, modes):
    for m in modes:
        if m.startswith("kitty="):
            os.write(1, b"\x1b[>%su" % m.split("=")[1].encode())
        else:
            os.write(1, b"\x1b[?%sh" % m.encode())
    emit("READY")
    while True:
        r, _, _ = select.select([fd], [], [], 20)
        if not r:
            emit("TIMEOUT")
            return
        data = os.read(fd, 4096)
        emit("GOT " + esc(data))
        if data in (b"q", b"\x1b[113u"):
            return

def main():
    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    tty.setraw(fd)
    try:
        if sys.argv[1] == "queries":
            queries(fd)
        else:
            echo_input(fd, sys.argv[2:])
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old)
    emit("DONE")

main()
