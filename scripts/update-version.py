#!/usr/bin/python

import argparse
import os
import re

parser = argparse.ArgumentParser(description="Update Wasmer versions")
parser.add_argument(
    "old_version",
    metavar="OLD_VERSION",
    help="version currently used by the repository",
)
parser.add_argument(
    "release_version",
    metavar="RELEASE_VERSION",
    help="version_to_release",
)
args = parser.parse_args()


def make_prerelease_version(version: str) -> str:
    parts = version.split(".", 2)
    if len(parts) != 3:
        raise ValueError(f"Invalid version format: {version} - {parts}")
    major, minor, patch = parts
    new_minor = str(int(major) * 100 + int(minor))
    # patch may potentially contain a pre-release identifier, and that's OK
    return f"0.{new_minor}.{patch}"


old_prerelease_version = make_prerelease_version(args.old_version)
release_prerelease_version = make_prerelease_version(args.release_version)


def replace(file, pattern, subst):
    # Read contents from file as a single string
    with open(file, "r") as file_handle:
        file_string = file_handle.read()

    # Use RE package to allow for replacement (also allowing for (multiline) REGEX)
    file_string = re.sub(pattern, subst, file_string)

    # Write contents to file.
    # Using mode 'w' truncates the file.
    with open(file, "w") as file_handle:
        file_handle.write(file_string)


def replace_version(path):
    print(args.old_version + " -> " + args.release_version + " (" + path + ")")
    replace(
        path,
        'version = "' + args.old_version + '"',
        'version = "' + args.release_version + '"',
    )
    replace(
        path,
        'version = "=' + args.old_version + '"',
        'version = "=' + args.release_version + '"',
    )
    replace(
        path,
        'version = "' + old_prerelease_version + '"',
        'version = "' + release_prerelease_version + '"',
    )
    replace(
        path,
        'version = "=' + old_prerelease_version + '"',
        'version = "=' + release_prerelease_version + '"',
    )


def replace_version_py(path):
    print(args.old_version + " -> " + args.release_version + " (" + path + ")")
    replace(
        path,
        'target_version = "' + args.old_version + '"',
        'target_version = "' + args.release_version + '"',
    )
    replace(
        path,
        'target_version = "' + old_prerelease_version + '"',
        'target_version = "' + release_prerelease_version + '"',
    )


def replace_version_iss(path):
    print(args.old_version + " -> " + args.release_version + " (" + path + ")")
    replace(
        path,
        "AppVersion=" + args.old_version,
        "AppVersion=" + args.release_version,
    )
    replace(
        path,
        "AppVersion=" + old_prerelease_version,
        "AppVersion=" + release_prerelease_version,
    )


print(
    "Updating crate versions from "
    + args.old_version
    + " to "
    + args.release_version
    + " (and prerelease versions from "
    + old_prerelease_version
    + " to "
    + release_prerelease_version
    + ")"
)
for root, dirs, files in os.walk("."):
    path = root.split(os.sep)
    # print((len(path) - 1) * '---', os.path.basename(root))
    for file in files:
        if "Cargo.toml" in file or "Cargo.standalone.toml" in file:
            replace_version(root + "/" + file)
        elif "wasmer.iss" in file:
            replace_version_iss(root + "/" + file)
        elif "publish.py" in file:
            replace_version_py(root + "/" + file)

os.system("cargo generate-lockfile")
