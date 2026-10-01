#!/usr/bin/env python3
"""Pack a folder into a zip, or unpack one.

usage: archive.py pack <folder> <archive.zip>
       archive.py unpack <archive.zip> <folder>

`pack` stores the folder under its own name, as `scenarios/s1-logs/brief.md`,
with entries sorted and fixed timestamps, so packing the same files again gives
the same archive. `unpack` writes that folder inside <folder>.
"""
import os
import sys
import zipfile


def pack(folder, archive):
    folder = os.path.normpath(folder)
    base = os.path.dirname(folder)
    paths = []
    for root, _, files in os.walk(folder):
        paths += [os.path.join(root, name) for name in files]
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as out:
        for path in sorted(paths):
            name = os.path.relpath(path, base).replace(os.sep, "/")
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.external_attr = 0o644 << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            with open(path, "rb") as file:
                out.writestr(info, file.read(), compresslevel=9)


def unpack(archive, folder):
    with zipfile.ZipFile(archive) as source:
        source.extractall(folder)


if __name__ == "__main__":
    if len(sys.argv) != 4 or sys.argv[1] not in ("pack", "unpack"):
        sys.exit(__doc__)
    {"pack": pack, "unpack": unpack}[sys.argv[1]](sys.argv[2], sys.argv[3])
