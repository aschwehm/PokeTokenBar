# Changelog

All notable changes to **PokeTokenBar** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Planned
- 🎶 Retro Pokémon 8-bit Sound Effects & Cry synth
- 🏆 Gym Leader Boss Battles & Coding Bounties
- 🌲 Customizable Ecosystem Habitats & Weather Effects

---

## [0.3.7] - 2026-08-21

### Fixed
- **🏅 Ribbon Case Modal Dialog**: Added full fixed-position backdrop and card styling so clicking the ribbon bar in the Buddy tab opens the interactive Ribbon Case modal.
- **🪪 Trainer Passport Customization**:
  - Fixed Avatar Picker Modal backdrop and scroll container so clicking the avatar frame opens the full Pokédex avatar selection dialog.
  - Converted nickname display and avatar frame into accessible interactive buttons with hover feedback.
  - Added automatic text focusing and selection on entering nickname edit mode (`Enter` to save, `Escape` to cancel).
- **🖥️ Window Layout & Dimensions**: Increased default window size to 460×660 in `tauri.conf.json` to comfortably fit all 5 navigation tabs (`Buddy`, `Usage`, `Shop & Bag`, `Pokédex`, `Passport`) without cramped icons.
- **🎨 Custom Themed Scrollbars**: Added `.pk-scroll` styling (thin scrollbars with themed tracks and thumbs across both vertical and horizontal directions) to the Coding Activity Heatmap, PokéJournal feed, Ribbon Case grid, and Avatar Picker.

---

## [0.3.6] - 2026-08-21

### Added
- **🪪 Holographic Trainer Passport Card**:
  - Custom Trainer ID (`#TR-XXXX`) and editable nickname.
  - Progressive career rank titles from `Novice Pokémon Trainer` to `AI Grandmaster Champion` scaling with lifetime token burns.
  - **Trainer Avatar Picker Modal**: Choose any Pokémon discovered in your Pokédex as your profile avatar (or leave set to auto-follow your active partner).
  - Stat counters for Lifetime Tokens, Eggs Hatched, and Achievement Ribbons unlocked.
- **🟩 GitHub-Style Coding Activity Heatmap**:
  - 26-week (182-day) retro pixel contribution grid visualizing token consumption across all 10 AI providers.
  - 5 intensity levels with emerald/neon glow styling.
  - Productivity metrics: Current Streak, Best Streak, Active Days, and Daily Average volume.
  - Interactive cell tooltips with formatted date, token volume, and day-of-week context.
- **📜 Trainer PokéJournal (Milestone Diary)**:
  - Real-time chronological memory diary logging companion hatches, evolutions, graduations, berry treats, and ribbon awards.
  - Custom event icons, Pokémon sprite thumbnails, timestamps, and descriptive flavor text.
- **Unit Tests**: Added `test_trainer_passport_and_journal` (337 total passing unit tests).

---

## [0.3.5] - 2026-08-21

### Added
- **🏅 Pokémon Ribbon & Achievement System**:
  - 11 unique unlockable ribbons: `Starter`, `Best Buddy Affection`, `Gourmet Berry Lover`, `Overdrive Surge`, `Midnight Coder`, `Bronze 10M`, `Silver 50M`, `Gold 100M`, `Titan 500M`, `Hall of Fame Graduate`, and `Star Sparkle Shiny`.
  - Interactive Ribbon Bar on Hero card with preview pills.
  - Dedicated **Ribbon Case Modal** showing unlocked badge state, honor titles, and locked milestone goals.
  - Pokédex integration capturing accumulated ribbons per species.
- **🕹️ Desktop Pet 2.0 Polish**:
  - Reduced rotation jitter on click interactions.
  - Gradient smoothing for glowing widget borders.
  - Clean evolution stage indicators (e.g. Stage 2/3 for Piloswine).

---

## [0.3.4] - 2026-08-21

### Added
- **⚡ Mega Evolution & Gigantamax Overdrive**:
  - Temporary Mega or Gigantamax form transformation when token burn rate enters `Fast` or `Blazing` tiers.
  - **2× Coin Rush**: Double Token Coins earned in PokéShop while Overdrive mode is active.
  - Prismatic HUD aura effects with particle bursts.
  - Optional toggle in Settings (default off).

---

## [0.3.3] - 2026-08-21

### Added
- **🕹️ Interactive Desktop Pet Widget**:
  - Floating, always-on-top transparent desktop companion with draggable HUD.
  - Perimeter circular SVG ring tracking active growth / hatch progress.
  - Petting interaction animations: Happy Hop, Playful Wiggle, and Floating Hearts.
  - 15-minute Sleep & Wake cycle with floating `Zzz` bubbles.

---

## [0.3.2] - 2026-08-21

### Added
- **🎒 PokéShop & Bag Economy**:
  - Buy and use Rare Candies (+10M XP), Nature Mints, Oran Berries (+15M XP), and Sitrus Berries (+50M XP + 1-hour Golden Sparkle Aura).
  - Guaranteed Egg Tier purchases (Rare, Epic, Legendary).
  - Permanent Shiny Charm item boosting shiny hatch odds from 1/64 to 1/48.
- **📖 Pokédex Detail Modal**:
  - Game lore, classification, height, weight, and element types for discovered Pokémon.

---

## [0.3.1] - 2026-08-20

### Added
- **10 AI Provider Parsers**: Support for Claude Code, Codex, Gemini CLI / AGY, Antigravity, Grok CLI, OpenCode, Hermes Agent, Cursor, Copilot CLI, and Kiro CLI.
- **Multi-Theme Support**: Midnight Glass, OLED Dark, Neon Cyber, and Retro Game Boy color palettes.

---

## [0.1.0] - 2026-08-19

### Added
- Initial cross-platform port of chattymin/PokeTokenBar to Tauri 2 (Rust + Svelte 5).
- Background file parsing, incubation engine, and token evolution thresholds.
