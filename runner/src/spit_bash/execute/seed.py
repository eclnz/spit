from __future__ import annotations

import os
import shutil
import stat
from pathlib import Path


def copy_folder(source: Path, destination: Path) -> str | None:
    try:
        return _copy(source, destination)
    except OSError as error:
        return f"could not copy {source}: {error}"


def _copy(source: Path, destination: Path) -> str | None:
    top = source.resolve()
    destination.mkdir(parents=True)
    for directory, folders, files in os.walk(source):
        here = Path(directory)
        inside = destination / here.relative_to(source)
        for name in sorted(folders + files):
            path = here / name
            shown = path.relative_to(source)
            info = path.lstat()
            if stat.S_ISLNK(info.st_mode):
                target = os.readlink(path)
                resolved = (here / target).resolve()
                if os.path.isabs(target) or not resolved.is_relative_to(top):
                    return f"{shown} in {source} is a link to {target}, outside the folder"
                os.symlink(target, inside / name)
            elif stat.S_ISDIR(info.st_mode):
                (inside / name).mkdir()
            elif stat.S_ISREG(info.st_mode):
                shutil.copy2(path, inside / name, follow_symlinks=False)
            else:
                return f"{shown} in {source} is neither a file nor a folder"
    return None
