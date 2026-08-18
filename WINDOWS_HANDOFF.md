# Windows Port Handoff — PokeTokenBar

_Generated for continuing development on Windows with Claude._

---

## 1. Executive Summary & Context

PokeTokenBar is a cross-platform port of [chattymin/PokeTokenBar](https://github.com/chattymin/PokeTokenBar) (macOS Swift app) built with **Tauri 2 (Rust backend + SvelteKit frontend)**.

- **Status**: The Linux version is 100% complete, fully working, and passes all 333 unit tests.
- **Repository**: `https://github.com/aschwehm/PokeTokenBar` (branch `main`).
- **Goal for Windows**: Eliminate Windows startup crashes, verify native tray/popover/pet behavior on Windows 10/11, and generate release installers (`.exe` / `.msi`).

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
4. **WebView2 Runtime**: (Pre-installed on Windows 10/11, but verify via Edge).

---

## 3. High-Priority Crash Root Causes & Fixes

When launching with `npm run tauri dev` on Windows, check the terminal output with `RUST_BACKTRACE=1`. Below are the top suspects and their fixes:

### A. Tray Icon Initialization / Fallback Panic
- **Location**: `src-tauri/src/integration/tray.rs`
- **Cause**: On Windows, `app.default_window_icon()` returns `None` if the `.ico` file is missing or in an unexpected format, or Tauri's Win32 tray fails if an icon is required by the OS shell.
- **Fix**: Ensure `src-tauri/icons/icon.ico` exists and `tauri.conf.json` references `"icons/icon.ico"`. In `tray.rs`, icon attachment is wrapped safely (do not use `.expect()`).

### B. Secondary Transparent Pet Window
- **Location**: `src-tauri/tauri.conf.json`
- **Cause**: The `"pet"` window is configured with `"transparent": true`, `"decorations": false`, `"visible": false`. On some Windows graphics drivers or WebView2 versions, creating a transparent window at application startup causes WebView2 initialization to fail.
- **Troubleshooting**: In `tauri.conf.json`, temporarily delete or comment out the `pet` window entry in `"windows"` array to test if the main popover opens cleanly.

### C. Shell Process Spawning (`bash -ilc`)
- **Location**: `src-tauri/src/platform/binary_locator.rs`
- **Cause**: The binary locator attempts to resolve CLI tools via login shells (`bash -ilc`). On Windows systems without Git Bash or WSL on PATH, spawning `bash` fails or blocks.
- **Fix**: Check `src-tauri/src/platform/binary_locator.rs` lines 250–320. Ensure `shell_path()` gracefully returns `None` on Windows instead of attempting to invoke a non-existent shell.

### D. Provider Data Paths on Windows
- **Location**: `src-tauri/src/platform/mod.rs` & `src-tauri/src/providers/`
- **Windows Default Paths**:
  - State Directory: `%LOCALAPPDATA%\poketokenbar\`
  - Claude Config: `%USERPROFILE%\.claude\.credentials.json`
  - Codex Sessions: `%USERPROFILE%\.codex\sessions\`
  - Antigravity / SQLite DBs: `%USERPROFILE%\.gemini\antigravity-cli\`
  - Cursor Global Storage: `%APPDATA%\Cursor\User\globalStorage\state.vscdb`
  - Copilot CLI: `%USERPROFILE%\.copilot\session-store.db`
  - Kiro CLI: `%LOCALAPPDATA%\kiro-cli\data.sqlite3`

### E. File Rename / Atomic Writes
- **Location**: `src-tauri/src/companion/store.rs` (`atomic_write`)
- **Note**: `std::fs::rename` fails on Windows if the target file already exists. We have added `#[cfg(windows)] std::fs::remove_file(path)` before rename. Ensure no other file operations assume Unix overwrite semantics.

---

## 4. How to Run and Debug on Windows

In PowerShell / Windows Terminal inside the project root:

```powershell
# 1. Install frontend packages
npm install

# 2. Verify frontend builds cleanly
npm run check
npm run build

# 3. Run with full Rust backtraces
$env:RUST_BACKTRACE="1"
npm run tauri dev
```

If testing Rust unit tests directly:
```powershell
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
```

---

## 5. Key File Reference

| Purpose | File Path |
|---|---|
| Main Tauri Setup & Handlers | `src-tauri/src/lib.rs` |
| App Commands & Snapshot DTO | `src-tauri/src/integration/app.rs` |
| System Tray Integration | `src-tauri/src/integration/tray.rs` |
| Windows / Platform Paths | `src-tauri/src/platform/mod.rs` |
| Shell Env & Binary Locator | `src-tauri/src/platform/binary_locator.rs` |
| 10 AI Usage Providers | `src-tauri/src/providers/reader.rs`, `additional.rs`, `antigravity.rs` |
| Claude Rate Limits Parser | `src-tauri/src/providers/claude_limits.rs` |
| PokéAPI & Offline Sprites | `src-tauri/src/providers/pokeapi.rs` |
| Companion State Machine | `src-tauri/src/companion/store.rs` |
| Svelte Popover UI | `src/routes/+page.svelte` |
| Svelte Desktop Pet Window | `src/routes/pet/+page.svelte` |
| Tauri Configuration | `src-tauri/tauri.conf.json` |
| Permissions / Capabilities | `src-tauri/capabilities/default.json` |

---

## 6. Verification Checklist for Windows Parity

- [ ] `npm run tauri dev` launches without crash and displays the popover window.
- [ ] System tray icon appears in the Windows taskbar overflow notification area.
- [ ] Left-click or menu toggle hides/shows the popover.
- [ ] Clicking `🐾` opens the floating desktop pet overlay.
- [ ] Coding activity in Claude Code, Codex, or Gemini CLI increments the token count.
- [ ] State persists in `%LOCALAPPDATA%\poketokenbar\companion-state.json`.
- [ ] `npm run tauri build` produces an installer `.exe` in `src-tauri\target\release\bundle\nsis\`.
