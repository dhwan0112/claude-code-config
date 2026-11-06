"""Write a minimal Claude Code config that skips onboarding, trusts PROJ, approves the dummy key.
usage: mkcfg.py OUT_FILE PROJ_DIR [PROJ_DIR...]"""
import json, sys

out, projs = sys.argv[1], sys.argv[2:]
cfg = {
    "hasCompletedOnboarding": True,
    "theme": "dark",
    "customApiKeyResponses": {"approved": ["A" * 20], "rejected": []},
    "projects": {p: {"hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True} for p in projs},
}
with open(out, "w", encoding="utf-8") as f:
    json.dump(cfg, f, indent=1)
print(out)
