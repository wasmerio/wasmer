we are seeing issues in lib/wasix around file system handling.

we are seeing corruption issues, where data intended for a different file (eg stdout/err)
actually ends up being written to another file.

one potential is file descriptor staleness/confusion bugs in the
wasix file system stack.

do a superfifical code base search, and find potential causes. do not
dig into specific potential causes too deeply yet, just collect them and write them
to a FDCORRUPTION.md file
