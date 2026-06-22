# Contributing to ayx-rs

Thank you for your interest. This document covers how to build, test, and submit changes.

## Prerequisites

- Rust stable toolchain via [rustup](https://rustup.rs/)
- `cargo fmt` and `cargo clippy` are included with the stable toolchain
- Install `cargo-nextest` once with `cargo install --locked cargo-nextest`

## Building locally

    git clone https://github.com/RyanMerlin/ayx-rs.git
    cd ayx-rs
    cargo build --workspace

## Running tests

    cargo nextest run --workspace --locked

## Documentation site

The published docs live in `docs-site/` and are built with Docusaurus.
The site syncs checked-in docs from `docs/` at build time, so the root docs
remain the source of truth.

To validate the docs site locally:

    cd docs-site
    npm ci
    npm run build

## Before opening a PR

Run these locally before pushing — CI enforces all three:

    cargo fmt --all
    cargo clippy --workspace --all-targets -- -D warnings
    cargo nextest run --workspace --locked

## Commit style

Keep commits focused. Write messages in the imperative: "Add X", "Fix Y", "Remove Z".

## Opening a pull request

Use the PR template and fill in the checklist before marking ready for review.
Draft PRs are welcome for early feedback.

## Reporting issues

Use the bug report or feature request issue templates.
Check existing issues before opening a new one.
