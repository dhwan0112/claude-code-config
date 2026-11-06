import json, sys
print(" ".join("%s=%sx%s" % (s["program"].split("/")[-1], s["cols"], s["rows"]) for s in json.load(sys.stdin)["sessions"]))
