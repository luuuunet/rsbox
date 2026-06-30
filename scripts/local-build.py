#!/usr/bin/env python3
"""Build rsbox locally on Windows (client / local server testing)."""
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.join(ROOT, "target", "release", "rsbox.exe")


def main():
    print(f"Building rsbox in {ROOT} ...")
    r = subprocess.run(
        ["cargo", "build", "--release", "-p", "rsbox"],
        cwd=ROOT,
    )
    if r.returncode != 0:
        sys.exit(r.returncode)
    if os.path.isfile(EXE):
        mb = os.path.getsize(EXE) / 1024 / 1024
        print(f"\nOK: {EXE} ({mb:.1f} MB)")
        print("\nLocal use:")
        print(f'  {EXE} check -c path\\to\\config.json')
        print(f'  {EXE} run  -c path\\to\\config.json')
    else:
        print("Build finished but rsbox.exe not found", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
