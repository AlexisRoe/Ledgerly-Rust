# Ledgerly-Rust

A drop-in Rust reimplementation of [Ledgerly-Java](https://github.com/AlexisRoe/Ledgerly-Java), built to learn Rust backend development on a system whose behaviour is already known.

Ledgerly is a REST backend where small teams record expenses, attach receipts and move each expense through an approval workflow. This repository serves the **same HTTP contract, database schema, test data, configuration and operational endpoints** as the Java version at the commit pinned in [`vendor/ledgerly-java/SOURCE`](vendor/ledgerly-java/SOURCE). You can point it at a database written by Java and it keeps working ([ADR 0005](docs/adr/0005-drop-in-replacement-on-the-same-data.md)). Security by design is the top priority.

## Start here

| Document | What it answers |
| --- | --- |
| [`MISSION.md`](MISSION.md) | Why this repository exists and what success looks like |
| [`CONCEPT.md`](CONCEPT.md) | Architecture, crate layout, security by design, definition of done |
| [`GLOSSARY.md`](GLOSSARY.md) | The domain language (shared with Java; it wins every naming argument) |
| [`docs/adr/`](docs/adr/) | Decisions and why they were made |
| [`docs/implementation-plan.md`](docs/implementation-plan.md) | Build order: milestones M0–M11, step by step |
| [`docs/local-setup.md`](docs/local-setup.md) | What to install and how to run it |
| [`docs/deployment.md`](docs/deployment.md) | Configuration, image, runbook |
| [`docs/learning-log.md`](docs/learning-log.md) | What each milestone taught, as raw material for lessons |
| [`CLAUDE.md`](CLAUDE.md) | Rules for the coding agent that implements the plan |
| [`api/openapi.yaml`](api/openapi.yaml), [`bruno/`](bruno/) | The HTTP contract and the request collection, shared with Java and read-only |
| [`vendor/ledgerly-java/`](vendor/ledgerly-java/) | Other shared files from Java, read-only: migrations, config defaults, Keycloak realm, Java's docs. The Java source itself stays [on GitHub](vendor/ledgerly-java/SOURCE) |
| [`docs/reference/`](docs/reference/) | Extracts of the Java code and tests that the port needs |

## Quick start

Install [OrbStack](https://orbstack.dev) (or Docker Desktop) and [mise](https://mise.jdx.dev), then:

```sh
mise install          # Rust toolchain and every CLI tool, pinned
pre-commit install    # secret scan and format checks before each commit
just up               # build the image, migrate, start everything in Docker
just                  # list all tasks
```

The API listens on http://localhost:8080. Demo users and passwords are listed in [`docs/deployment.md`](docs/deployment.md#demo-data).

> **Status:** design complete, implementation not started. `just` and the Rust workspace arrive in milestone M1.
