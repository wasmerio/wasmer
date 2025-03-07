#! /usr/bin/env python3

"""This is a script for publishing the wasmer crates to crates.io.
It should be run in the root of wasmer like `python3 scripts/publish.py --help`.

Please lint with pylint and format with black.

$ pylint scripts/publish.py

--------------------------------------------------------------------
Your code has been rated at 10.00/10 (previous run: 10.00/10, +0.00)

$ black scripts/publish.py
"""

import argparse
import itertools
import os
import re
import subprocess
import time
import sys
import typing
from pprint import pprint

try:
    # try import tomllib from standard Python library: New in version 3.11.
    import tomllib
except ModuleNotFoundError:
    try:
        # if tomllib is missing, this is an older python3 version. Try
        # importing the equivalent third-party library tomli:
        import tomli as tomllib
    except ModuleNotFoundError:
        print("Please install tomli, `pip3 install tomli`")
        sys.exit(1)

try:
    from graphlib import TopologicalSorter
except ImportError:
    print("Incompatible python3 version: lacks graphlib standard library module.")
    sys.exit(1)

SETTINGS = {
    # extra features to put when publishing, for example wasmer-cli needs a
    # compiler by default otherwise it won't work standalone
    "publish_features": {
        "wasmer-cli": "default,cranelift",
        "wasmer-wasix": "sys,wasmer/sys-default",
        "wasmer-wasix-types": "wasmer/sys-default",
        "wasmer-wast": "wasmer/sys-default",
        "wai-bindgen-wasmer": "sys",
        "wasmer-cache": "wasmer/sys-default",
    },
    # workspace members we want to publish but whose path doesn't start by
    # "./lib/"
    "non-lib-workspace-members": set(["tests/lib/wast"]),
}


def get_latest_version_for_crate(crate_name: str) -> typing.Optional[str]:
    """Fetches the latest version of a given crate name in local cargo registry."""
    output = subprocess.run(
        ["cargo", "search", crate_name], capture_output=True, check=True
    )
    rexp_src = f'^{crate_name} = "([^"]+)"'
    prog = re.compile(rexp_src)
    haystack = output.stdout.decode("utf-8")
    for line in haystack.splitlines():
        result = prog.match(line)
        if result:
            return result.group(1)
    return None


class Crate:
    """Represents a workspace crate that is to be published to crates.io."""

    def __init__(
        self,
        dependencies: typing.List[str],
        cargo_manifest: dict,
        cargo_file_path="Cargo.toml",
    ):
        self.name = cargo_manifest["package"]["name"]
        self.dependencies = dependencies
        self.cargo_manifest = cargo_manifest
        if not os.path.isabs(cargo_file_path):
            cargo_file_path = os.path.join(os.getcwd(), cargo_file_path)
        self.cargo_file_path = cargo_file_path

    def __str__(self):
        return f"{self.name}: {self.dependencies} {self.cargo_file_path} {self.path()}"

    @property
    def path(self) -> str:
        """Return the absolute filesystem path containing this crate."""
        return os.path.dirname(self.cargo_file_path)

    @property
    def version(self) -> str:
        """Return the crate's version according to its manifest."""
        return self.cargo_manifest["package"]["version"]


