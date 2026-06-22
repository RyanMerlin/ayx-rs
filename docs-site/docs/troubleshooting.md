---
title: Troubleshooting
sidebar_position: 2
---

# Troubleshooting

## Start with doctor

`ayx doctor` validates configuration, auth, and network connectivity without touching remote state.

```bash
ayx doctor
ayx one doctor discover
```

## Reference surfaces

| Problem | Go to |
|---------|-------|
| Command not found or unexpected behavior | [Command surface](./reference/command-surface) |
| Config resolution order or profile shape | [Runtime config contract](./reference/runtime-config-contract) |
| CLI flags and stable behavior contract | [CLI spec](./reference/cli-spec) |
| API paths and parameters | [API Reference](/reference/api/) |

## Site vs. binary disagreement

If this site and your local binary disagree, trust the binary and the checked-in release notes first. The command surface page is regenerated from the live clap tree on every CI run — check which release version you are on with `ayx --version` and compare it against the [release notes](./releases).
