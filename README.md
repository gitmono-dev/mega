# Mega — Monorepo Infrastructure for the AI Agent Era

Mega is an open-source implementation of [Google Piper](https://cacm.acm.org/magazines/2016/7/204032-why-google-stores-billions-of-lines-of-code-in-a-single-repository/fulltext) — a Git-compatible monorepo engine built for AI-native engineering workflows. Written entirely in Rust, Mega is designed to manage petabyte-scale codebases while serving as the infrastructure backbone for AI coding agents.

## Why Mega?

AI coding agents are becoming first-class participants in software engineering. But today's version control systems were designed for human developers working in isolated branches — they lack the unified context, structured metadata, and programmatic interfaces that agents need to operate reliably at scale.

Monorepos solve the context problem. When an agent can see the entire codebase — dependencies, downstream consumers, build targets, and test coverage — it makes better decisions, produces fewer hallucinations, and delivers atomic cross-project changes in a single commit.

**Mega brings Google-scale monorepo infrastructure to the open-source world, purpose-built for the agentic future.**

## Mega + Libra: Version Control for Agents

Mega works together with [**Libra**](https://github.com/web3infra-foundation/libra), our Rust-based, Git-compatible client with SQLite-backed storage, to provide a complete version control workflow where AI agents are tracked, attributable contributors:

- **Mega** (server-side) — The centralized monorepo engine. Manages code at scale with full codebase context, trunk-based development, and fine-grained access control. Provides the global visibility that agents need for dependency analysis, impact assessment, and cross-project reasoning.
- **Libra** (agent-side) — A lightweight, embeddable Git client optimized for programmatic access. Agents use Libra to clone, commit, and push with structured metadata and intent tracking — no shell-out to `git` required.

Together, they enable a new paradigm: **from intent to merge, every agent action is versioned, attributed, and traceable.**

## Features

### Git Compatible

Mega offers full Git protocol support with a monorepo. Clone or pull any folder in the monorepo into your local filesystem as a standard Git repository, and seamlessly push changes back. Both human developers and AI agents interact through the same familiar Git interface.

### Trunk-Based Development

Large-scale codebases thrive on trunk-based development — a single source of truth, continuous integration, and short-lived branches. This model is especially critical for AI agents, which benefit from always operating against the latest, consistent state of the codebase. Learn more at [Trunk-Based Development](https://trunkbaseddevelopment.com/).

### Conventional Commits

Mega supports [Conventional Commits](https://www.conventionalcommits.org/), enabling both humans and agents to produce structured, machine-readable commit messages that power automated changelogs, semantic versioning, and audit trails.

### Scorpio — FUSE Filesystem for Monorepo

[Scorpio](https://github.com/web3infra-foundation/scorpiofs) is a FUSE filesystem that mounts any monorepo folder as a local filesystem. Developers and agents work with their codebase as if it were local, while Mega handles the scale underneath — no need to check out the entire repository.

### Buck2 Integration

Mega integrates [Buck2](https://buck2.build/) as its default build system. Developed by Meta in Rust, Buck2 enables declarative, reproducible, and highly parallelized builds — essential for maintaining build correctness across a monorepo that both humans and agents contribute to simultaneously.

## Roadmap

Mega is evolving toward deeper AI-native capabilities:

- **IntentSpec** — A structured, machine-readable intent contract that drives agent task execution with security policies and provenance binding.
- **Multi-Agent DAG Orchestration** — Pipeline architecture for coordinating multiple AI agents across complex, multi-step code generation workflows.
- **Code Attribution** — Line-level tracking of AI-generated vs. human-written code, enabling auditability and trust in agent contributions.

## Quick Start

To facilitate a rapid deployment and hands-on experience with the Mega service, the following instructions are derived from the project's [documentation](https://github.com/web3infra-foundation/mega/tree/main/docker).

- **Docker demo (recommended):** [docker/README.md](docker/README.md)
- **Native development:** [docs/development.md](docs/development.md)
- **Architecture:** [docs/architecture.md](docs/architecture.md)

Related projects: [Libra](https://github.com/web3infra-foundation/libra) (Git-compatible agent client), [ScorpioFS](https://github.com/web3infra-foundation/scorpiofs) (FUSE monorepo mount).

## Community

Discord Channel - https://discord.gg/HMFuu6pJmQ

## Contributing

The mega project relies on community contributions and aims to simplify getting started. To develop Mega, clone the repository, then install all dependencies and initialize the database schema, run the test suite and try it out locally. Pick an issue, make changes, and submit a pull request for community review.

### Pre-submission Checks
Before submitting a Pull Request, please ensure your code passes the following checks (run in order after modifying code):

```bash
# 1. Format Rust code (apply fixes)
cargo +nightly fmt --all

# 2. Lint Rust (warnings are errors)
cargo clippy --all-targets --all-features -- -D warnings

# 3. Check frontend formatting
pnpm -C moon prettier --check .
```

All commands must complete without errors. Clippy treats warnings as errors; Prettier must report no formatting drift.

To verify Rust formatting without writing files (CI-style):

```bash
cargo +nightly fmt --all --check
```

To auto-fix frontend formatting:

```bash
pnpm -C moon prettier --write .
```

### Buck2 Build Requirements

This project builds with Buck2. Please install both Buck2 and cargo-buckal before development:

```bash
# Install buck2: download the latest release tarball from
# https://github.com/facebook/buck2/releases, extract the binary,
# and place it in ~/.cargo/bin (ensure ~/.cargo/bin is on PATH).
# Example (replace <tag> and <platform> with the latest for your OS):
wget https://github.com/facebook/buck2/releases/download/<tag>/buck2-<platform>.tar.gz
tar -xzf buck2-<platform>.tar.gz
mv buck2 ~/.cargo/bin/

# Install cargo-buckal (requires Rust toolchain)
cargo install --git https://github.com/buck2hub/cargo-buckal.git
```

Pull Requests must also pass the Buck2 build:

```bash
cargo buckal build
```

When you update dependencies in `Cargo.toml`, regenerate Buck metadata and third-party lockfiles:

```bash
cargo buckal migrate
```

More information on contributing to Mega is available in the [Contributing Guide](docs/contributing.md).

## License

Mega is licensed under this License:

- MIT LICENSE ( [LICENSE-MIT](LICENSE-MIT) or https://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or https://www.apache.org/licenses/LICENSE-2.0)
