#!/usr/bin/env python3
"""Select the standalone embedding driver's dependency from its CLI build."""
from pathlib import Path
import hashlib
import json
import sys


def select_rlib(deps, name, manifest=""):
    metadata = json.loads(Path(manifest).read_text()) if manifest else {}
    paths = ([Path(metadata[name])] if manifest
             else list(Path(deps).glob("lib"+name+"-*.rlib")))
    assert len(paths) == 1, (name, paths)
    selected = paths[0].resolve()
    assert selected.parent == Path(deps).resolve() and selected.is_file()
    assert selected.name.startswith("lib"+name+"-") and selected.suffix == ".rlib"
    expected = metadata.get("_sha256", {}).get(name)
    if expected:
        digest = hashlib.sha256()
        with selected.open("rb") as library:
            while chunk := library.read(1024*1024):
                digest.update(chunk)
        assert digest.hexdigest() == expected, "CLI dependency changed after its build"
    return selected


if __name__ == "__main__":
    print(select_rlib(*sys.argv[1:]))
