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

- **Local by design.** The repository holds code only. Runs, adapters, configuration, artifacts, and logs stay in `~/.janus`, on the machine where the work happens.
- **One lead, many minds.** A lead agent owns the goal and chooses what to do itself and what to delegate. Collaborators bring different models — and different blind spots.
- **Replayable by default.** Every session, orchestration step, approval, artifact, and delivery is written as an event, so a run can be inspected and resumed rather than reconstructed from memory.
- **Native permissions.** Each agent keeps the permission model it was built with. Janus does not add a second switchboard or a parallel approval layer.

## Core concepts

| Concept | What it means |
| --- | --- |
| Agent (provider connection) | A launchable coding agent, such as Codex or Claude Code. Each agent has its own configuration directory and credentials. |
| Role | Binds an agent and defines its name, responsibilities, model, reasoning effort, permission mode, skills, and MCP. |
| Skill library | Skills available to roles. Create them in the app, import a local folder, or paste a GitHub repository URL under Settings › Skill Library to discover every skill in the repository. |
| Workspace | A local directory plus a set of role members, one of whom is the lead. |
| Run | A goal submitted in a workspace, followed from creation through delivery and acceptance. |
| Lead agent | The agent that drives the goal with `/goal`. It can be Claude Code or Codex, and decides whether to work directly or delegate through orchestration. |
| Orchestration and shared board | The set of tasks submitted by the lead (assignee, dependencies, acceptance criteria). Janus validates them before execution; progress, blockers, and questions are written to the shared board. |

## Quick start

### The macOS app (recommended)

1. Download `Janus-<version>.dmg` from [GitHub Releases](https://github.com/duoyuli/Janus/releases) and drag Janus into Applications.
2. On first launch, macOS may say the developer cannot be verified (the package is not signed with an Apple certificate). Open System Settings → Privacy & Security and choose Open Anyway.
3. Requires an Apple silicon Mac (M series) and macOS 14 or later. The app bundles the Janus service and Node.js, so neither Rust nor Node is required. The Codex and Claude Code adapters bundle their CLIs.

The app ships with its own Janus service and works out of the box. If you already run Janus with `just start`, the app connects to that service. See [Deployment](docs/operations/deployment.md#macos_app).

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

1. Add an agent under Settings → Agents and run the configuration check. Its ACP adapter downloads at this step; later you can check for updates, upgrade, or roll back under Settings → ACP Adapters.
2. Bind roles to agents under Settings → Roles, and choose a model and permission mode.
3. Create a workspace: choose a directory, add members, and name a Claude Code or Codex role as the lead.
4. Enter a goal on the workspace home page and hand it to the lead.

The repository holds code only. Janus data, adapters, configuration, and logs live in `~/.janus`; see [Deployment](docs/operations/deployment.md).

`examples/calc/` is a minimal example project. Copy it outside the repository (for example `cp -R examples/calc ~/calc`) before using it as a workspace, since tasks edit files in place. Verify it with `python3 -m unittest discover -s examples/calc`.

## Documentation

Engineering documentation starts at [docs/index.md](docs/index.md) and is organized into five areas:

| Area | Contents |
| --- | --- |
| [Architecture](docs/architecture/index.md) | Components and process model, storage layout, ACP adapter management, macOS app |
| [Domains](docs/domains/index.md) | Run lifecycle, goal mode and shared board, agents / roles / workspaces and permission modes |
| [Interfaces](docs/interfaces/index.md) | HTTP API, events, board MCP tools, ACP integration, configuration |
| [Operations](docs/operations/index.md) | Deployment, CI and releases, runbook, testing, security boundaries |
| [Decisions](docs/decisions/index.md) | Architecture decision records (ADRs) |

Agents working in this repository should read [AGENTS.md](AGENTS.md) first (`CLAUDE.md` points to the same file).

## Scope

Janus is built for one machine and one person: organizing multiple coding agents to develop, research, review, document, and verify work. Tasks modify the directory you choose, the service has no authentication, and there is no OS-level sandbox. Read the [security boundaries](docs/operations/security.md) before pointing it at a real project.
