# Architecture

This document describes the design of the **PokeTokenBar Linux/Windows port** and how it maps onto
the original macOS Swift codebase. It is the source of truth for implementation decisions; update it
when those decisions change.

---

## 1. Goals

1. Recreate the original's core experience — token tracking + Pokémon companion — on **Linux**
   first, then **Windows**, with the same data fidelity and game logic.
2. Keep the **logic platform-agnostic** and the **UI + OS integration behind a thin abstraction**,
   so Linux/Windows (and later macOS, if desired) share one codebase.
3. Reuse the original's test suite as the **executable specification** for the ported logic.

## 2. Technology decisions

| Concern | Choice | Rationale |
|---|---|---|
| UI shell | **Tauri 2** (web frontend) | Web tech is ideal for animated sprites + rich popover; Tauri is lightweight (vs Electron) and cross-platform. |
| Core logic | **Rust** | Small/fast binary for a 24/7 tray app; strong fit for parsing (JSONL/SQLite/protobuf) and a state machine; excellent test story. |
| Frontend | **TypeScript + Vite** (framework TBD: Svelte or React) | Standard Tauri stack; `<canvas>`/CSS for sprite animation and the floating pet. |
| IPC | Tauri commands (`invoke`) | Typed Rust↔JS boundary; events for push (usage updates, notifications). |
| Storage | `~/.config/poketokenbar` + `~/.local/share/poketokenbar` (XDG) on Linux; `%APPDATA%`/`%LOCALAPPDATA%` on Windows | The original uses `~/Library/Application Support/…`; see §6. |
| SQLite | `rusqlite` (bundled) | Mirrors the original's `sqlite3` usage for OpenCode/Hermes/Cursor/Copilot/Kiro/Antigravity. |
| Protobuf | `prost` | For the Antigravity "Cascade" protobuf blob. |
| HTTP | `reqwest` (or `ureq`) | Ports `URLSession` calls to PokéAPI, sprites, OAuth, status pages, GitHub. |

## 3. High-level architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Web frontend (Vite/TS)                 │
│  Popover UI · Pokédex · Catch log · Bag · Shop · Settings   │
│  Sprite rendering · Floating-pet window content · Tray menu │
└──────────────────────────┬──────────────────────────────────┘
                           │  invoke() / events (IPC)
