# PokeTokenBar (Linux / Windows port)

**Your AI coding tokens, hatched into Pokémon — now cross-platform.**

This project is a from-scratch cross-platform port of the original macOS menu-bar app [**chattymin/PokeTokenBar**](https://github.com/chattymin/PokeTokenBar) created by [**@chattymin**](https://github.com/chattymin). Re-engineered with **Tauri 2 (Rust backend + Svelte 5 frontend)**, it brings the AI-coding Pokémon companion experience to **Windows** and **Linux**.

It monitors the AI coding tokens you burn across 10 tools (Claude Code, Codex, Gemini CLI, Antigravity, OpenCode, Hermes Agent, Cursor, Grok CLI, Copilot CLI, Kiro CLI) and turns them into a growing, evolving Pokémon buddy right on your desktop.

> **Status: v0.3.1 — Pokédex Flavor Entries, UI Overhaul & Performance Engine (Linux & Windows).**
> - **10 Supported AI Providers**: Claude Code, Codex, Gemini CLI, Grok CLI, Antigravity, OpenCode, Hermes Agent, Cursor, Copilot CLI, Kiro CLI.
> - **Interactive Pokédex Detail Modal**: Click any discovered Pokémon to read its official game Pokédex flavor text, classification, height, weight, and rarity.
> - **Redesigned Interface**: Space Grotesk & JetBrains Mono typography, dark aesthetic cards, radial type glows, stage pips, and segmented AI usage breakdown.
> - **Desktop Companion Pet Widget**: Floating, always-on-top circular HUD with an SVG perimeter evolution progress ring and real-time pace flame badges.
> - **Zero-Freeze Async Engine**: Provider log scans decoupled from UI state locks (< 0.01 ms mutex duration).
> - **333 passing unit tests**. See [docs/architecture.md](docs/architecture.md) and [docs/roadmap.md](docs/roadmap.md).

> [!IMPORTANT]
> **Provider Testing & Verification Status**:
> Live end-to-end testing has currently only been verified with **Antigravity / `agy` (the new Gemini CLI)** and **Claude Code** (as these are the tools available in our testing environment).
>
> Parsers for all other 8 providers (Codex, Grok CLI, OpenCode, Hermes Agent, Cursor, Copilot CLI, Kiro CLI, legacy Gemini) are implemented and validated against the original Swift reference test suite fixtures (330+ unit tests), but **live testing from the community is actively welcome**! If you encounter any path or format differences, please open an issue on GitHub.

---

## 🌟 Key Features

### 🐾 1. Active Buddy & Evolution System
- **Egg Incubation to Final Mastery**: Burn AI tokens while coding to incubate eggs, hatch new Pokémon partners, and evolve them through multiple evolutionary stages.
- **Stage Pips & Type Badges**: Live stage tracker (`● ● ○`) and dynamic color themes matching Pokémon elemental types (Grass, Fire, Water, Electric, Ice, Ground, Psychic, Dragon, etc.).
- **Quick Action Items**: Use **Nature Mints** to reroll growth attributes or **Rare Candies** for instant progress directly from the Buddy card.
- **Celebration Banners**: Interactive banners with sparkle animations celebrating evolutionary milestones and Hall of Fame graduations.

### 📊 2. Usage Analytics & Claude Rate Limits
- **Aggregated Summaries**: Real-time token consumption metrics for **Today**, **This Week**, and **This Month**, plus estimated cost in USD.
- **Active AI Tools Breakdown**: Proportional horizontal tool share bar and detailed per-provider breakdown lists.
- **Claude OAuth Rate Limit Tracking**: Direct integration with `~/.claude/.credentials.json` to monitor 5-hour session windows and 7-day weekly rate limits.

### 🎒 3. Shop & Bag Economy
- **Wallet Balance**: Tokens earned by coding become available currency to spend in the PokéShop.
- **Bag Inventory**: Store Nature Mints, Rare Candies, Shiny Charms, and Pokémon Eggs.
- **PokéShop**: Purchase Basic, Uncommon+, and Rare+ eggs, Mints, and Charms with interactive purchase animations and balance protection.

### 📖 4. Pokédex & Detailed Entry Dialog
- **Collection Tracker**: Browse all discovered Pokémon with primary type banners, active buddy tags, and animated Showdown GIFs.
- **Interactive Detail Modal**: Click on any discovered Pokémon to view its full Pokédex profile:
  - Official game description / lore flavor text.
  - Genus classification title (e.g. *Pig Pokémon*, *Seed Pokémon*).
  - Physical stats (Height in meters, Weight in kilograms).
  - Category (Standard, Legendary, Mythical).
- **Silhouette Progression**: Mystery locked slots (`?` / `???`) indicate remaining species to discover.

### 🐾 5. Desktop Companion Pet (Floating Widget)
- **Always-on-Top Floating Sphere**: Transparent, frameless circular pet widget draggable anywhere on your desktop.
- **Circular Perimeter Progress Ring**: Non-intrusive SVG ring progress indicator wrapping the circumference, leaving the center 100% unobstructed for your Pokémon.
- **Pace Flame Badge**: Dynamically displays pace indicators (⚡ Fast Pace, 🔥 On Fire) based on your real-time token burn tier.

### 🎨 6. Theme Engine & Customization
- **4 Visual Themes**: *Midnight Glass*, *OLED Dark*, *Neon Cyber*, and *Game Boy Retro*.
- **Configurable Polling**: Adjust background token refresh frequency (10s, 30s, 60s, 120s, 300s).
- **Launch at Login**: Native autostart support on Windows (Registry) and Linux (XDG Autostart).
- **Animated Sprites Toggle**: Choose between animated Pokémon Showdown GIFs or static pixel art sprites.

---

## ⚡ Performance & Architecture

- **Rust Backend**: Fast, lightweight background daemon leveraging Tokio async runtime and Rusqlite.
- **Non-Blocking State Architecture**: Filesystem and WSL log scans run asynchronously in background tasks without holding the global application lock, ensuring the UI and floating pet never stall or freeze.
- **Disk Caching**: 30-day disk cache for the PokéAPI GraphQL species index and local sprite cache in `%LOCALAPPDATA%\poketokenbar\sprites` (Windows) or `~/.local/share/poketokenbar/sprites` (Linux) with zero external network requests during standard coding sessions.

---

## 📦 Building & Installation

### Prerequisites
- [Node.js](https://nodejs.org) (v18+)
- [Rust](https://rustup.rs) (1.78+)
- Platform build dependencies (Visual Studio C++ Build Tools on Windows, `libwebkit2gtk-4.1-dev` on Linux)

### Development
```bash
# Install frontend dependencies
npm install

# Run in development mode (hot reload)
npm run tauri dev
```

### Production Build
```bash
# Run TypeScript / Svelte checks
npm run check

# Build release binaries & installers
npm run tauri build
```
Installers will be generated under `src-tauri/target/release/bundle/`:
- **Windows**: `nsis/PokeTokenBar_0.3.0_x64-setup.exe` and `msi/PokeTokenBar_0.3.0_x64_en-US.msi`
- **Linux**: `deb/poketokenbar_0.3.0_amd64.deb` and `appimage/poketokenbar_0.3.0_amd64.AppImage`

---

## 🙏 Acknowledgements & Credits

- [**chattymin/PokeTokenBar**](https://github.com/chattymin/PokeTokenBar) by [**@chattymin**](https://github.com/chattymin): The original macOS app that inspired this port. Huge appreciation for the original game design, companion mechanics, and the comprehensive Swift reference test suite.
- [**PokéAPI**](https://pokeapi.co): Providing the public Pokémon species index, evolutionary chains, and types data.
- [**Pokémon Showdown**](https://play.pokemonshowdown.com): Animated battle sprites.

---

## ⚡ AI Vibe-Coding Disclaimer

> [!WARNING]
> **100% Vibe-Coded Project**:
> This entire cross-platform port, architecture, Rust backend, Svelte 5 frontend, and UI design was **completely vibe-coded** using:
> - **Gemini 3.7 Flash** (AGY / Antigravity)
> - **DeepSeek V4 Pro**
> - **Claude Design (Sonnet 5)**
>
> While backed by **333 passing unit tests** ported from the original Swift reference test suite with zero errors, **use at your own risk!** Bugs may be encountered, and contributions/PRs are always welcome.

---

## 📜 License & Disclaimer

This project is an **unofficial, non-commercial fan project**. It is not affiliated with, endorsed, sponsored, or approved by Nintendo, Game Freak, Creatures Inc., or The Pokémon Company. "Pokémon" and all related names, characters, and imagery are trademarks and copyrights of their respective owners.

No Pokémon assets are committed or bundled — species data and sprites are fetched at runtime from public APIs and cached locally.

The software is open source under the [MIT License](LICENSE).

