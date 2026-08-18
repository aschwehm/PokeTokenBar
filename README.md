# PokeTokenBar (Linux / Windows port)

**Your AI coding tokens, hatched into Pokémon — now cross-platform.**

This is a from-scratch port of [chattymin/PokeTokenBar](https://github.com/chattymin/PokeTokenBar),
a macOS menu-bar app that turns the AI-coding tokens you burn (Claude Code, Codex, Gemini CLI,
Antigravity, OpenCode, Hermes Agent, Cursor, Grok CLI, Copilot CLI, Kiro CLI) into a growing
Pokémon companion. The original is written in **Swift 6 + SwiftUI + AppKit**; this port re-implements
it with **Tauri 2** (Rust core + web frontend) so it runs on **Linux** first and **Windows** after.

> **Status: Feature-Complete Port (Linux & Windows).** Full feature parity with the original:
> all 10 providers (Claude Code, Codex, Gemini, Grok, Antigravity, OpenCode, Hermes, Cursor, Copilot, Kiro),
> official Claude OAuth rate limits, companion hatching/evolution/graduation, inventory shop,
> floating draggable desktop pet (`🐾`), autostart on login, offline sprite caching, and desktop notifications.
> 333 unit tests pass. See [docs/roadmap.md](docs/roadmap.md) and [handoff.md](handoff.md).

## Why a port (and why Tauri)

The original's value splits roughly 60/40:

- **~60–70% is pure, cross-platform logic** — the token parsers (JSONL / SQLite / protobuf),
  aggregation & burn-rate forecasting, model pricing, the companion state machine, save/transfer,
  localization, and the PokéAPI client. This ports almost directly.
- **~30–40% is macOS-only** — the SwiftUI views, `NSStatusBar`/`NSPopover`, the floating desktop pet
  (`NSPanel`/`NSEvent`), `NSImage`/GIF decoding, Keychain, launch-at-login, single-instance, crash
  reporting, and notifications.

Swift has **no viable menu-bar/tray UI story on Linux or Windows** (SwiftUI does not exist off
macOS), so the UI layer is rewritten regardless. Tauri was chosen so that the *logic* lives in
**Rust** (small, fast, well-tested, ideal for a 24/7 tray app) and the *UI* lives in a **web
frontend** (ideal for animated sprites and a rich popover), with first-class system-tray,
notification, and transparent-window support on all three platforms.

The original's extensive Swift test suite is used as the **executable specification** for the Rust
logic — we re-implement against verified behavior, not guesses.

See [docs/architecture.md](docs/architecture.md) for the full design and the Swift→Rust module
mapping.

## One important platform difference

The original shows an **animated sprite + live token count in the macOS menu bar**. Neither the
Linux tray (`StatusNotifierItem` / AppIndicator) nor the Windows tray (`Shell_NotifyIcon`) supports
arbitrary animation or text — only a static icon + tooltip. On these platforms:

- **System tray** = static icon; clicking it opens the popover window.
- **Floating pet** (desktop widget) = where the animated sprite + live count live. This is already a
  first-class feature in the original and ports cleanly.

## AI Vibe-Coding Disclaimer

> [!WARNING]
> This port was **completely vibe-coded** using **DeepSeek V4 Pro** and **Gemini 3.7 Flash**.
> While backed by 333 passing unit tests ported from the original Swift reference test suite, **use at your own risk**!

## License & disclaimer

This project is an **unofficial, non-commercial fan project**. It is not affiliated with,
endorsed, sponsored, or approved by Nintendo, Game Freak, Creatures Inc., or The Pokémon Company.
"Pokémon" and all related names, characters, and imagery are trademarks and copyrights of their
respective owners.

As with the original, **no Pokémon assets are committed or bundled** — species data and sprites are
fetched at runtime from the public [PokéAPI](https://pokeapi.co) and cached locally on the user's
device. The MIT license covers this project's original source code only; it grants no rights to any
third-party trademarks, artwork, or data.

The port is MIT-licensed. See [LICENSE](LICENSE).