┌──────────────────────────▼──────────────────────────────────┐
│                    Rust core (src-tauri)                     │
│                                                             │
│  ┌──────────────┐  ┌───────────────┐  ┌─────────────────┐  │
│  │  providers/  │  │   companion/  │  │   integration/  │  │
│  │ usage readers│  │ state machine │  │ tray·notify·key │  │
│  │ (Claude/Codex│  │ egg→evolve→   │  │ chain·autostart │  │
│  │  /Gemini/…)  │  │ graduate·dex  │  │ single-instance │  │
│  └──────┬───────┘  └──────┬────────┘  └────────┬────────┘  │
│         └────────┬────────┘                    │            │
│                  │   domain/ (models, pricing, │            │
│                  │   token format, save/cache) │            │
│                  └──────────────┬──────────────┘            │
│                                 │                            │
│               platform/ (paths, shell, process)              │
└─────────────────────────────────────────────────────────────┘
```

- **`domain/`** — pure data types and calculations (equivalent to the Swift `Core/Models.swift`,
  `ModelPricing.swift`, `TokenFormatter.swift`, `CompanionModel.swift`, `SaveTransfer.swift`).
- **`providers/`** — one module per AI CLI; each implements a common `UsageProvider` trait and
  registers in a central `providers![]` list (mirrors Swift `UsageProvider` + `UsageStore.init`).
- **`companion/`** — the egg→hatch→evolve→graduate state machine, shiny/nature rolls, Pokédex/catch
  log, shop/bag economy (mirrors `CompanionStore.swift`).
- **`integration/`** — thin, per-platform wrappers over Tauri plugins (tray, notification, autostart,
  single-instance, updater) and the secret store.
- **`platform/`** — path resolution (XDG vs `%APPDATA%` vs `~/Library`), shell lookup for
  environment variables, and process spawning (mirrors `BinaryLocator.swift` / `UsageEnvironment.swift`).

## 4. Swift → Rust module mapping

Legend: ✅ portable as-is · 🔶 portable with small tweaks · 🔴 full rewrite (UI/OS integration).

| Swift file (original) | Port target | Notes |
|---|---|---|
| `Core/UsageProvider.swift` | `providers/mod.rs` trait | ✅ protocol → trait; `ProviderEnrichment` → struct |
| `Core/Models.swift` | `domain/models.rs` | ✅ `Codable` → `serde`; `ISO8601Parser` → `chrono`/`time` |
| `Core/ModelPricing.swift` | `domain/pricing.rs` | ✅ static table + family fallback |
| `Core/TokenFormatter.swift` | `domain/format.rs` | ✅ compact/grouped/cost/percent |
| `Core/CompanionModel.swift` | `domain/companion.rs` | ✅ state kinds, language enum |
| `Core/Localization.swift` | `frontend i18n` (KO/EN/JA/ES) | 🔶 string table moves to the web layer; provider label helpers to `providers/` |
| `Core/SaveTransfer.swift` | `domain/save.rs` | ✅ envelope + `sanitized()`; dialog UI moves to frontend |
| `Core/PokeAPIClient.swift` | `providers/pokeapi.rs` | 🔶 actor → `reqwest`; only cache path differs (§6) |
| `Core/ProcessRunner.swift` | `platform/process.rs` | 🔶 `Process` → `std::process`/`tokio` |
| `Core/ProviderStatusChecker.swift` | `providers/status.rs` | ✅ statuspage parsing |
| `Core/SupportMail.swift` | `integration/mailto.rs` | ✅ `mailto:` assembly; opener → Tauri `opener` plugin |
| `Core/LocalUsageProvider.swift` | `providers/claude|gemini|antigravity|grok|codex.rs` | ✅ delegates to reader/cache |
| `Core/LocalUsageReader.swift` | `providers/reader.rs` | 🔶 **core value**; parsing portable; only Claude-Desktop path is macOS-only (§6) |
| `Core/LocalUsageCache.swift` | `providers/cache.rs` | 🔶 cache path → XDG; zlib via `flate2` |
| `Core/LocalAdditionalUsageProvider.swift` | `providers/opencode|hermes|cursor|copilot|kiro.rs` | 🔶 SQLite via `rusqlite`; Cursor/Kiro paths differ per OS (§6) |
| `Core/LocalAntigravityUsageReader.swift` | `providers/antigravity.rs` | 🔶 protobuf blob via `prost` |
| `Core/CodexRateLimitsProvider.swift` | `providers/codex_limits.rs` | 🔶 drops `/Applications/Codex.app` path |
| `Core/UsageStore.swift` | `companion/usage_store.rs` | 🔶 aggregation/forecast pure; remove `NSWorkspace`/App-Nap/`UNUserNotificationCenter` |
| `Core/UsageEnvironment.swift` | `platform/env.rs` | 🔶 delegates to shell lookup |
| `Core/BinaryLocator.swift` | `platform/binary_locator.rs` | 🔶 `/bin/zsh`→`/bin/bash`; drop Homebrew paths |
| `Core/AppEnv.swift` | `platform/app_env.rs` | 🔶 `.app` bundle check → "packaged build?" check |
| `Core/AppLog.swift` | `platform/log.rs` | 🔶 `~/Library/Logs` → XDG (§6) |
| `Core/KeychainAccess.swift` | `integration/secrets.rs` | 🔴 macOS Keychain → OS secret store (Secret Service / Credential Manager) |
| `Core/OAuthLimitsProvider.swift` | `providers/oauth_limits.rs` | 🔶 keep `~/.claude/.credentials.json` fallback (portable); Keychain branch → secrets module |
| `Core/LoginItem.swift` | Tauri `autostart` plugin | 🔴 `SMAppService` → XDG autostart / `HKCU\...\Run` |
| `Core/SingleInstance.swift` | Tauri `single-instance` plugin | 🔴 `NSRunningApplication`/`sysctl` → DBus/`/proc` / named mutex |
| `Core/UpdateChecker.swift` | Tauri `updater` plugin | 🔴 apply path (`NSWorkspace`) → per-OS updater |
| `Core/CrashReporter.swift` | `integration/crash.rs` (`#![cfg]`) | 🔴 `NSException` → `panic` hook + backtrace |
| `PokeTokenBarApp.swift` | `src-tauri/src/main.rs` + `lib.rs` | 🔴 full rewrite: `NSStatusBar` → tray plugin, `NSPopover` → positioned window |
| `UI/PopoverView.swift` et al. | `src/` web views | 🔴 SwiftUI → HTML/CSS/TS |
| `UI/FloatingPetPanel.swift` | transparent always-on-top Tauri window | 🔴 `NSPanel`/`NSEvent` → Tauri window config + mouse events |
| `UI/SpriteLoader.swift` | `frontend sprite.ts` | 🔴 `NSImage` → browser `Image`/`createImageBitmap` |
| `UI/SpriteAnimation.swift` | `frontend gif.ts` | 🔴 `CGImageSource`/ImageIO → browser GIF decoding / `gifuct-js` |
| `Tests/PokeTokenBarTests/*` | `src-tauri/tests/*` + `#[cfg(test)]` | 🔶 port to Rust as the spec (see §7) |

## 5. Platform abstraction layer

The macOS-only OS integrations each have a Tauri-native counterpart:

