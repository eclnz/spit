"""Start an output folder as a private copy of an input folder."""

from __future__ import annotations

import os
import shutil
import stat
from pathlib import Path


def copy_folder(source: Path, destination: Path) -> str | None:
    """Copy the folder `source` to `destination`, which does not exist; None when it is copied, or why not.

    Every file is copied, never linked, so that nothing the job writes reaches the input. A relative link
    whose target stays inside `source` is kept as a link; any other link, and anything that is neither a
    file nor a folder, is refused, since the copy could not keep it apart from the input.
    """
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
                # A link to a folder is kept as a link; `os.walk` does not go into it.
                os.symlink(target, inside / name)
            elif stat.S_ISDIR(info.st_mode):
                (inside / name).mkdir()
            elif stat.S_ISREG(info.st_mode):
                shutil.copy2(path, inside / name, follow_symlinks=False)
            else:
                return f"{shown} in {source} is neither a file nor a folder"
    return None
