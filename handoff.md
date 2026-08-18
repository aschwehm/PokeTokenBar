# Handoff — PokeTokenBar (Linux/Windows port)

_Last updated: 2026-08-18_

## What this is

A from-scratch port of [chattymin/PokeTokenBar](https://github.com/chattymin/PokeTokenBar) — a
macOS menu-bar app that turns AI-coding token usage (Claude Code, Codex, Gemini, …) into a
grow-and-collect Pokémon companion — to **Linux** (now complete) and **Windows** (next).

The original is Swift 6 + SwiftUI + AppKit. This port is **Tauri 2** (Rust core + Svelte frontend).
The Swift test suite was used as the executable spec for the Rust logic.

**Current status: Linux port complete, fully featured & passing 332 tests.** All 10 local usage
providers ported, desktop notifications active, Claude official OAuth rate limits wired, floating
desktop pet overlay window added, and native window decorations / dragging operational.

## How to build / test / run

```bash
cd ~/PokeTokenBar
cargo test               # 332 tests (run from src-tauri/, or with --manifest-path src-tauri/Cargo.toml)
npm run check            # svelte-check
npm run build            # vite build (needed before any cargo build that embeds assets)
npm run tauri dev        # run the app (window opens on launch; left-click tray toggles)
```

Quality gates (all currently green):
```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

### Environment already set up on this machine

- **Rust** via rustup (`~/.cargo/bin`, on PATH via `.bashrc`/`.profile`).
- **Tauri system deps** installed via pacman: `webkit2gtk-4.1`, `base-devel`, `libappindicator-gtk3`
  (+ `ayatana-appindicator`), `librsvg`, `xdotool`, `openssl`, `appmenu-gtk-module`.
- **Desktop:** GNOME on Wayland with native window controls (`decorations: true`) and system tray.
- Node 26 + npm 12; frontend deps already `npm install`ed.

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

## What's ported (Swift → Rust)

| Swift file | Rust | Notes |
|---|---|---|
| Models.swift | domain/models.rs | usage/limit models, ISO8601 |
| ModelPricing.swift | domain/pricing.rs | rate table + exact fallback order |
| TokenFormatter.swift | domain/format.rs | compact/grouped/cost/percent |
| CompanionModel.swift | domain/companion.rs | rarity/balance/natures/evo/shop/state |
| SaveTransfer.swift | domain/save.rs | envelope + sanitize + rebase |
| BinaryLocator.swift | platform/binary_locator.rs | cached binary + login-shell env (`bash -ilc`) |
| UsageEnvironment.swift | platform/env.rs | single-spawn batched env lookup |
| AppEnv/AppLog.swift | platform/app_env.rs, log.rs | XDG + %LOCALAPPDATA% logging |
| LocalUsageReader.swift | providers/reader.rs | Claude/Codex/Gemini/Grok parsers + Codex fork/replay |
| LocalUsageCache.swift | providers/cache.rs | incremental disk cache |
| LocalAntigravityUsageReader.swift | providers/antigravity.rs | Antigravity SQLite + Protobuf wire parser |
| LocalAdditionalUsageProvider.swift | providers/additional.rs | OpenCode, Hermes Agent, Cursor, Copilot CLI, Kiro CLI |
| OAuthLimitsProvider.swift | providers/claude_limits.rs | Claude official 5h & 7d limits from ~/.claude/.credentials.json |
| PokeAPIClient.swift | providers/pokeapi.rs | species/evolution fetch + GraphQL index + 30d disk cache |
| CompanionStore.swift | companion/store.rs | egg→hatch→evolve→graduate, shop/candy/ditto |
| UsageStore.swift | companion/usage_store.rs | aggregation + burn tier + all 10 providers |
| — | integration/app.rs, tray.rs, notify.rs | Tauri command bridge, tray menu, OS desktop notifications |

**Providers working (10 of 10):**
Claude Code, Codex, Gemini, Grok, Antigravity CLI, OpenCode, Hermes Agent, Cursor, Copilot CLI, Kiro CLI.

## Immediate next steps

1. Run `npm run tauri dev` to test the floating pet overlay window (`🐾` button in header or tray menu).
2. Validate Windows build in CI / Windows VM.