| macOS API | Linux | Windows | Tauri plugin / crate |
|---|---|---|---|
| `NSStatusBar`/`NSStatusItem` | AppIndicator / StatusNotifierItem | `Shell_NotifyIcon` | `tauri` tray + `tray-icon` |
| `NSPopover` | GTK popover / positioned frameless window | WinUI flyout / positioned window | Tauri window + `tauri-plugin-positioner` |
| `NSPanel` (floating pet) | transparent always-on-top GTK window | layered window | Tauri window config (`transparent`, `alwaysOnTop`, `decorations:false`, `skipTaskbar`) |
| `NSImage` / GIF decode | browser canvas / `<img>` | browser canvas / `<img>` | web platform (native to the frontend) |
| `UNUserNotificationCenter` | libnotify / D-Bus `org.freedesktop.Notifications` | toast notifications | `tauri-plugin-notification` |
| Keychain (`Security`) | Secret Service (org.freedesktop.secrets) | Credential Manager | `keyring` crate |
| `SMAppService` (launch at login) | XDG autostart `.desktop` | `HKCU\...\Run` | `tauri-plugin-autostart` |
| `NSRunningApplication` + `sysctl` (single instance) | DBus name / `/proc` scan | named mutex | `tauri-plugin-single-instance` |
| `NSWorkspace.open` | `xdg-open` | `ShellExecute` | `tauri-plugin-opener` |
| `ProcessInfo.beginActivity` (App Nap) | *dropped* | *dropped* | n/a (no equivalent) |

## 6. Filesystem paths

The app's **own state** moves off `~/Library/…` onto cross-platform locations:

| Purpose | macOS (original) | Linux (XDG) | Windows |
|---|---|---|---|
| Companion state, sprite cache, `base-index.json`, `usage-cache.json` | `~/Library/Application Support/PokeTokenBar/` | `~/.local/share/poketokenbar/` (state + cache) | `%LOCALAPPDATA%\poketokenbar\` |
| Logs / running marker / crash log | `~/Library/Logs/…` | `~/.local/share/poketokenbar/logs/` | `%LOCALAPPDATA%\poketokenbar\logs\` |
| Config (settings) | `UserDefaults` | `~/.config/poketokenbar/` | `%APPDATA%\poketokenbar\` |

Provider data paths are **mostly already cross-platform** and read as-is:

| Provider path | Portability |
|---|---|
| `~/.claude/projects/**/*.jsonl`, `~/.config/claude/projects/**` | ✅ XDG-compatible |
| `~/.claude/.credentials.json` | ✅ (portable OAuth fallback) |
| `~/.codex/sessions/**/rollout-*.jsonl` | ✅ |
| `~/.gemini/tmp/**/chats/*.json(l)` | ✅ |
| `~/.gemini/antigravity-cli/conversations/*.db` | ✅ |
| `~/.local/share/opencode/opencode.db` | ✅ XDG |
| `~/.hermes/state.db` | ✅ |
| `~/.grok/sessions/**/updates.jsonl` (honors `$GROK_HOME`) | ✅ |
| `~/.copilot/session-store.db` (honors `$COPILOT_HOME`) | ✅ |
| `~/Library/Application Support/Claude/…` (Desktop embedded) | 🔶 macOS-only; add Windows `%APPDATA%\Claude` equivalent |
| `~/Library/Application Support/Cursor/…/state.vscdb` | 🔶 → `%APPDATA%\Cursor\…` on Windows |
| `~/Library/Application Support/kiro-cli/data.sqlite3` | 🔶 → `%APPDATA%\kiro-cli\…` on Windows |

## 7. Testing strategy

The original's Swift test suite is our spec. For each logic module we port, we also port its tests:

- **Unit tests** live in `src-tauri/src/**` as `#[cfg(test)]` mods and in `src-tauri/tests/` for
  integration-style suites (parser fixtures, aggregation, companion state, shop economy).
- **Parser fixtures** (e.g. `CodexFork/`, `CodexSubagent/` JSONL samples) are carried over verbatim —
  they encode real-world log formats that naive re-implementation gets wrong (the original's own
  defect log documents several such traps: fork dedup, null-vs-absent JSON, cache-subtraction,
  subagent folding).
- **Cross-cutting rules from the original `docs/reference/defect-log.md` and `CLAUDE.md` are
  binding** and will be re-encoded as Rust clippy lints / tests where possible (e.g. "no provider
  reads a usage-location env var directly — route through `UsageEnvironment`").

## 8. Security & privacy (parity with original)

- **On-device** usage reading; never uploads usage or runs model turns.
- Outbound requests to the same hosts: `pokeapi.co`, `graphql.pokeapi.co`,
  `raw.githubusercontent.com`, `api.anthropic.com`, `status.claude.com`, `status.openai.com`,
  `api.github.com`. None carry usage, tokens, prompts, or project paths.
- Secret store read **only on explicit refresh**, never during auto-poll.
- No Pokémon assets committed or bundled — fetched at runtime and cached under the user's data dir.
