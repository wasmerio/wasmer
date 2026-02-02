#! /usr/bin/env python3

from itertools import takewhile
import os
import time
import sys
import subprocess
import tempfile
import datetime
import re

RELEASE_VERSION = ""
DATE = datetime.date.today().strftime("%d/%m/%Y")
SIGNOFF_REVIEWER = "Arshia001"
TAG = "main"

if len(sys.argv) > 1:
    RELEASE_VERSION = sys.argv[1]
else:
    print("no release version as first argument")
    sys.exit(1)


if len(sys.argv) > 2:
    TAG = sys.argv[2]

RELEASE_VERSION_WITH_V = RELEASE_VERSION

if not (RELEASE_VERSION.startswith("v")):
    RELEASE_VERSION_WITH_V = "v" + RELEASE_VERSION
else:
    RELEASE_VERSION = RELEASE_VERSION[1:]

if os.system("git --version") != 0:
    print("git not installed")
    sys.exit(1)

if os.system("gh --version") != 0:
    print("gh not installed")
    sys.exit(1)


def get_file_string(file):
    file_handle = open(file, "r", newline="")
    file_string = file_handle.read()
    file_handle.close()
    return file_string


def write_file_string(file, file_string):
    file_handle = open(file, "w")
    file_handle.write(file_string)
    file_handle.close()


def replace(file, pattern, subst):
    file_string = get_file_string(file)
    file_string = file_string.replace(pattern, subst, 1)
    write_file_string(file, file_string)


