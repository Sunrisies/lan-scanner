#!/usr/bin/env python3
"""Package one native lan-scanner build using only Python's standard library."""
import argparse
import shutil
import tarfile
import tempfile
import zipfile
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--platform", required=True)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--readme", default=Path("README.md"), type=Path)
    parser.add_argument("--output", default=Path("dist"), type=Path)
    args = parser.parse_args()

    if not args.binary.is_file():
        raise SystemExit(f"binary not found: {args.binary}")
    if not args.readme.is_file():
        raise SystemExit(f"README not found: {args.readme}")

    is_windows = args.platform.startswith("windows-")
    suffix = ".exe" if is_windows else ""
    name = f"lan-scanner{suffix}"
    package_name = f"lan-scanner-{args.platform}"
    args.output.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory() as temp:
        staging = Path(temp)
        shutil.copy2(args.binary, staging / name)
        shutil.copy2(args.readme, staging / "README.md")
        archive = args.output / (package_name + (".zip" if is_windows else ".tar.gz"))
        if is_windows:
            with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
                bundle.write(staging / name, name)
                bundle.write(staging / "README.md", "README.md")
        else:
            with tarfile.open(archive, "w:gz") as bundle:
                bundle.add(staging / name, arcname=name)
                bundle.add(staging / "README.md", arcname="README.md")
    print(f"created {archive}")


if __name__ == "__main__":
    main()
