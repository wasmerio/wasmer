// Preloaded into Node by `make test-js-jspi`.
//
// An async test that waits for something nothing will bring leaves Node with
// nothing to do. Node then exits, with success, and the tests that had not
// run yet are never heard of: a hang reads as a pass. The test runner exits
// explicitly when it is done, so Node running out of work is always a hang.
process.on("beforeExit", () => {
  console.error("error: a test waits for something that will never happen");
  process.exit(1);
});
