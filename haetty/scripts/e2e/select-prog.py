# A program for select.sh: the alternate screen with mouse reporting (SGR), as vim or Claude Code's full-screen
# renderer would have it. Every byte it reads goes to DIR/mouse.bin; when DIR/rewrite appears it redraws its first
# row; DIR/quit ends it.
# usage: python3 -I select-prog.py DIR
import os
import sys
import termios
import time
import tty

d = sys.argv[1]
fd = sys.stdin.fileno()
old = termios.tcgetattr(fd)
tty.setraw(fd)
os.set_blocking(fd, False)
out = sys.stdout
out.write("\x1b[?1049h\x1b[H\x1b[2J\x1b[?1000h\x1b[?1002h\x1b[?1006h")
out.write("ALT one two three\r\nMOUSE READY\r\n")
out.flush()
rewritten = False
try:
    with open(os.path.join(d, "mouse.bin"), "ab") as log:
        while not os.path.exists(os.path.join(d, "quit")):
            try:
                b = os.read(fd, 4096)
            except BlockingIOError:
                b = b""
            if b:
                log.write(b)
                log.flush()
            if not rewritten and os.path.exists(os.path.join(d, "rewrite")):
                out.write("\x1b[HALT changed row \x1b[K")
                out.flush()
                rewritten = True
            time.sleep(0.02)
finally:
    out.write("\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?1049l")
    out.flush()
    termios.tcsetattr(fd, termios.TCSADRAIN, old)
