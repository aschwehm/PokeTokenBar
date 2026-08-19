# Roadmap

The port ships **Linux first**, then **Windows**. Each phase ends in a runnable, testable milestone.
The guiding principle: **MVP first, architected for full parity** — get the core loop (track →
hatch → evolve → graduate) working end-to-end, then widen.

## Phase 0 — Scaffold & CI (setup) [DONE]

- [x] Scaffold Tauri 2 + Vite + TypeScript in this repo (`src/` frontend, `src-tauri/` backend).
- [x] Wire the module layout from [architecture.md](architecture.md) §3 (`domain/`, `providers/`,
      `companion/`, `integration/`, `platform/`).
- [x] Configure `tauri.conf.json` (identifier `dev.poketokenbar.app`, tray, single window).
- [x] Set up CI (GitHub Actions): `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`,
      frontend `tsc` + `vite build`, on Linux + Windows runners.
- [x] Lock in SvelteKit frontend framework.

**Exit criteria:** `cargo test` and `npm run build` green in CI on Linux and Windows.

## Phase 1 — MVP: the core loop (Linux) [DONE]

- [x] `platform/` — XDG paths, shell env lookup (`shellEnvironmentValues`), process spawn.
- [x] `domain/` — models, pricing, token formatting, companion state, save envelope + sanitization.
- [x] `providers/` — Claude, Codex, Gemini readers + incremental cache (port + tests incl. fixtures).
- [x] `companion/usage_store` — today/week/month aggregation, burn tier.
- [x] `companion/` state machine — egg → hatch (PokéAPI, weighted by capture rate) → evolve →
      graduate; natures, shiny roll; Pokédex + catch log persistence.
- [x] `providers/pokeapi.rs` + sprite fetching (runtime fetch, disk cache, base64 data URLs).
- [x] Tray icon (static) → popover window; home view (companion card + today's tokens).
- [x] Port the relevant Swift tests to Rust as the spec.

## Phase 2 — Full tracking parity [DONE]

- [x] Remaining providers: Antigravity (protobuf), OpenCode, Hermes, Cursor, Copilot, Kiro, Grok (all 10 active).
- [x] Official 5-hour/weekly limits for Claude (OAuth via `~/.claude/.credentials.json`).
- [x] Cost estimates (`ModelPricing`) surfaced in backend + snapshots.
- [x] Combined & per-provider breakdown in Sources UI list.

## Phase 3 — Companion features & the floating pet [DONE]

- [x] Shop (Mint, Rare Candy, Pokémon/Uncommon/Rare Egg, Shiny Charm) + Bag.
- [x] Rare-Candy-on-limit rewards; shiny banner/notification.
- [x] Floating pet: transparent, always-on-top, frameless, draggable window (`🐾` toggle);
      progress ring, burn flame animations, offline sprite caching.
- [x] Notifications (hatch, evolve, shiny reveals) via `tauri-plugin-notification`.
- [x] Settings surface (tray actions, autostart launch-at-login, language switch).

## Phase 4 — OS integration & hardening [DONE]

- [x] Launch-at-login (`tauri-plugin-autostart` for XDG autostart on Linux / registry on Windows).
- [x] Offline sprite disk caching in `~/.local/share/poketokenbar/sprites`.
- [x] Packaging: Linux AppImage + `.deb` build targets generated and verified.

## Phase 5 — Windows port [DONE / CI-WIRED]

- [x] `platform/` Windows paths (`%LOCALAPPDATA%`, `USERPROFILE`), platform-agnostic path joiners.
- [x] Cross-platform tray, notifications, autostart, window draggable regions.
- [x] Packaging: Windows NSIS / MSI bundle targets configured in `tauri.conf.json` and `.github/workflows/ci.yml`.

## Phase 6 — Claude Design UI Overhaul & Performance Polish (v0.3.0) [DONE]

- [x] Complete frontend overhaul matching the Claude Design specifications.
- [x] Space Grotesk & JetBrains Mono typography with custom dark aesthetic palettes.
- [x] Hero buddy card with stage pips, elemental type styling, and dual-tone gradient progress bars.
- [x] Active AI Tools horizontal distribution bar with proportional per-provider breakdowns.
- [x] PokéShop purchase shake animations (`pk-shake`) on insufficient token balance.
- [x] Pokédex 2-column grid with animated Showdown GIFs and mystery discovery progression slots.
- [x] Redesigned desktop pet with an SVG circular perimeter progress ring and unclipped glow.
- [x] Decoupled heavy provider I/O scanning from global state locks (<0.01ms state lock, zero UI freezes).
- [x] Added window minimize & hide backend commands and header pet toggle shortcut.

---

## Cross-cutting principles (from the original's own rules)

These are binding, carried over from the original's `CLAUDE.md` / `defect-log.md`:

- **No literal `providerID == "..."` branches on generic paths** — provider-specific behavior is
  isolated to that provider's module.
- **A single source for path/env resolution** — no provider reads a usage-location env var directly.
- **External log formats are validated against the upstream *writer*, not self-made fixtures.**
- **Dedup keys use the turn's own globally-unique id; timestamps use the turn time, not the
  re-write time; subagent sessions are folded into parents.**
- **Defensive parsing at trust boundaries** — external numeric values are clamped before arithmetic
  (the original hit SIGTRAP on hostile log values; we must not repeat it).
