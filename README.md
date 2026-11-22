<div align="center">

# Janus (Joint Adversarial Network for Unified Software)

*Creation challenged, Perfection forged.*

<sub>Local-first · Multi-agent · Replayable</sub>

[English](README.md) · [中文](README.zh-CN.md)

</div>

> A gate has two faces. One looks forward, to what can be built. The other looks back, at what must hold.

Janus turns your coding agents into a team that runs on your Mac. You give a goal to a lead agent. The lead does the work itself or splits it among digital employees, checks what comes back, and delivers. You only step in for the decisions that are yours to make. Every session, subtask, approval and delivery is recorded as an event you can replay.

The current version is an MVP. It connects Codex and Claude Code through the Agent Client Protocol (ACP); support for more agents comes later.

## Why Janus

One coding agent is a tool. Run four at once and you become their manager. You carry one agent's output to another for review. You decide who fixes what the reviewer found. You restart the one that stalled, keep two of them from editing the same file, and wake them up when their quota comes back.

The models are good enough. What is missing is the organization around them: someone who owns the goal, a place where work and questions are visible, a rule for what reaches you, and a way to check the result before it lands. Janus is that organization.

## How it works

- **A lead that manages.** The lead drives your goal with `/goal`. If the goal is small, it does the work in its own session. Otherwise it submits subtasks, with an assignee, dependencies and acceptance criteria for each. Once it has delegated, it only manages: it sends work back, reassigns it, or checks it, and never takes it over. Collaborators do not message each other; everything goes through the lead and the shared board.
- **You decide only what is yours.** The lead settles technical choices and trade-offs itself. Approvals, decisions the lead hands up, and deliveries waiting for acceptance reach your inbox, from every workspace. You can step into any agent's session at any time to steer it or take over.
- **Your machine stays yours.** Janus never writes your own agent configuration. On a Git repository, each run works in its own worktree, started from your `HEAD`, and accepting it fast-forwards one commit onto the branch you were on. Janus never leaves a half-done merge in your checkout or overwrites your uncommitted changes. Agents see a mirror of your home directory, so `gh`, SSH and your other tools keep working, while the paths you hide (by default, other agents' configuration and high-risk credentials) stay out of reach.
- **Replayable by default.** Every session, orchestration step, approval, artifact and delivery is an event, so a run can be inspected and resumed instead of reconstructed from memory.
- **Native permissions.** Each agent keeps its own permission modes. Janus adds no second switchboard. An employee with no mode set starts in Codex's Full access or Claude Code's Bypass permissions, so goals run without stopping for approvals.

## From a run to a Loop

A run is one goal, from start to delivery. A Loop keeps a workspace's lead on duty. It wakes on a schedule you set, looks around, answers what a sentence can answer, and dispatches what needs changing as new runs. It asks you only what only you can decide, and everything it does lands in your inbox.

Runs never depend on each other, and there is no time or token budget. The only limit is the machine: by default up to 8 runs work at once and up to 12 agent processes stay open. The rest wait in a queue and start as slots free up.

## Built the same way

Janus is developed by agents. Its own changes are written, reviewed and merged by coding agents working under [AGENTS.md](AGENTS.md). Each rule in its pre-merge checklist comes from something that went wrong. For example, a run once deleted files inside the repository, and a collaborator once impersonated the lead on the board. Each of those incidents is recorded in [LESSONS.md](LESSONS.md). Every change ends with `just e2e`, which runs every user journey on a real build of the service and the app and checks what each page shows against the service's own records.

## Core concepts

| Concept | What it means |
| --- | --- |
| Agent | A launchable coding agent, Codex or Claude Code in this version, with its own configuration directory and credentials. |
| Employee | A digital employee: an agent bound to a name, avatar, responsibilities, instructions, model, reasoning effort and permission mode. Its instructions go into the agent's system layer. |
| Library | Skills and MCP servers, kept once and given to every employee. Each session gets one skill, `janus-skills`, a catalog the agent searches when it needs a skill, and one MCP server, `janus-tools`, which starts or connects the real servers on first use. No agent configuration file is written. Import skills from a GitHub repository, copy them from a local folder, or write them in the app; add MCP servers with a form or pasted JSON, or in one click from Recommended. |
| Workspace | A local directory, a team of employees with one lead, and shared context: background that every run and Loop in the workspace starts from. |
| Run | A goal submitted in a workspace, followed from creation through delivery and acceptance. |
| Subtasks and board | The work the lead delegates, each with an assignee, dependencies and acceptance criteria. Janus validates them before they run. Progress, blockers and questions go on the shared board. |
| Loop | A workspace's lead on a schedule: presets or any standard five-field cron expression, in your time zone. It dispatches runs and asks you questions, and both land in the inbox. |
| Chat | A one-to-one conversation with a single employee, outside any workspace, in a fresh directory that is deleted with the chat. It uses the same session view as a run: steer, take over, approve, paste images, switch model and mode. |
| Inbox | Everything waiting on you across all workspaces. Choices are answered in place, and what nobody is blocked on can wait until later. |

## Quick start

### The macOS app (recommended)

1. Download `Janus-<version>.dmg` from [GitHub Releases](https://github.com/duoyuli/Janus/releases) and drag Janus into Applications.
2. On first launch, macOS may say the developer cannot be verified (the package is not signed with an Apple certificate). Open System Settings › Privacy & Security and choose Open Anyway.
3. Requires an Apple silicon Mac (M series) and macOS 15 or later: Janus supports the three latest major versions of macOS (15, 26 and 27). The app bundles the Janus service and Node.js, so neither Rust nor Node is required. The Codex and Claude Code adapters bundle their CLIs.

The app ships with its own Janus service and works out of the box. If you already run Janus with `just start`, the app connects to that service. See [Deployment](docs/operations/20261031-deployment.md#macos_app). Later releases install from Settings › General › Updates: only the changed parts download, and the app restarts into the new version.

Press ⌘, to open Janus Settings. In General › Language, choose Follow System, 简体中文, or English. Changes take effect immediately and persist across launches.

After the first installation, or when Janus’s signing identity changes (for example when upgrading from a release before 2026.11.18.5), Janus shows a macOS permission checklist; updates with the same signature keep the permissions you granted. The checklist is also available under Settings › General › macOS Permissions. One button starts sequential guidance for Full Disk Access, App Management and Screen Recording; you still switch each one on in macOS, and Janus detects the status of all three. Agents started by a separate service may need their own authorization. See [macOS permissions](docs/architecture/20261031-frontend.md#macos_permissions).

### From source

#### Prerequisites

- The Rust toolchain and Cargo (edition 2024), Swift 6 (Xcode or the Command Line Tools), and [just](https://github.com/casey/just).
- Node.js 22.19 or later, and the npm that ships with it.
- Built-in adapters bundle the Codex and Claude Code CLIs.
- The first use of an agent needs network access to download its ACP adapter.

#### Install and start

From the repository root:

```sh
just install   # build and install to ~/.janus/bin/janus
just start     # run as a service: starts automatically at login (macOS); just run runs it in the foreground
```

Then `just mac` runs the app from source and connects it to this service; `just dmg` builds `Janus.app` and a DMG locally. For development, `just dev` starts the service from source. Running `just` without arguments lists every command.

### First run

1. Add an agent under Settings › Employees and run the configuration check. Its ACP adapter downloads at this step; later you can check for updates, upgrade, or roll back from the adapter line on the agent's card.
   To connect Claude through an API, select **Custom API** under authentication, then click **Insert Template** or paste your `export KEY=value` lines for the endpoint, token, and models.
2. Bind employees to agents on the same page, choose a model and permission mode, and turn on **Can Be Lead** for the ones that should lead.
3. Create a workspace: choose a directory, pick its team, and name a lead.
4. Enter a goal on the workspace home page and choose **Send to Lead**.

The repository holds code only. Janus data, adapters, configuration, and logs live in `~/.janus`; see [Deployment](docs/operations/20261031-deployment.md).

`examples/calc/` is a minimal example project. Copy it outside the repository (for example `cp -R examples/calc ~/calc`) before using it as a workspace. Make the copy a Git repository with one commit (`git init && git add -A && git commit -m init`): each run then works in its own worktree, keeps a diff, and when you accept it Janus commits the result and fast-forwards the branch you were on, so it lands in your directory. Without Git, the lead and subtasks edit the directory in place and keep no diff; if another run is already working there, Janus warns you but does not stop you. Verify it with `python3 -m unittest discover -s examples/calc`.

## Documentation

Engineering documentation starts at [docs/index.md](docs/index.md) and is organized into seven areas:

| Area | Contents |
| --- | --- |
| [Architecture](docs/architecture/index.md) | Components and process model, storage layout, ACP adapter management, macOS app |
| [Domains](docs/domains/index.md) | Run lifecycle, goal mode and shared board, agents / employees / workspaces and permission modes, resource library |
| [Interfaces](docs/interfaces/index.md) | HTTP API, events, board MCP tools, ACP integration, configuration |
| [Operations](docs/operations/index.md) | Deployment, CI and releases, runbook, testing, security boundaries |
| [Decisions](docs/decisions/index.md) | Architecture decision records (ADRs) |
| [Research](docs/research/index.md) | Reports on external multi-agent and agent-team work, with takeaways for this project |
| [Review](docs/review/index.md) | Code review and audit reports |

Agents working in this repository should read [AGENTS.md](AGENTS.md) first (`CLAUDE.md` points to the same file).

## The name

Janus is the Roman god of gates, beginnings, and transitions. He has two faces: one turned toward the future, where work is created, and one turned toward the past, where work is examined. A gate is both a passage and a threshold, and a delivery is not real until it has survived scrutiny.

**Joint Adversarial Network for Unified Software** puts that image into engineering terms: agents with different models, contexts and blind spots work on the same problem, and each one's work is challenged by another. What survives is delivered.

## Scope

Janus is built for one machine and one person. Runs change the directory you choose (or a worktree that is merged back when you accept). Agents can read and write everything in your home directory except the paths you hide, the service has no authentication, and there is no OS-level sandbox. Read the [security boundaries](docs/operations/20261031-security.md) before pointing it at a real project.
