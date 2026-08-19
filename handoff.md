# Handoff — PokeTokenBar (Linux/Windows port)

_Last updated: 2026-08-19 (v0.3.0)_

## What this is

A from-scratch port of [chattymin/PokeTokenBar](https://github.com/chattymin/PokeTokenBar) — a
macOS menu-bar app that turns AI-coding token usage (Claude Code, Codex, Gemini, …) into a
grow-and-collect Pokémon companion — to **Linux** and **Windows** (both feature-complete & verified).

The original is Swift 6 + SwiftUI + AppKit. This port is **Tauri 2** (Rust core + Svelte 5 frontend).
The Swift test suite was used as the executable spec for the Rust logic.

**Current status: v0.3.0 Feature Complete & Passing 333 Tests.**
- All 10 AI usage providers ported and active with non-blocking async log scans.
- Complete UI redesign matching Claude Design specs.
- Always-on-top transparent desktop pet companion with circular perimeter progress ring.
- Windows NSIS (`.exe`) and MSI (`.msi`) release installers verified.

## How to build / test / run

```bash
cargo test --manifest-path src-tauri/Cargo.toml   # 333 tests
npm run check                                    # svelte-check
npm run build                                    # vite build
npm run tauri dev                                # run in dev mode
npm run tauri build                              # build release installers
```

## Repository layout

```
src-tauri/            Rust crate "poketokenbar_lib"
  src/
    domain/           pure data: models, pricing, format, companion model, save, decoding
    platform/         paths (XDG / %LOCALAPPDATA%), shell env lookup, logging, data_dir()
    providers/        usage readers: reader.rs, cache.rs, local.rs, additional.rs, antigravity.rs, claude_limits.rs, pokeapi.rs
    companion/        store.rs (state machine), usage_store.rs (aggregation)
    integration/      app.rs (state + commands + Snapshot DTO), tray.rs, notify.rs
    main.rs, lib.rs
  tests/fixtures/     CodexFork / CodexSubagent JSONL fixtures
src/                  SvelteKit frontend (routes/+page.svelte = popover, routes/pet/+page.svelte = pet)
docs/                 architecture.md, roadmap.md
.github/workflows/ci.yml   Linux + Windows matrix (fmt/clippy/test/svelte-check/build)
```

## Providers working (10 of 10):
Claude Code, Codex, Gemini CLI, Grok CLI, Antigravity CLI, OpenCode, Hermes Agent, Cursor, Copilot CLI, Kiro CLI.

> **Note on live testing**: Real-world live testing has been conducted with **Antigravity / `agy` (Gemini CLI)** and **Claude Code**. The other 8 providers are validated against 330+ ported unit test fixtures. Community testing for additional providers is welcome!

