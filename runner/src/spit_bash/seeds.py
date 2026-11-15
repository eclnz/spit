"""Output folders that start as a copy of an input folder, as a job's `seed_folders` says."""

from __future__ import annotations

import os
import shutil
import stat
from pathlib import Path


def copy(source: Path, target: Path) -> str | None:
    """Copy what the folder `source` holds into the empty folder `target`, so the job's commands may change
    the copy and leave the input as it was; or say why it cannot be copied.

    A link is copied as a link when its text is relative and it leads to a place inside `source`, so it
    leads to the same place in the copy. Any other link, and anything but files, folders and such links,
    cannot be copied faithfully, and the job fails rather than share it with the input.
    """
    root = source.resolve()
    for folder, names, files in os.walk(source):
        here = Path(folder)
        into = target / here.relative_to(source)
        for name in [*names, *files]:
            path = here / name
            made = into / name
            info = path.lstat()
            if stat.S_ISLNK(info.st_mode):
                text = os.readlink(path)
                if os.path.isabs(text) or not (here / text).resolve().is_relative_to(root):
                    return f"{path} is a link to {text}, which leads outside the folder it is copied from"
                made.symlink_to(text)
            elif stat.S_ISDIR(info.st_mode):
                made.mkdir()
            elif stat.S_ISREG(info.st_mode):
                shutil.copy2(path, made)
            else:
                return f"{path} is neither a file, a folder nor a link, so it cannot be copied"
    return None
