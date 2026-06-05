#!/usr/bin/env python3
import argparse
import base64
from pathlib import Path


CRC32C_POLY = 0x82F63B78


def crc32c_table() -> list[int]:
    table = []
    for value in range(256):
        crc = value
        for _ in range(8):
            if crc & 1:
                crc = (crc >> 1) ^ CRC32C_POLY
            else:
                crc >>= 1
        table.append(crc & 0xFFFFFFFF)
    return table


def crc32c(path: Path) -> int:
    table = crc32c_table()
    crc = 0xFFFFFFFF

    with path.open("rb") as file:
        while chunk := file.read(1024 * 1024):
            for byte in chunk:
                crc = table[(crc ^ byte) & 0xFF] ^ (crc >> 8)

    return crc ^ 0xFFFFFFFF


def content_digest_crc32c(path: Path) -> str:
    checksum = crc32c(path)
    encoded = base64.b64encode(checksum.to_bytes(4, "big")).decode("ascii")
    return f"crc32c=:{encoded}:"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Print a Content-Digest crc32c value for a file."
    )
    parser.add_argument("file", type=Path, help="file to checksum")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    print(content_digest_crc32c(args.file))


if __name__ == "__main__":
    main()
