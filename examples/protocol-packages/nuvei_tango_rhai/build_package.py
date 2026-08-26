#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
from zipfile import ZIP_STORED, ZipFile, ZipInfo


PACKAGE_FILES = (
    "display.rhai",
    "document.toml",
    "manifest.toml",
    "protocol.rhai",
)
ARCHIVE_NAME = "nuvei-tango-json-rhai-1.0.0.zip"
FIXED_TIMESTAMP = (1980, 1, 1, 0, 0, 0)


def build(output_directory: Path) -> tuple[Path, str]:
    package_root = Path(__file__).resolve().parent
    output_directory.mkdir(parents=True, exist_ok=True)
    archive_path = output_directory / ARCHIVE_NAME

    with ZipFile(archive_path, "w", compression=ZIP_STORED) as archive:
        for relative_path in PACKAGE_FILES:
            info = ZipInfo(relative_path, FIXED_TIMESTAMP)
            info.compress_type = ZIP_STORED
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            archive.writestr(info, (package_root / relative_path).read_bytes())

    digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
    checksum_path = output_directory / f"{ARCHIVE_NAME}.sha256"
    checksum_path.write_text(f"{digest}  {ARCHIVE_NAME}\n", encoding="ascii")
    return archive_path, digest


def main() -> None:
    parser = argparse.ArgumentParser(description="Build the deterministic Nuvei Tango Rhai ZIP")
    parser.add_argument(
        "--output-directory",
        type=Path,
        default=Path(__file__).resolve().parent / "dist",
    )
    arguments = parser.parse_args()
    archive_path, digest = build(arguments.output_directory)
    print(f"archive={archive_path}")
    print(f"sha256={digest}")


if __name__ == "__main__":
    main()
