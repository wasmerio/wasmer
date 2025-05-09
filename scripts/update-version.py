#!/usr/bin/python

PREVIOUS_VERSION='6.0.0'
NEXT_VERSION='6.0.1'

def make_prerelease_version(version: str) -> str:
    parts = version.split('.', 2)
    if len(parts) != 3:
        raise ValueError(f"Invalid version format: {version} - {parts}")
    major, minor, patch = parts
    new_minor = str(int(major) * 100 + int(minor))
    # patch may potentially contain a pre-release identifier, and that's OK
    return f"0.{new_minor}.{patch}"

PREVIOUS_PRERELEASE_VERSION = make_prerelease_version(PREVIOUS_VERSION)
NEXT_PRERELEASE_VERSION = make_prerelease_version(NEXT_VERSION)

import os
import re

def replace(file, pattern, subst):
    # Read contents from file as a single string
    file_handle = open(file, 'r')
    file_string = file_handle.read()
    file_handle.close()

    # Use RE package to allow for replacement (also allowing for (multiline) REGEX)
    file_string = (re.sub(pattern, subst, file_string))

    # Write contents to file.
    # Using mode 'w' truncates the file.
    file_handle = open(file, 'w')
    file_handle.write(file_string)
    file_handle.close()

def replace_version(path):
    print(PREVIOUS_VERSION + " -> " + NEXT_VERSION + " (" + path + ")")
    replace(path, "version = \"" + PREVIOUS_VERSION +"\"", "version = \"" + NEXT_VERSION +"\"")
    replace(path, "version = \"=" + PREVIOUS_VERSION +"\"", "version = \"=" + NEXT_VERSION +"\"")
    replace(path, "version = \"" + PREVIOUS_PRERELEASE_VERSION +"\"", "version = \"" + NEXT_PRERELEASE_VERSION +"\"")
    replace(path, "version = \"=" + PREVIOUS_PRERELEASE_VERSION +"\"", "version = \"=" + NEXT_PRERELEASE_VERSION +"\"")
    pass

def replace_version_py(path):
    print(PREVIOUS_VERSION + " -> " + NEXT_VERSION + " (" + path + ")")
    replace(path, "target_version = \"" + PREVIOUS_VERSION +"\"", "target_version = \"" + NEXT_VERSION +"\"")
    replace(path, "target_version = \"" + PREVIOUS_PRERELEASE_VERSION +"\"", "target_version = \"" + NEXT_PRERELEASE_VERSION +"\"")
    pass

def replace_version_iss(path):
    print(PREVIOUS_VERSION + " -> " + NEXT_VERSION + " (" + path + ")")
    replace(path, "AppVersion=" + PREVIOUS_VERSION, "AppVersion=" + NEXT_VERSION)
    replace(path, "AppVersion=" + PREVIOUS_PRERELEASE_VERSION, "AppVersion=" + NEXT_PRERELEASE_VERSION)
    pass

print ("Updating crate versions from " + PREVIOUS_VERSION + " to " + NEXT_VERSION + " (and prerelease versions from " + PREVIOUS_PRERELEASE_VERSION + " to " + NEXT_PRERELEASE_VERSION + ")")
for root, dirs, files in os.walk("."):
    path = root.split(os.sep)
    # print((len(path) - 1) * '---', os.path.basename(root))
    for file in files:
        if "Cargo.toml" in file:
            replace_version(root + "/" + file)
        elif "wasmer.iss" in file:
            replace_version_iss(root + "/" + file)
        elif "publish.py" in file:
            replace_version_py(root + "/" + file)

os.system("cargo generate-lockfile")