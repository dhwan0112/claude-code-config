# Per-machine values

`tools/machine.sh` sources `<hostname>.env` from this folder when it exists. A file sets only the values that differ
from the defaults in `tools/machine.sh`, each as `: "${NAME:=value}"` (so the environment still wins). The Windows
check machine needs no file: its WSL checkout uses the defaults.
