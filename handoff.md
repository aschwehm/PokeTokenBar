# Handoff — PokeTokenBar (Linux/Windows port)

_Last updated: 2026-08-18_

## What this is

A from-scratch port of [chattymin/PokeTokenBar](https://github.com/chattymin/PokeTokenBar) — a
macOS menu-bar app that turns AI-coding token usage (Claude Code, Codex, Gemini, …) into a
grow-and-collect Pokémon companion — to **Linux** (now) and **Windows** (later).

The original is Swift 6 + SwiftUI + AppKit. This port is **Tauri 2** (Rust core + Svelte frontend).
The Swift test suite was used as the executable spec for the Rust logic.

**Current status: Linux MVP complete and buildable.** Backend fully ported (315 tests), tray +
popover UI wired. Not yet run on a real desktop (no GUI in the build environment).

## How to build / test / run

```bash
cd ~/PokeTokenBar
cargo test               # 315 tests (run from src-tauri/, or use `cd src-tauri`)
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
- **Desktop:** GNOME on Wayland. GNOME hides tray icons by default → for tray use, install/enable
  `gnome-shell-extension-appindicator` (`appindicatorsupport@rgcjonas.gmail.com`). The window is
  currently set `visible: true` so the app is usable without the tray.
- Node 26 + npm 12; frontend deps already `npm install`ed.

## Repository layout

```
src-tauri/            Rust crate "poketokenbar_lib"
  src/
    domain/           pure data: models, pricing, format, companion model, save, decoding
    platform/         paths (XDG), shell env lookup, logging, data_dir()
    providers/        usage readers: reader.rs (parsers), cache.rs, local.rs, pokeapi.rs, mod.rs (trait)
    companion/        store.rs (state machine), usage_store.rs (aggregation)
    integration/      app.rs (state + commands + Snapshot DTO), tray.rs
    main.rs, lib.rs
  tests/fixtures/     CodexFork / CodexSubagent JSONL fixtures
src/                  SvelteKit frontend (routes/+page.svelte = the popover)
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
| — | domain/decoding.rs | lenient serde helpers |
| BinaryLocator.swift | platform/binary_locator.rs | cached binary + login-shell env (`bash -ilc`) |
| UsageEnvironment.swift | platform/env.rs | single-spawn batched env lookup |
| AppEnv/AppLog.swift | platform/app_env.rs, log.rs | |
| LocalUsageReader.swift | providers/reader.rs | Claude/Codex/Gemini/Grok parsers + Codex fork/replay |
| LocalUsageCache.swift | providers/cache.rs | incremental disk cache |
| LocalUsageProvider.swift | providers/local.rs | 4 providers + UsageProvider trait |
| PokeAPIClient.swift | providers/pokeapi.rs | species/evolution + GraphQL index + 30d disk cache |
| CompanionStore.swift | companion/store.rs | egg→hatch→evolve→graduate, shop/candy/ditto |
| UsageStore.swift | companion/usage_store.rs | aggregation + burn tier + alert/forecast helpers |
| — | integration/app.rs, tray.rs | new: Tauri command bridge + tray |

**Providers working now:** Claude Code, Codex, Gemini, Grok.
**Not yet ported:** Antigravity (protobuf/SQLite), OpenCode, Hermes Agent, Cursor, Copilot, Kiro
(SQLite), plus official OAuth limits, status polling, keychain.

## Key decisions & notes for the next person

- **Synchronous core.** No tokio/async in `domain/platform/providers/companion`. Network is
  blocking `ureq`; file I/O is `std`. The only async is at the Tauri command layer
  (`spawn_blocking` in `integration/app.rs`).
- **Lenient/defensive decoding is load-bearing.** `domain/decoding.rs` + the manual `Deserialize`
  impls reproduce the original's crash/poison-data protections (missing/null/type-mismatch →
  default; corrupt dex item → dropped; required field → fall back to egg). Do not "simplify" this.
- **Codex fork/replay resolution** in `providers/reader.rs` is the most intricate logic
  (`expand_codex_parent_closure`, `resolve_codex_rollouts`) — verified against `tests/fixtures/`.
- **Deliberately deferred / stubbed (see docs/roadmap.md):**
  - `notify_companion_event` → no-op (notifications = Phase 3/4).
  - Sprite prefetch in `ensure_egg_prefetch` → omitted (UI phase).
  - Localization (`L`) → placeholder English strings, `// TODO: localize`.
  - `UsageStore`: official limits, five-hour forecast, provider-status, 429 backoff, timer/sleep —
    Phase 2. The trait returns `Option<DailyUsage>` (no error channel) for now; revisit when adding
    network/OAuth providers.
  - `menu_limit_line` returns `None` until official limits land.
- **State files** (auto-created under `~/.local/share/poketokenbar/`): `companion-state.json`,
  `usage-cache.json`, `base-index.json`, `logs/PokeTokenBar.log`. `PTB_STATE_DIR` env var isolates
  state for testing.
- **Window config** is `visible: true` (dev convenience). For the final tray-app behavior revert to
  `visible: false` + tray toggle once the GNOME appindicator situation is sorted.
- **Pokémon assets are never bundled** — names/sprites come from PokéAPI at runtime (IP-safe, same
  stance as the original).

## Commit history (11 commits)

```
d61a542 docs: update README status for MVP
769941f feat: wire tray + popover UI and Tauri command bridge
b6c88f1 feat: port usage aggregation layer (UsageStore)
76e4357 feat: port companion state machine (hatch/evolve/graduate, shop, candy, ditto)
f72ed9d feat: port PokéAPI client (species/evolution fetch + index cache)
c0333a6 feat: port usage cache and local providers (Claude/Codex/Gemini/Grok)
64e9d97 feat: port usage log reader (Claude/Codex/Gemini/Grok) from Swift
fda275a feat: port domain model and platform layer from Swift
ea85f57 feat: scaffold Tauri 2 + SvelteKit port with architecture skeleton and CI
```

## Immediate next steps

1. **Run on a real desktop** (`npm run tauri dev`) and confirm tray + popover + real usage readout.
2. Fix any window/tray/Wayland quirks that surface.
3. Then Phase 2 per `docs/roadmap.md`: the remaining 6 providers, official limits, notifications.

The original Swift source used as reference is at `/home/fachpersonal/PokeTokenBar-original`.
