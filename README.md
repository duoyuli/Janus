<div align="center">

# Janus (Joint Adversarial Network for Unified Software)

*Creation challenged, Perfection forged.*

<sub>Local-first · Multi-agent · Replayable</sub>

[English](README.md) · [中文](README.zh-CN.md)

</div>

> A gate has two faces. One looks forward, to what can be built. The other looks back, at what must hold.

Janus is a local-first multi-agent workspace. Through the Agent Client Protocol (ACP), it connects Codex, Claude Code, and other ACP-compatible agents into a single team: one lead agent sets the direction, collaborators carry the work, and acceptance is decided in the open. Every session, orchestration step, approval, artifact, and delivery is recorded as a replayable event.

## The name

Janus is the Roman god of gates, beginnings, and transitions. He is shown with two faces: one turned toward the future, where work is created; one turned toward the past, where work is examined. The gate is both passage and threshold — a delivery is not real until it has survived scrutiny.

**Joint Adversarial Network for Unified Software** turns that image into an engineering goal: a network of heterogeneous agents, each with a different model, context, and temperament, working on the same problem. Creation is challenged. What survives is delivered.

*Creation challenged, Perfection forged.* is the same idea in one line.

## Why Janus

- **Local by design.** The repository holds code only. Task records, adapters, configuration, artifacts, and logs stay in `~/.janus`, on the machine where the work happens.
- **One lead, many minds.** A lead agent owns the goal and chooses what to do itself and what to delegate. Collaborators bring different models — and different blind spots.
- **Replayable by default.** Every session, orchestration step, approval, artifact, and delivery is written as an event, so a task can be inspected and resumed rather than reconstructed from memory.
- **Native permissions.** Each agent keeps the permission model it was built with. Janus does not add a second switchboard or a parallel approval layer.

## Core concepts

| Concept | What it means |
| --- | --- |
| Agent (provider connection) | A launchable coding agent, such as Codex or Claude Code. Each agent has its own configuration directory and credentials. |
| Digital employee | Binds an agent and defines its name, responsibilities, model, reasoning effort, permission mode, and the skills and MCP servers it uses from the library. |
| Resource library | Skills and MCP servers kept once and picked per digital employee. Write a skill in the app, copy a local folder, paste an MCP config, or import from a GitHub repository. Each session gets its digital employee's resources when it starts (as a plugin for Claude Code, an extra skills root for Codex, and MCP servers for both); no agent config file is written. |
| Workspace | A local directory plus a set of digital employees, one of whom is the lead. |
| Task | A goal submitted in a workspace, followed from creation through delivery and acceptance. |
| Lead agent | The agent that drives the goal with `/goal`. It can be Claude Code or Codex, and decides whether to work directly or delegate through orchestration. |
| Orchestration and shared board | The set of subtasks submitted by the lead (assignee, dependencies, acceptance criteria). Janus validates them before execution; progress, blockers, and questions are written to the shared board. |

## Quick start

### The macOS app (recommended)

1. Download `Janus-<version>.dmg` from [GitHub Releases](https://github.com/duoyuli/Janus/releases) and drag Janus into Applications.
2. On first launch, macOS may say the developer cannot be verified (the package is not signed with an Apple certificate). Open System Settings → Privacy & Security and choose Open Anyway.
3. Requires an Apple silicon Mac (M series) and macOS 14 or later. The app bundles the Janus service and Node.js, so neither Rust nor Node is required. The Codex and Claude Code adapters bundle their CLIs.

The app ships with its own Janus service and works out of the box. If you already run Janus with `just start`, the app connects to that service. See [Deployment](docs/operations/20261031-deployment.md#macos_app).

### From source

#### Prerequisites

- The Rust toolchain and Cargo (edition 2024), Swift 6 (Xcode or the Command Line Tools), and [just](https://github.com/casey/just).
- Node.js 22.19 or later, and the npm that ships with it.
- Agents of the custom-command type need their own launch command. Built-in adapters bundle the Codex and Claude Code CLIs.
- The first use of an agent needs network access to download its ACP adapter.

#### Install and start

From the repository root:

```sh
just install   # build and install to ~/.janus/bin/janus
just start     # run as a service: starts automatically at login (macOS); just run runs it in the foreground
```

Then `just mac` runs the app from source and connects it to this service; `just dmg` builds `Janus.app` and a DMG locally. For development, `just dev` starts the service from source. Running `just` without arguments lists every command.

### First run

1. Add an agent under Settings → Agents and run the configuration check. Its ACP adapter downloads at this step; later you can check for updates, upgrade, or roll back under Settings → ACP.
2. Bind digital employees to agents under Settings → Digital Employees, choose a model and permission mode, and turn on "can lead" for the ones that should lead.
3. Create a workspace: choose a directory, enable digital employees, and name one bound to Claude Code or Codex as the lead.
4. Enter a goal on the workspace home page and hand it to the lead.

The repository holds code only. Janus data, adapters, configuration, and logs live in `~/.janus`; see [Deployment](docs/operations/20261031-deployment.md).

`examples/calc/` is a minimal example project. Copy it outside the repository (for example `cp -R examples/calc ~/calc`) before using it as a workspace, since subtasks edit files in place. Verify it with `python3 -m unittest discover -s examples/calc`.

## Documentation

Engineering documentation starts at [docs/index.md](docs/index.md) and is organized into seven areas:

| Area | Contents |
| --- | --- |
| [Architecture](docs/architecture/index.md) | Components and process model, storage layout, ACP adapter management, macOS app |
| [Domains](docs/domains/index.md) | Task (run) lifecycle, goal mode and shared board, agents / digital employees / workspaces and permission modes, resource library |
| [Interfaces](docs/interfaces/index.md) | HTTP API, events, board MCP tools, ACP integration, configuration |
| [Operations](docs/operations/index.md) | Deployment, CI and releases, runbook, testing, security boundaries |
| [Decisions](docs/decisions/index.md) | Architecture decision records (ADRs) |
| [Research](docs/research/index.md) | Reports on external multi-agent and agent-team work, with takeaways for this project |
| [Review](docs/review/index.md) | Code review and audit reports |

Agents working in this repository should read [AGENTS.md](AGENTS.md) first (`CLAUDE.md` points to the same file).

## Scope

Janus is built for one machine and one person: organizing multiple coding agents to develop, research, review, document, and verify work. Subtasks modify the directory you choose, the service has no authentication, and there is no OS-level sandbox. Read the [security boundaries](docs/operations/20261031-security.md) before pointing it at a real project.
