#! /usr/bin/env python3

import os
import signal
import time
import sys
import subprocess
import tempfile
import datetime
import re

RELEASE_VERSION=""
DATE = datetime.date.today().strftime("%d/%m/%Y")
SIGNOFF_REVIEWER = "syrusakbary"

if len(sys.argv) > 1:
    RELEASE_VERSION = sys.argv[1]
else:
    print("no release version as first argument")
    sys.exit(1)

RELEASE_VERSION_WITH_V = RELEASE_VERSION

if not(RELEASE_VERSION.startswith("v")):
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
    file_handle = open(file, 'r')
    file_string = file_handle.read()
    file_handle.close()
    return file_string

def write_file_string(file, file_string):
    file_handle = open(file, 'w')
    file_handle.write(file_string)
    file_handle.close()

def replace(file, pattern, subst):
    file_string = get_file_string(file)
    file_string = file_string.replace(pattern, subst,1)
    write_file_string(file, file_string)

def make_release(version):
    gh_logged_in = os.system("gh auth status") == 0
    if not(gh_logged_in):
        raise Exception("please log in")
    
    import tempfile

    temp_dir = tempfile.TemporaryDirectory()
    print(temp_dir.name)
    if os.system("git clone https://github.com/wasmerio/wasmer --branch master --depth 1 " + temp_dir.name) != 0:
        raise Exception("could not clone github repo")

    # generate changelog
    proc = subprocess.Popen(['gh', "search", "prs", "--repo", "wasmerio/wasmer", "--merged", "--limit", "100"], stdout = subprocess.PIPE, cwd = temp_dir.name)
    proc.wait()
    if proc.returncode != 0:
        print(proc.stdout)
        raise Exception("could not run gh search prs")

    lines = []
    for line in proc.stdout:
        line = line.decode("utf-8").rstrip()
        if "Release" in line:
            break
        lines.append(line)

    changed = []
    added = []
    fixed = []
    release_notes_changed = []

    for l in lines:
        fields = l.split("\t")
        pr_number = fields[1]
        pr_text = fields[3]
        l = "  - [#" + pr_number + "](https://github.com/wasmerio/wasmer/pull/" + pr_number + ") " + pr_text
        release_notes_changed.append(l)
        if "add" in l.lower():
            added.append(l)
        elif "fix" in l.lower():
            fixed.append(l)
        else:
            changed.append(l)

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

    for l in changelog:
        print("        " + l)

    proc = subprocess.Popen(['gh','search', "prs", "--repo", "wasmerio/wasmer", "--merged"], stdout = subprocess.PIPE, cwd = temp_dir.name)
    proc.wait()

    already_released_str = ""
    for line in proc.stdout:
        line = line.decode("utf-8").rstrip()
        if RELEASE_VERSION + "\t" in line:
            already_released_str = line
            break
    
    already_released = already_released_str != ""

    proc = subprocess.Popen(['gh','pr', "list", "--repo", "wasmerio/wasmer"], stdout = subprocess.PIPE, cwd = temp_dir.name)
    proc.wait()

    github_link_line = ""
    for line in proc.stdout:
        line = line.decode("utf-8").rstrip()
        if "release-" + RELEASE_VERSION + "\t" in line:
            github_link_line = line
            break
    
    print("github link line" + github_link_line)

    if github_link_line != "":
        proc = subprocess.Popen(['git','pull', "origin", "release-" + RELEASE_VERSION], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()

        proc = subprocess.Popen(['git','checkout', "-b", "release-" + RELEASE_VERSION], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()
        
        proc = subprocess.Popen(['git','pull', "origin", "release-" + RELEASE_VERSION], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()

        proc = subprocess.Popen(['git','log', "--oneline"], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()
        for line in proc.stdout:
            print(line.rstrip())

    if github_link_line == "" and not(already_released):

        # git checkout -b release-3.0.0-rc.2
        proc = subprocess.Popen(['git','checkout', "-b", "release-" + RELEASE_VERSION], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()
        
        if proc.returncode != 0:
            for line in proc.stdout:
                print(line.rstrip())
            raise Exception("could not run git checkout -b release-" + RELEASE_VERSION)

        replace(temp_dir.name + "/CHANGELOG.md", "## **Unreleased**", "\r\n".join(changelog))

        proc = subprocess.Popen(['git','commit', "-am", "Update CHANGELOG"], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()
        if proc.returncode != 0:
            for line in proc.stdout:
                print(line.rstrip())
            raise Exception("could not commit CHANGELOG " + RELEASE_VERSION_WITH_V)

        # Update version numbers
        update_version_py = get_file_string(temp_dir.name + "/scripts/update-version.py")
        previous_version = re.search("NEXT_VERSION=\'(.*)\'", update_version_py).groups(1)[0]
        next_version = RELEASE_VERSION
        print("updating version " + previous_version + " -> " + next_version)
        update_version_py = re.sub("PREVIOUS_VERSION=\'.*\'","PREVIOUS_VERSION='" + previous_version + "'", update_version_py)
        update_version_py = re.sub("NEXT_VERSION=\'.*\'","NEXT_VERSION='" + next_version + "'", update_version_py)
        write_file_string(temp_dir.name + "/scripts/update-version.py", update_version_py)
        proc = subprocess.Popen(['python3', temp_dir.name + "/scripts/update-version.py"], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()

        proc = subprocess.Popen(['git','commit', "-am", "Release " + RELEASE_VERSION], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()
        if proc.returncode != 0:
            for line in proc.stdout:
                print(line.rstrip())
            raise Exception("could not commit CHANGELOG " + RELEASE_VERSION_WITH_V)


        proc = subprocess.Popen(['git','log', "--oneline"], stdout = subprocess.PIPE, cwd = temp_dir.name)
        for line in proc.stdout:
            line = line.decode("utf-8").rstrip()
            print(line)
        proc.wait()

        proc = subprocess.Popen(['git','push', "-f", "-u", "origin", "release-" + RELEASE_VERSION], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()

        proc = subprocess.Popen(['gh','pr', "create", "--head", "release-" + RELEASE_VERSION, "--title", "Release " + RELEASE_VERSION, "--body", "[bot] Release wasmer version " + RELEASE_VERSION, "--reviewer", SIGNOFF_REVIEWER], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()

        proc = subprocess.Popen(['gh','pr', "list", "--repo", "wasmerio/wasmer"], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()

        for line in proc.stdout:
            line = line.decode("utf-8").rstrip()
            if "release-" + RELEASE_VERSION + "\t" in line:
                github_link_line = line
                break

    pr_number = ""
    if (already_released):
        pr_number = already_released_str.split("\t")[1]
        print("already released in PR " + pr_number)
    else:
        pr_number = github_link_line.split("\t")[0]
        print("releasing in PR " + pr_number)

    while not(already_released):
        proc = subprocess.Popen(['gh','pr', "checks", pr_number], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()

        bors_failed = False
        all_checks_have_passed = True

        if proc.stderr is not None:
            for line in proc.stderr:
                if "no checks reported" in line:
                    all_checks_have_passed = False

        if all_checks_have_passed: 
            for line in proc.stdout:
                line = line.decode("utf-8").rstrip()
                print("---- " + line)
                if "no checks reported" in line:
                    all_checks_have_passed = False
                if line.startswith("*"):
                    all_checks_have_passed = False
                if "pending" in line and not("bors" in line):
                    all_checks_have_passed = False
                if line.startswith("X"):
                    raise Exception("check failed")
                if "fail" in line and "bors" in line:
                    bors_failed = True
                if "pending" in line and "bors" in line:
                    bors_failed = True
                if "fail" in line and not("bors" in line):
                    raise Exception("check failed")

        if all_checks_have_passed:
            if proc.returncode != 0 and not(bors_failed):
                raise Exception("failed to list checks with: gh pr checks " + pr_number)
            break
        else:
            print("Waiting for checks to pass... PR " + pr_number + "    https://github.com/wasmerio/wasmer/pull/" + pr_number)
            time.sleep(30)

    if not(already_released):
        # PR created, checks have passed, run python script and publish to crates.io
        proc = subprocess.Popen(['gh','pr', "comment", pr_number, "--body", "[bot] Checks have passed. Publishing to crates.io..."], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()

        proc = subprocess.Popen(['python3',temp_dir.name + "/scripts/publish.py", "publish"], stdout = subprocess.PIPE, cwd = temp_dir.name)
        while True:
            line = proc.stdout.readline()
            line = line.decode("utf-8").rstrip()
            print(line.rstrip())
            if not line: break
            
        proc.wait()
    
        if proc.returncode != 0:
            log = ["[bot] Failed to publish to crates.io"]
            log.append("")
            log.append("```")
            for line in proc.stdout:
                line = line.decode("utf-8").rstrip()
                log.append("stdout: " + line)
            log.append("```")
            log.append("```")
            if proc.stderr is not None:
                for line in proc.stderr:
                    line = line.decode("utf-8").rstrip()
                    log.append("stderr: " + line)
            log.append("```")
            proc = subprocess.Popen(['gh','pr', "comment", pr_number, "--body", "\r\n".join(log)], stdout = subprocess.PIPE, cwd = temp_dir.name)
            proc.wait()
            raise Exception("Failed to publish to crates.io: " + "\r\n".join(log))
        else:
            proc = subprocess.Popen(['gh','pr', "comment", pr_number, "--body", "[bot] Successfully published wasmer version " + RELEASE_VERSION + " to crates.io"], stdout = subprocess.PIPE, cwd = temp_dir.name)
            proc.wait()

    last_commit = ""
    proc = subprocess.Popen(['git','log'], stdout = subprocess.PIPE, cwd = temp_dir.name)
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

    proc = subprocess.Popen(['git','checkout', "master"], stdout = subprocess.PIPE, cwd = temp_dir.name)
    proc.wait()
    if proc.returncode != 0:
        for line in proc.stdout:
            print(line.rstrip())
        raise Exception("could not commit checkout master " + RELEASE_VERSION_WITH_V)

    if not(already_released):
        proc = subprocess.Popen(['gh','pr', "comment", pr_number, "--body", "bors r+"], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()

    # wait for bors to merge PR
    while not(already_released):

        print("git pull origin master...")
        proc = subprocess.Popen(['git','pull', "origin", "master"], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()
        if proc.returncode != 0:
            for line in proc.stdout:
                print(line.rstrip())
            raise Exception("could not pull origin ")
        
        proc = subprocess.Popen(['gh','search', "prs", "--repo", "wasmerio/wasmer", "--merged"], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()

        github_link_line = ""
        for line in proc.stdout:
            line = line.decode("utf-8").rstrip()
            if RELEASE_VERSION + "\t" in line:
                github_link_line = line
                break
                    
        current_commit = ""
        proc = subprocess.Popen(['git','log'], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()
        if proc.returncode == 0:
            for line in proc.stdout:
                line = line.decode("utf-8").rstrip()
                print(line.rstrip())
                current_commit = line
                break
        else: 
            raise Exception("could not git log master")

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
    proc = subprocess.Popen(['git','log', "--oneline"], stdout = subprocess.PIPE, cwd = temp_dir.name)
    proc.wait()
    if proc.returncode == 0:
        for line in proc.stdout:
            line = line.decode("utf-8").rstrip()
            if "Merge #" + pr_number in line:
                correct_checkout = line
    else:
        raise Exception("could not git log branch " + RELEASE_VERSION_WITH_V)

    if correct_checkout == "":
        raise Exception("could not get last info")

    print(correct_checkout)
    checkout_hash = correct_checkout.split(" ")[0]
    print("checking out hash " + checkout_hash)

    proc = subprocess.Popen(['git','tag', "-d", RELEASE_VERSION_WITH_V], stdout = subprocess.PIPE, cwd = temp_dir.name)
    proc.wait()

    proc = subprocess.Popen(['git','push', "-d", "origin", RELEASE_VERSION_WITH_V], stdout = subprocess.PIPE, cwd = temp_dir.name)
    proc.wait()

    proc = subprocess.Popen(['git','tag', RELEASE_VERSION_WITH_V, checkout_hash], stdout = subprocess.PIPE, cwd = temp_dir.name)
    proc.wait()

    proc = subprocess.Popen(['git','push', "-f", "origin", RELEASE_VERSION_WITH_V], stdout = subprocess.PIPE, cwd = temp_dir.name)
    proc.wait()

    # Make release and wait for it to finish
    if not(already_released):
        proc = subprocess.Popen(['gh','workflow', "run", "build.yml", "--field", "release=" + RELEASE_VERSION_WITH_V, "--ref", RELEASE_VERSION_WITH_V], stdout = subprocess.PIPE, cwd = temp_dir.name)
        proc.wait()
        time.sleep(5)

    while True:
        # gh run list --workflow=build.yml
        proc = subprocess.Popen(['gh','run', "list", "--workflow=build.yml"], stdout = subprocess.PIPE, cwd = temp_dir.name)
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

        proc = subprocess.Popen(['gh','release', "list"], stdout = subprocess.PIPE, cwd = temp_dir.name)
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
        "curl https://get.wasmer.io -sSfL | sh -s \"" + RELEASE_VERSION_WITH_V + "\"",
        "```",
        "",
    ]

    if not(len(added) == 0) and not(len(changed) == 0):
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
    release_notes.append("See full list of changes in the [CHANGELOG](https://github.com/wasmerio/wasmer/blob/master/CHANGELOG.md#" + hash + ")")

    proc = subprocess.Popen(['gh','release', "edit", RELEASE_VERSION_WITH_V, "--notes", "\r\n".join(release_notes)], stdout = subprocess.PIPE, cwd = temp_dir.name)
    proc.wait()

    raise Exception("script done and merged")

try:
    make_release(RELEASE_VERSION)
except Exception as err:
    while True:
        print(str(err))
        if os.system("say " + str(err)) != 0:
            sys.exit()