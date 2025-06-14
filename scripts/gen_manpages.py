# CLASSIFICATION: COMMUNITY
# Filename: gen_manpages.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-13
"""Generate manpages from CLI help Markdown files using pandoc."""

import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs" / "community"
MAN = ROOT / "docs" / "man"

mapping = {
    "CLI_HELP_COHCLI.md": "cohcli.1",
    "CLI_HELP_COHRUN.md": "cohrun.1",
    "CLI_HELP_COHCC.md": "cohcc.1",
    "CLI_HELP_COHCAP.md": "cohcap.1",
    "CLI_HELP_COHTRACE.md": "cohtrace.1",
}


def build(src: Path, dst: Path):
    subprocess.run(["pandoc", "-s", "-t", "man", str(src), "-o", str(dst)], check=True)


def main():
    for md, man in mapping.items():
        src = DOCS / md
        dst = MAN / man
        if src.exists():
            build(src, dst)
            print(f"generated {dst}")


if __name__ == "__main__":
    main()
