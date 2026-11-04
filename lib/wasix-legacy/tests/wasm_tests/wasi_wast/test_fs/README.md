# Test FS

This is a test "file system" used in some of the WASI integration tests.

It's just a bunch of files in a tree.

If you make changes here, update the expected output fixtures in the parent
`wasi_wast` directory.

The contents of `temp` are deleted before each run.  If you want to test making or mutating files, do it in that sub directory.
