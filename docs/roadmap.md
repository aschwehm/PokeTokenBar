# Roadmap

The port ships **Linux first**, then **Windows**. Each phase ends in a runnable, testable milestone.
The guiding principle: **MVP first, architected for full parity** — get the core loop (track →
hatch → evolve → graduate) working end-to-end, then widen.

## Phase 0 — Scaffold & CI (setup)

**Goal:** a compiling Tauri 2 skeleton with the project structure, toolchain, and test harness in
place before any feature work.

- [ ] Scaffold Tauri 2 + Vite + TypeScript in this repo (`src/` frontend, `src-tauri/` backend).
- [ ] Wire the module layout from [architecture.md](architecture.md) §3 (`domain/`, `providers/`,
      `companion/`, `integration/`, `platform/`).
- [ ] Configure `tauri.conf.json` (identifier `dev.poketokenbar.app`, tray, single window).
- [ ] Set up CI (GitHub Actions): `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`,
      frontend `tsc` + `vite build`, on Linux + Windows runners.
- [ ] Decide frontend framework (Svelte vs React) and lock it in.

**Exit criteria:** `cargo test` and `npm run build` green in CI on Linux and Windows.

## Phase 1 — MVP: the core loop (Linux)

**Goal:** a Linux tray icon that opens a popover showing today's token usage from **Claude Code,
Codex, and Gemini CLI**, feeding an egg→hatch→evolve→graduate companion with a basic Pokédex.

- [ ] `platform/` — XDG paths, shell env lookup (`shellEnvironmentValues`), process spawn.
- [ ] `domain/` — models, pricing, token formatting, companion state, save envelope + sanitization.
- [ ] `providers/` — Claude, Codex, Gemini readers + incremental cache (port + tests incl. fixtures).
- [ ] `companion/usage_store` — today/week/month aggregation, burn tier.
- [ ] `companion/` state machine — egg → hatch (PokéAPI, weighted by capture rate) → evolve →
      graduate; natures, shiny roll; Pokédex + catch log persistence.
- [ ] `providers/pokeapi.rs` + sprite fetching (runtime fetch, cached).
- [ ] Tray icon (static) → popover window; home view (companion card + today's tokens).
- [ ] Port the relevant Swift tests to Rust as the spec.

**Exit criteria:** on Linux, burn tokens in Claude/Codex/Gemini → see today's total in the popover →
watch a Pokémon hatch, evolve, graduate, and land in the Pokédex. Persisted across restarts.

## Phase 2 — Full tracking parity

**Goal:** all ten providers + official limits + cost + burn-rate forecast.

- [ ] Remaining providers: Antigravity (protobuf), OpenCode, Hermes, Cursor, Copilot, Kiro, Grok.
- [ ] Official 5-hour/weekly limits for Claude & Codex (OAuth via `~/.claude/.credentials.json`
      first; Keychain/secret-store later), reset countdowns, burn-rate forecast.
- [ ] Cost estimates (`ModelPricing`) surfaced in the UI.
- [ ] Per-service tabs when ≥2 providers are detected; combined totals.
- [ ] Provider incident banner (statuspage).

**Exit criteria:** all ten providers aggregate correctly (fixture-driven tests); limits/cost/forecast
render; per-service tabs work.

## Phase 3 — Companion features & the floating pet

**Goal:** the full "raise & collect" surface and the desktop companion.

- [ ] Shop (Mint, Rare Candy, Pokémon/Uncommon/Rare Egg, Shiny Charm) + Bag.
- [ ] Rare-Candy-on-limit rewards; shiny banner/notification.
- [ ] Floating pet: transparent, always-on-top, frameless, draggable window; hover callout;
      right-click menu; size 48–192px; sprite animation (GIF) in the web layer.
- [ ] Notifications (hatch, evolve, shiny, limit alerts) via `tauri-plugin-notification`.
- [ ] Full settings surface (menu-bar items → tray options, refresh interval, launch-at-login,
      limit thresholds, event notifications, KO/EN/JA).

**Exit criteria:** full feature parity with the original's UI on Linux.

## Phase 4 — OS integration & hardening

- [ ] Secret store (Secret Service via `keyring`) for Claude OAuth; still read-only-on-refresh.
- [ ] Launch-at-login (XDG autostart), single-instance, in-app update check, crash reporting.
- [ ] Save import/export (envelope `SaveTransfer` + file dialogs).
- [ ] Packaging: AppImage + `.deb`/`.rpm`/AUR, icons, release pipeline.

**Exit criteria:** installable, self-updating, autostarting Linux app with secret storage and a
release artifact.

## Phase 5 — Windows port

**Goal:** run the same codebase on Windows.

- [ ] `platform/` Windows paths (`%APPDATA%`/`%LOCALAPPDATA%`), shell env (`cmd`), process spawn.
- [ ] Cursor/Kiro/Claude-Desktop Windows data paths.
- [ ] Tray (`Shell_NotifyIcon`), notifications (toast), autostart (`HKCU\...\Run`), single-instance
      (mutex), secret store (Credential Manager).
- [ ] Packaging: MSI/NSIS installer, code signing.
- [ ] CI Windows runner green.

**Exit criteria:** feature parity with the Linux build on Windows.

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
