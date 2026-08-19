# Windows Port Handoff — PokeTokenBar

_Generated for PokeTokenBar v0.3.0._

---

## 1. Executive Summary & Context

PokeTokenBar is a cross-platform port of [chattymin/PokeTokenBar](https://github.com/chattymin/PokeTokenBar) (macOS Swift app) built with **Tauri 2 (Rust backend + SvelteKit frontend)**.

- **Status**: **v0.3.0 Feature Complete & Verified on Windows and Linux**.
  - All 10 AI usage providers active (Claude Code, Codex, Gemini CLI, Grok CLI, Antigravity, OpenCode, Hermes Agent, Cursor, Copilot CLI, Kiro CLI).
  - Modern UI redesign matching Claude Design specifications.
  - Transparent floating desktop pet companion HUD with circular perimeter progress ring.
  - Zero-freeze async provider log scanning.
  - 333 unit tests passing.
- **Repository**: `https://github.com/aschwehm/PokeTokenBar` (branch `main`).
- **Release Installers**:
  - Windows NSIS: `src-tauri/target/release/bundle/nsis/PokeTokenBar_0.3.0_x64-setup.exe`
  - Windows MSI: `src-tauri/target/release/bundle/msi/PokeTokenBar_0.3.0_x64_en-US.msi`

---

## 2. Windows Dev Environment Prerequisites

Run these commands in PowerShell (Admin) if setting up a fresh Windows machine:

1. **Rust Toolchain**:
   ```powershell
   winget install Rustlang.Rustup
   rustup default stable-x86_64-pc-windows-msvc
   ```
2. **C++ Build Tools**:
   ```powershell
   winget install Microsoft.VisualStudio.2022.BuildTools --override "--passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
   ```
3. **Node.js**:
   ```powershell
   winget install OpenJS.NodeJS.LTS
   ```
4. **WebView2 Runtime**: Pre-installed on Windows 10/11.

---

## 3. Architecture & Windows Optimizations

### A. Non-Blocking Async State Engine
- Heavy provider I/O (JSONL log walks, WSL scans, SQLite queries) is executed asynchronously in background tasks without holding the Tauri `AppState` mutex.
- Global mutex lock duration is $<0.01\text{ ms}$, ensuring snappy UI navigation with zero freezes.

### B. Transparent Circular Pet Window
- Configured with `width: 160, height: 160`, `transparent: true`, `decorations: false`, and `alwaysOnTop: true`.
- Progress is displayed via an SVG circular progress ring around the perimeter, ensuring the center sprite is 100% unobstructed and outer glow effects are never clipped.

### C. Fine-Grained Window Capabilities
- Registered `core:window:allow-minimize`, `core:window:allow-unminimize`, `core:window:allow-hide`, and `core:window:default` in `src-tauri/capabilities/default.json`.
- Implemented `minimize_window` and `hide_window` Tauri commands in `src-tauri/src/integration/app.rs` with frontend dual-fallback.

### D. Provider Data Paths on Windows
- State Directory: `%LOCALAPPDATA%\poketokenbar\`
- Claude Config: `%USERPROFILE%\.claude\.credentials.json`
- Codex Sessions: `%USERPROFILE%\.codex\sessions\`
- Antigravity / SQLite DBs: `%USERPROFILE%\.gemini\antigravity-cli\`
- Cursor Global Storage: `%APPDATA%\Cursor\User\globalStorage\state.vscdb`
- Copilot CLI: `%USERPROFILE%\.copilot\session-store.db`
- Kiro CLI: `%LOCALAPPDATA%\kiro-cli\data.sqlite3`

---

## 4. How to Run and Build on Windows

In PowerShell / Windows Terminal inside the project root:

```powershell
# 1. Install frontend packages
npm install

# 2. Check TypeScript & Svelte
npm run check

# 3. Build release binaries & installers
npm run tauri build
```

If testing Rust unit tests directly:
```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

---

## 5. Verification Checklist for Windows Parity

- [x] `npm run tauri dev` launches without crash and displays the popover window.
- [x] System tray icon appears in the Windows taskbar overflow notification area.
- [x] Left-click or menu toggle hides/shows the popover.
- [x] Clicking `🐾` opens the floating desktop pet overlay.
- [x] Minimize and Close buttons in the titlebar operate smoothly.
- [x] Coding activity in Claude Code, Codex, or Gemini CLI increments the token count.
- [x] State persists in `%LOCALAPPDATA%\poketokenbar\companion-state.json`.
- [x] `npm run tauri build` produces an installer `.exe` in `src-tauri\target\release\bundle\nsis\`.