class Publisher:
    """A class responsible for figuring out dependencies,
    creating a topological sorting in order to publish them
    to crates.io in a valid order."""

    def __init__(self, version=None, dry_run=True, verbose=True):
        self.dry_run: bool = dry_run
        self.verbose: bool = verbose

        # open workspace Cargo.toml
        with open("Cargo.toml", "rb") as file:
            data = tomllib.load(file)

        if version is None:
            version = data["workspace"]["package"]["version"]
        self.version: str = version

        if self.verbose and not self.dry_run:
            print(f"Publishing version {self.version}")
        elif self.verbose and self.dry_run:
            print(f"Publishing version {self.version} dry run!")

        # define helper function
        check_local_dep_fn = lambda t: isinstance(t[1], dict) and "path" in t[1]
        members = set(
            map(
                lambda p: p + "/Cargo.toml",
                filter(
                    lambda path: (
                        path.startswith("lib/") and os.path.exists(path + "/Cargo.toml")
                    )
                    or path in SETTINGS["non-lib-workspace-members"],
                    itertools.chain(
                        data["workspace"]["members"],
                        map(
                            lambda p: p[1]["path"],
                            filter(check_local_dep_fn, data["dependencies"].items()),
                        ),
                    ),
                ),
            )
        )
        crates = []
        for member in members:
            with open(member, "rb") as file:
                member_data = tomllib.load(file)

            def return_dependencies(toml) -> typing.List[str]:
                acc = set()
                stack = [toml]
                while len(stack) > 0:
                    toml = stack.pop()
                    if "dependencies" in toml:
                        acc.update(
                            list(
                                map(
                                    lambda dep: dep[1]["package"]
                                    if "package" in dep[1]
                                    else dep[0],
                                    filter(
                                        check_local_dep_fn, toml["dependencies"].items()
                                    ),
                                )
                            )
                        )
                    if "dev-dependencies" in toml:
                        acc.update(
                            list(
                                map(
                                    lambda dep: dep[1]["package"]
                                    if "package" in dep[1]
                                    else dep[0],
                                    filter(
                                        check_local_dep_fn, toml["dev-dependencies"].items()
                                    ),
                                )
                            )
                        )
                    if "target" in toml:
                        stack.append(toml["target"])
                    for key, value in toml.items():
                        if key.startswith("cfg"):
                            stack.append(value)
                return list(acc)

            dependencies = return_dependencies(member_data)
            crates.append(Crate(dependencies, member_data, cargo_file_path=member))

        self.crates = crates
        self.crate_index: typing.Dict[str, Crate] = {c.name: c for c in crates}

        self.create_publish_order()

        self.starting_dir = os.getcwd()

    def create_publish_order(self):
        """Creates a valid publish order by topologically
        sorting crates using the dependency graph."""
        topological_sorter = TopologicalSorter()

        for crate in self.crates:
            topological_sorter.add(crate.name, *crate.dependencies)

        self.publish_order: typing.List[str] = [*topological_sorter.static_order()]

    def is_crate_already_published(self, crate_name: str) -> bool:
        """Checks if a given crate name is already published
        with the version string we intend to publish with."""
        found_string: str = get_latest_version_for_crate(crate_name)
        if found_string is None:
            return False

        if self.version == found_string:
            return True
        
        crate = self.crate_index[crate_name]
        with open(crate.path + "/Cargo.toml", "rb") as file:
            data = tomllib.load(file)
        crate_version = data["package"]["version"]

        if crate_version is None:
            return False

        return crate_version == found_string

    def publish_crate(self, crate_name: str):
        # pylint: disable=broad-except
        """Publish a given crate by name."""
        status = None
        try:
            crate = self.crate_index[crate_name]
            os.chdir(crate.path)
            extra_args = []
            if crate_name in SETTINGS["publish_features"]:
                extra_args = ["--features", SETTINGS["publish_features"][crate_name]]
            if self.dry_run:
                print(f"In dry-run: not publishing crate `{crate_name}`")
                command = ["cargo", "publish", "--dry-run"] + extra_args
                if self.verbose:
                    print(*command)
                output = subprocess.run(command, check=True)
                if self.verbose:
                    print(output)
            else:
                command = ["cargo", "publish"] + extra_args
                if self.verbose:
                    print(*command)
                output = subprocess.run(command, check=True)
                if self.verbose:
                    print(output)
            if self.verbose:
                print("Success.")
        except Exception as exc:
            if self.verbose:
                print(f"Failed to publish {crate_name}.")
            print(exc)
            status = exc
        finally:
            os.chdir(self.starting_dir)
        return status

    def publish(self):
        # pylint: disable=too-many-branches
        """Publish all packages in workspace."""
        if self.verbose and self.dry_run:
            print("(Dry run, not actually publishing anything)")
        if self.verbose:
            print("Publishing order:")
            pprint(self.publish_order)
        status = {}
        failures = 0

        for crate_name in self.publish_order:
            print(f"Publishing `{crate_name}`...")
            if not self.is_crate_already_published(crate_name):
                status[crate_name] = self.publish_crate(crate_name)
                if status[crate_name]:
                    failures = +1
            else:
                print(f"`{crate_name}` was already published!")
                continue

            # sleep for 16 seconds between crates to ensure the crates.io
            # index has time to update
            if not self.dry_run:
                print(
                    "Sleeping for 16 seconds to allow the `crates.io` index to update..."
                )
                time.sleep(16)
            else:
                print("In dry-run: not sleeping for crates.io to update.")
        if failures > 0 and self.verbose:
            print(f"encountered {failures} failures.")
            for key, value in status.items():
                if value is None:
                    result = "ok"
                else:
                    result = str(value)
                print(f"{key}\t{result}")
        if self.verbose:
            print(f"Published {len(status) - failures} crates.")
        return failures


def main():
    """Main executable function."""
    os.environ["WASMER_PUBLISH_SCRIPT_IS_RUNNING"] = "1"
    parser = argparse.ArgumentParser(
        description="Publish the Wasmer crates to crates.io"
    )
    subparsers = parser.add_subparsers(dest="subcommand")
    health_cmd = subparsers.add_parser(
        "health-check",
        help="""Check the dependency graph is a tree, meaning a non-cyclic
        planar graph. Combine with verbosity to print a topological sorting
        of the graph.""",
    )
    health_cmd.add_argument(
        "-v",
        "--verbose",
        default=False,
        action="store_true",
        help="Be verbose.",
    )
    health_cmd.add_argument(
        "--print-dependencies",
        default=False,
        action="store_true",
        help="For each crate, print its dependencies.",
    )
    publish_cmd = subparsers.add_parser(
        "publish", help="Publish Wasmer crates to crates.io."
    )
    publish_cmd.add_argument(
        "--version",
        default=None,
        type=str,
        help="""Define the semver target triple (Default is automatically
        read from workspace Cargo.toml.""",
    )
    publish_cmd.add_argument(
        "--dry-run",
        default=False,
        action="store_true",
        help="""Run the script without actually publishing anything to
        crates.io""",
    )
    publish_cmd.add_argument(
        "-v",
        "--verbose",
        default=False,
        action="store_true",
        help="Be verbose.",
    )

    args = parser.parse_args()
    if args.subcommand == "health-check":
        verbose = args.verbose
        publisher = Publisher(verbose=verbose)
        if verbose:
            print(f"Version is {publisher.version}")
            pprint(publisher.publish_order)
        if args.print_dependencies:
            for crate in publisher.crates:
                print(f"{crate.name}: {crate.dependencies}")
        return 0

    if args.subcommand == "publish":
        verbose = args.verbose
        publisher = Publisher(dry_run=args.dry_run, verbose=verbose)
        return publisher.publish()
    return 0


if __name__ == "__main__":
    main()
