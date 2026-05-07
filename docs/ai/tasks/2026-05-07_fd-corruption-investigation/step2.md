we are working on  ./step1.md

We already have a list of potential suspects in  ./potential-problems.md

For each of these, spawn a subagent to do a thorough investigation. 
The agent should write findings out to a ./findings/<problem>.md file
and output a short valid/invalid response

If the problem seems valid, add a reproduction test first using a subagent.
The subagent should read lib/wasix/tests/wasm_tests/README.md, the problem findings, and write and run a test to reproduce the issue.
Then it should output if the test was able to be reproduces.
Note: the subagent can use 'nix shell github:wasix-org/wasinix#wasixcc' to get a working wasixcc in this environment.

If the problem was reproducable, spawn yet another subagent to actually fix the problem, re-run the tests to verify, and then commit the changes.
It should output "Fixed" to the parent.

Go one by one through the potential problems.

NOTE: YOU ARE THE PARENT ORCHESTRATOR.
KEEP YOUR OWN TOKEN USAGE LOW. DO NOT READ FULL REPORTS AND FINDINGS, JUST DELEGATE WORK TO SUBAGENTS!

