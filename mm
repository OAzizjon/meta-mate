#!/bin/sh
# mm from Git Bash or a Linux shell in the project folder: ./mm status   (see tools/mm.py)
here=$(dirname "$0")
for py in "$here/.venv/Scripts/python.exe" "$here/.venv/bin/python"; do
	[ -x "$py" ] && exec "$py" "$here/tools/mm.py" "$@"
done
if command -v py >/dev/null 2>&1; then exec py -3 "$here/tools/mm.py" "$@"; fi
exec python3 "$here/tools/mm.py" "$@"
