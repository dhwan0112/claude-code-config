import sys
d = open(sys.argv[1], 'rb').read()
parts = d.split(b'\n', 3); w, h = map(int, parts[1].split()); px = parts[3]
n = 0
for i in range(0, w * h * 3, 3):
    r, g, b = px[i], px[i + 1], px[i + 2]
    if max(abs(r - g), abs(g - b), abs(r - b)) > 12:
        n += 1
print(n)