def make_release(version):
    gh_logged_in = os.system("gh auth status") == 0
    if not (gh_logged_in):
        raise Exception("please log in")

    temp_dir = tempfile.TemporaryDirectory(prefix="wasmer-git-")
    print(temp_dir.name)
    if (
        os.system(
            "git clone git@github.com:wasmerio/wasmer.git --branch "
            + TAG
            + " --depth 1 "
            + temp_dir.name
        )
        != 0
    ):
        raise Exception("could not clone github repo")

    # As of now, GH CLI cannot list more items!
    GH_LISTING_LIMIT = 1000

    # generate changelog
    listed_prs = subprocess.check_output(
        [
            "gh",
            "search",
            "prs",
            "--repo",
            "wasmerio/wasmer",
            "--merged",
            "--limit",
            str(GH_LISTING_LIMIT),
            "--sort",
            "updated",
        ],
        encoding="utf-8",
        cwd=temp_dir.name,
    ).splitlines()

    listed_prs = list(takewhile(lambda line: "Release " not in line, listed_prs))
    print(f"Listed {len(listed_prs)} merged PRs since the latest release")

    # Make sure we listed all the merged PRs since the last release!
    assert len(listed_prs) < GH_LISTING_LIMIT

    changed = []
    added = []
    fixed = []
    release_notes_changed = []

    for line in listed_prs:
        fields = line.split("\t")
        pr_number = fields[1]
        pr_text = fields[3]
        line = (
            "  - [#"
            + pr_number
            + "](https://github.com/wasmerio/wasmer/pull/"
            + pr_number
            + ") "
            + pr_text
        )
        release_notes_changed.append(line)
        if "add" in line.lower():
            added.append(line)
        elif "fix" in line.lower():
            fixed.append(line)
        else:
            changed.append(line)

    changelog = []

    changelog.append("## **Unreleased**")
    changelog.append("")
    changelog.append("## " + RELEASE_VERSION + " - " + DATE)
    changelog.append("")
    changelog.append("## Added")
    changelog.append("")
    for a in added:
        changelog.append(a)
    changelog.append("")
    changelog.append("## Changed")
    changelog.append("")
    for c in changed:
        changelog.append(c)
    changelog.append("")
    changelog.append("## Fixed")
    changelog.append("")
    for f in fixed:
        changelog.append(f)
    changelog.append("")
    changelog.append("")

    for line in changelog:
        print("        " + line)

    proc = subprocess.Popen(
        [
            "gh",
            "search",
            "prs",
            "--repo",
            "wasmerio/wasmer",
            "--merged",
            "--sort",
            "updated",
        ],
        stdout=subprocess.PIPE,
        cwd=temp_dir.name,
    )
    proc.wait()

    already_released_str = ""
    for line in proc.stdout:
        line = line.decode("utf-8").rstrip()
        if RELEASE_VERSION + "\t" in line:
            already_released_str = line
            break

    already_released = already_released_str != ""

    proc = subprocess.Popen(
        ["gh", "pr", "list", "--repo", "wasmerio/wasmer"],
        stdout=subprocess.PIPE,
        cwd=temp_dir.name,
    )
    proc.wait()

    github_link_line = ""
    for line in proc.stdout:
        line = line.decode("utf-8").rstrip()
        if "release-" + RELEASE_VERSION + "\t" in line:
            github_link_line = line
            break

    print("github link line" + github_link_line)

    if github_link_line != "":
        proc = subprocess.Popen(
            ["git", "pull", "origin", "release-" + RELEASE_VERSION],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()

        proc = subprocess.Popen(
            ["git", "checkout", "-b", "release-" + RELEASE_VERSION],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()

        proc = subprocess.Popen(
            ["git", "pull", "origin", "release-" + RELEASE_VERSION],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()

        proc = subprocess.Popen(
            ["git", "log", "--oneline"], stdout=subprocess.PIPE, cwd=temp_dir.name
        )
        proc.wait()
        for line in proc.stdout:
            print(line.rstrip())

    if github_link_line == "" and not (already_released):
        # git checkout -b release-3.0.0-rc.2
        proc = subprocess.Popen(
            ["git", "checkout", "-b", "release-" + RELEASE_VERSION],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()

        if proc.returncode != 0:
            for line in proc.stdout:
                print(line.rstrip())
            raise Exception("could not run git checkout -b release-" + RELEASE_VERSION)

        replace(
            temp_dir.name + "/CHANGELOG.md", "## **Unreleased**", "\r\n".join(changelog)
        )

        proc = subprocess.Popen(
            ["git", "commit", "-am", "Update CHANGELOG"],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()
        if proc.returncode != 0:
            for line in proc.stdout:
                print(line.rstrip())
            raise Exception("could not commit CHANGELOG " + RELEASE_VERSION_WITH_V)

        # Update version numbers
        update_version_py = get_file_string(
            temp_dir.name + "/scripts/update-version.py"
        )
        previous_version = re.search('NEXT_VERSION = "(.*)"', update_version_py).groups(
            1
        )[0]
        next_version = RELEASE_VERSION
        print("updating version " + previous_version + " -> " + next_version)
        update_version_py = re.sub(
            'PREVIOUS_VERSION = ".*"',
            f'PREVIOUS_VERSION = "{previous_version}"',
            update_version_py,
        )
        update_version_py = re.sub(
            'NEXT_VERSION = ".*"',
            f'NEXT_VERSION = "{next_version}"',
            update_version_py,
        )
        write_file_string(
            temp_dir.name + "/scripts/update-version.py", update_version_py
        )
        proc = subprocess.Popen(
            ["python3", temp_dir.name + "/scripts/update-version.py"],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()

        proc = subprocess.Popen(
            ["git", "commit", "-am", "Release " + RELEASE_VERSION],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()
        if proc.returncode != 0:
            for line in proc.stdout:
                print(line.rstrip())
            raise Exception("could not commit CHANGELOG " + RELEASE_VERSION_WITH_V)

        proc = subprocess.Popen(
            ["git", "log", "--oneline"], stdout=subprocess.PIPE, cwd=temp_dir.name
        )
        for line in proc.stdout:
            line = line.decode("utf-8").rstrip()
            print(line)
        proc.wait()

        proc = subprocess.Popen(
            ["git", "push", "-f", "-u", "origin", "release-" + RELEASE_VERSION],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()

        proc = subprocess.Popen(
            [
                "gh",
                "pr",
                "create",
                "--head",
                "release-" + RELEASE_VERSION,
                "--title",
                "Release " + RELEASE_VERSION,
                "--body",
                "[bot] Release wasmer version " + RELEASE_VERSION,
                "--reviewer",
                SIGNOFF_REVIEWER,
            ],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()

        proc = subprocess.Popen(
            ["gh", "pr", "list", "--repo", "wasmerio/wasmer"],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()

        for line in proc.stdout:
            line = line.decode("utf-8").rstrip()
            if "release-" + RELEASE_VERSION + "\t" in line:
                github_link_line = line
                break

    pr_number = ""
    if already_released:
        pr_number = already_released_str.split("\t")[1]
        print("already released in PR " + pr_number)
    else:
        pr_number = github_link_line.split("\t")[0]
        print("releasing in PR " + pr_number)

    while not (already_released):
        proc = subprocess.Popen(
            ["gh", "pr", "checks", pr_number], stdout=subprocess.PIPE, cwd=temp_dir.name
        )
        proc.wait()

        all_checks_have_passed = True

        print(
            "Waiting for checks to pass... PR "
            + pr_number
            + "    https://github.com/wasmerio/wasmer/pull/"
            + pr_number
        )
        print("")

        for line in proc.stdout:
            line = line.decode("utf-8").rstrip()
            print("    " + line)
            if "no checks reported" in line:
                all_checks_have_passed = False
            if line.startswith("*") or "pending" in line:
                all_checks_have_passed = False
            if line.startswith("X") or "fail" in line:
                raise Exception("check failed")

        if all_checks_have_passed:
            break
        else:
            time.sleep(5)

    last_commit = ""
    proc = subprocess.Popen(["git", "log"], stdout=subprocess.PIPE, cwd=temp_dir.name)
    proc.wait()
    if proc.returncode == 0:
        for line in proc.stdout:
            line = line.decode("utf-8").rstrip()
            print(line.rstrip())
            last_commit = line
            break
    else:
        raise Exception("could not git log branch " + RELEASE_VERSION_WITH_V)

    if last_commit == "":
        raise Exception("could not get last info")

    proc = subprocess.Popen(
        ["git", "checkout", "main"], stdout=subprocess.PIPE, cwd=temp_dir.name
    )
    proc.wait()
    if proc.returncode != 0:
        for line in proc.stdout:
            print(line.rstrip())
        raise Exception("could not commit checkout main " + RELEASE_VERSION_WITH_V)

    if not (already_released):
        proc = subprocess.Popen(
            ["gh", "pr", "merge", "--auto", pr_number, "--merge", "--delete-branch"],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()

    # wait for bors to merge PR
    while not (already_released):
        print("git pull origin main...")
        proc = subprocess.Popen(
            ["git", "pull", "origin", "main"],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()
        if proc.returncode != 0:
            for line in proc.stdout:
                print(line.rstrip())
            raise Exception("could not pull origin ")

        proc = subprocess.Popen(
            [
                "gh",
                "search",
                "prs",
                "--repo",
                "wasmerio/wasmer",
                "--merged",
                "--sort",
                "updated",
            ],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()

        github_link_line = ""
        for line in proc.stdout:
            line = line.decode("utf-8").rstrip()
            if RELEASE_VERSION + "\t" in line:
                github_link_line = line
                break

        current_commit = ""
        proc = subprocess.Popen(
            ["git", "log"], stdout=subprocess.PIPE, cwd=temp_dir.name
        )
        proc.wait()
        if proc.returncode == 0:
            for line in proc.stdout:
                line = line.decode("utf-8").rstrip()
                print(line.rstrip())
                current_commit = line
                break
        else:
            raise Exception("could not git log main")

        if current_commit == "":
            raise Exception("could not get current info")

        if github_link_line != "":
            print("ok: " + current_commit + " == " + last_commit)
            print(github_link_line)
            break
        else:
            time.sleep(20)

    # Select the correct merge commit to tag
    correct_checkout = ""
    proc = subprocess.Popen(
        ["git", "log", "--oneline"], stdout=subprocess.PIPE, cwd=temp_dir.name
    )
    proc.wait()
    if proc.returncode == 0:
        for line in proc.stdout:
            line = line.decode("utf-8").rstrip()
            if "Merge pull request #" + pr_number in line:
                correct_checkout = line
    else:
        raise Exception("could not git log branch " + RELEASE_VERSION_WITH_V)

    if correct_checkout == "":
        raise Exception("could not get last info")

    print(correct_checkout)
    checkout_hash = correct_checkout.split(" ")[0]
    print("checking out hash " + checkout_hash)

    proc = subprocess.Popen(
        ["git", "tag", "-d", RELEASE_VERSION_WITH_V],
        stdout=subprocess.PIPE,
        cwd=temp_dir.name,
    )
    proc.wait()

    proc = subprocess.Popen(
        ["git", "push", "-d", "origin", RELEASE_VERSION_WITH_V],
        stdout=subprocess.PIPE,
        cwd=temp_dir.name,
    )
    proc.wait()

    proc = subprocess.Popen(
        ["git", "tag", RELEASE_VERSION_WITH_V, checkout_hash],
        stdout=subprocess.PIPE,
        cwd=temp_dir.name,
    )
    proc.wait()

    proc = subprocess.Popen(
        ["git", "push", "-f", "origin", RELEASE_VERSION_WITH_V],
        stdout=subprocess.PIPE,
        cwd=temp_dir.name,
    )
    proc.wait()

    # Make release and wait for it to finish
    if not (already_released):
        proc = subprocess.Popen(
            [
                "gh",
                "workflow",
                "run",
                "build.yml",
                "--field",
                "release=" + RELEASE_VERSION_WITH_V,
                "--ref",
                RELEASE_VERSION_WITH_V,
            ],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()
        time.sleep(5)

    while True:
        # gh run list --workflow=build.yml
        proc = subprocess.Popen(
            ["gh", "run", "list", "--workflow=build.yml"],
            stdout=subprocess.PIPE,
            cwd=temp_dir.name,
        )
        proc.wait()

        workflow_line = ""
        if proc.returncode == 0:
            for line in proc.stdout:
                line = line.decode("utf-8").rstrip()
                if RELEASE_VERSION_WITH_V in line:
                    workflow_line = line
                    break

        print("workflow line: " + workflow_line)

        if workflow_line.startswith("X"):
            raise Exception("release workflow failed")

        proc = subprocess.Popen(
            ["gh", "release", "list"], stdout=subprocess.PIPE, cwd=temp_dir.name
        )
        proc.wait()

        release_line = ""
        if proc.returncode == 0:
            for line in proc.stdout:
                line = line.decode("utf-8").rstrip()
                if RELEASE_VERSION_WITH_V in line:
                    release_line = line
                    break

        if release_line != "":
            break
        else:
            print("not released yet")

        time.sleep(30)

    # release done, update release

    release_notes = [
        "Install this version of wasmer:",
        "",
        "```sh",
        'curl https://get.wasmer.io -sSfL | sh -s "' + RELEASE_VERSION_WITH_V + '"',
        "```",
        "",
    ]

    if not (len(added) == 0) and not (len(changed) == 0):
        release_notes.append("## What's Changed")
        release_notes.append("")

    for a in added:
        release_notes.append(a)

    for c in changed:
        release_notes.append(c)

    hash = RELEASE_VERSION + "---" + DATE
    hash = hash.replace(".", "")
    hash = hash.replace("/", "")

    release_notes.append("")
    release_notes.append(
        "See full list of changes in the [CHANGELOG](https://github.com/wasmerio/wasmer/blob/main/CHANGELOG.md#"
        + hash
        + ")"
    )

    proc = subprocess.Popen(
        [
            "gh",
            "release",
            "edit",
            RELEASE_VERSION_WITH_V,
            "--notes",
            "\r\n".join(release_notes),
        ],
        stdout=subprocess.PIPE,
        cwd=temp_dir.name,
    )
    proc.wait()

    print("Script done and merged 🎉🎉🎉")


make_release(RELEASE_VERSION)
