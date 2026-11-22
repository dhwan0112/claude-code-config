# Stage PD: the test repository for the real Claude Code recordings (synthetic content only). usage: mkrepo.py DIR HOOKLOG
import json, os, sys
d, hooklog = sys.argv[1], sys.argv[2]
def w(p, t):
    p = os.path.join(d, p); os.makedirs(os.path.dirname(p), exist_ok=True); open(p, "w").write(t)
calc = ['"""Small calculator helpers."""', "", "import math", "", "SCALE = 10", "", "",
        "def add(a, b):", "    # add two numbers", "    # and return the sum", "    tmp = a + b", "    return tmp", "", "",
        "def mul(a, b):", "    tmp = a * b", "    return tmp", "", "",
        "def area(r):", "    tmp = math.pi * r * r", "    return round(tmp, 2)", "", "",
        "def scale(x):", "    return x * SCALE", "", "",
        "def main():", "    print(add(1, 2))", "    print(mul(3, 4))", "    print(area(2))", "    print(scale(5))", "", "",
        'if __name__ == "__main__":', "    main()"]
w("src/calc.py", "\n".join(calc) + "\n")
rs = ["pub struct Point {", "    pub x: i32,", "    pub y: i32,", "}", "", "impl Point {",
      "    pub fn new(x: i32, y: i32) -> Self {", "        Point { x, y }", "    }", "",
      "    pub fn sum(&self) -> i32 {", "        self.x + self.y", "    }", "",
      "    pub fn scaled(&self, k: i32) -> Point {", "        Point { x: self.x * k, y: self.y * k }", "    }", "}", "",
      "pub fn origin() -> Point {", "    Point::new(0, 0)", "}", "", "pub fn far() -> Point {", "    Point::new(100, 100)", "}", "",
      "pub fn manhattan(a: &Point, b: &Point) -> i32 {", "    (a.x - b.x).abs() + (a.y - b.y).abs()", "}", "",
      "#[cfg(test)]", "mod tests {", "    use super::*;", "", "    #[test]", "    fn sums() {",
      "        assert_eq!(Point::new(1, 2).sum(), 3);", "    }", "}"]
w("src/lib.rs", "\n".join(rs) + "\n")
ko = ["# 메모", "", "이 저장소는 시험용입니다.", "", "## 할 일", "", "- 첫째 항목: 계산기 정리", "- 둘째 항목: 문서 다듬기",
      "- 셋째 항목: 시험 추가", "", "## 참고", "", "넓은 글자와 English가 섞인 줄입니다.", ""]
w("notes/readme-ko.md", "\n".join(ko) + "\n")
mk = ["CC = cc", "CFLAGS = -O2", "", "all: app", "", "app: main.o util.o", "\t$(CC) -o app main.o util.o", "",
      "clean:", "\trm -f app *.o", "", ".PHONY: all clean"]
w("Makefile", "\n".join(mk) + "\n")
go = ["package util", "", "func Max(a, b int) int {", "\tif a > b {", "\t\treturn a", "\t}", "\treturn b", "}", "",
      "func Min(a, b int) int {", "\tif a < b {", "\t\treturn a", "\t}", "\treturn b", "}"]
w("src/util.go", "\n".join(go) + "\n")
words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon".split()
long = []
for i in range(8):
    long.append(f"line {i}: " + " ".join(words[(i + k) % len(words)] for k in range(40)))
w("data/long.txt", "\n".join(long) + "\n")
tbl = ["ROWS = ["]
for i in range(60):
    tbl.append(f"    ({i}, \"item-{i:03d}\", {i * 3}),")
tbl += ["]", "", "", "def total():", "    return sum(r[2] for r in ROWS)"]
w("big/table.py", "\n".join(tbl) + "\n")
w("notes/todo.txt", "buy milk\nwrite tests\nfix the calculator\nread the docs\ncall back\n")
w("conf/a.cfg", "[main]\nname = a\nenabled = false\nlevel = 1\n")
w("conf/b.cfg", "[main]\nname = b\nenabled = false\nlevel = 2\n")
nb = {"cells": [{"cell_type": "code", "execution_count": None, "id": "c1", "metadata": {}, "outputs": [],
                 "source": ["x = 1\n", "print(x)"]},
                {"cell_type": "markdown", "id": "c2", "metadata": {}, "source": ["# Notes"]}],
      "metadata": {"language_info": {"name": "python"}}, "nbformat": 4, "nbformat_minor": 5}
w("nb/demo.ipynb", json.dumps(nb, indent=1) + "\n")
logger = f"python3 -Ic \"import sys,time,json; d=json.load(sys.stdin); d['_t']=time.time(); open('{hooklog}','a').write(json.dumps(d)+'\\n')\""
hooks = {ev: [{"matcher": "Write|Edit|MultiEdit|NotebookEdit", "hooks": [{"type": "command", "command": logger}]}]
         for ev in ("PreToolUse", "PostToolUse")}
w(".claude/settings.json", json.dumps({"hooks": hooks}, indent=1) + "\n")
