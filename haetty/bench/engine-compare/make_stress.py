"""Write the 16.9 MB synthetic stream used in docs/DESIGN.md §4 to stress.bin."""
import random
random.seed(1)
words = ["alpha", "beta", "감마", "델타", "epsilon", "│", "─", "╭", "╮", "✔", "❯", "日本語", "emoji😀", "ok"]
out = []
for i in range(200000):
    c = random.choice([31, 32, 33, 34, 35, 36, 37, 90])
    tc = f"\x1b[38;2;{random.randrange(256)};{random.randrange(256)};{random.randrange(256)}m"
    line = " ".join(random.choice(words) for _ in range(random.randrange(3, 14)))
    out.append(f"\x1b[{c}m{i:6d}\x1b[0m {tc}{line}\x1b[0m\r\n")
    if i % 500 == 0:
        out.append("\x1b[?2026h\x1b[H\x1b[2J\x1b[1;1H" + "x" * 80 + "\x1b[?2026l")
open("stress.bin", "w").write("".join(out))
