# GitHub Copilot Instructions

## Code Writing Standards
- Follow established code-writing standards for your language (spacing, comments, naming).
- Consider internal coding rules for folder and function naming.
- Follow the "boy scout rule": Always leave the codebase cleaner than you found it.

## Comment Usage
- Use comments sparingly and make them meaningful.
- Avoid commenting on obvious things; use comments to explain "why" or unusual behavior.

## Conditional Encapsulation
- Encapsulate nested if/else statements into functions with descriptive names for clarity.

## DRY Principle
- Avoid code duplication; reuse code via functions, classes, modules, or libraries.
- Modify code in one place if updates are needed.

## Function Length & Responsibility
- Write short, focused functions (single responsibility principle).
- Break up long or complex functions into smaller ones.

## General Code Style & Readability
- Write readable, understandable, and maintainable code.
- Prioritize clarity and adhere to coding standards.
- Regularly review and refactor code for structure and maintainability.
- Use version control (e.g., Git) for collaboration.

## Naming Conventions
- Use meaningful, descriptive names for variables, functions, and classes.
- Names should reflect purpose and behavior; avoid names that require comments to explain intent.

## Making your changes pass CI
- Before submitting any changes
  - Run the commands for formatting
  - Run the linter command shown below
  - Run tests for code you changed and everything that depends on it.
- CI will reject code with formatting or linting issues.

## Useful commands:
- Format the code: `cargo fmt`
- Lint and fix common mistakes: `RUSTFLAGS="-D dead-code -D nonstandard-style -D unused-imports -D unused-mut -D unused-variables -D unused-unsafe -D unreachable-patterns -D bad-style -D improper-ctypes -D unused-allocation -D unused-comparisons -D while-true -D unconditional-recursion -D bare-trait-objects -D function_item_references -D clippy::uninlined_format_args " cargo clippy --all --exclude wasmer-swift --locked --fix --allow-dirty -- -D clippy::all`
- Build the cli: `cargo build -p wasmer-cli --features cranelift,llvm,wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load`
- Test the cli: `cargo test -p wasmer-cli --features cranelift,llvm,wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load`
- Test WASIX: `cargo test -p wasmer-wasix --features sys`
