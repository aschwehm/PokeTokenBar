<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { isEnabled, enable, disable } from "@tauri-apps/plugin-autostart";
  import { onMount } from "svelte";

  interface ShopEntry {
    kind: string;
    tier: string | null;
    price: number;
    canBuy: boolean;
    owned: number;
  }

  interface DexSpecies {
    id: number;
    name: string;
    rarity: string;
    isShiny: boolean;
    isRaising: boolean;
  }

  interface CompanionView {
    displayState: string;
    displayName: string;
    isEgg: boolean;
    hasActive: boolean;
    currentSpeciesId: number | null;
    isShiny: boolean;
    rarity: string | null;
    isFinalStage: boolean;
    stageText: string;
    progress: number;
    tokensToNext: number;
    eggProgress: number;
    eggTokensToHatch: number;
    availableTokens: number;
    ownedItems: [string, number][];
    shop: ShopEntry[];
    dex: DexSpecies[];
    justEvolvedTo: string | null;
    justGraduated: string | null;
  }

  interface ProviderView {
    id: string;
    displayName: string;
    todayTotalTokens: number;
    weekTotalTokens: number;
    monthTotalTokens: number;
  }

  interface LimitWindow {
    utilization?: number | null;
    resetsAt?: string | null;
  }

  interface LimitStatus {
    fiveHour?: LimitWindow | null;
    sevenDay?: LimitWindow | null;
    subscriptionType?: string | null;
    rateLimitTier?: string | null;
    planDisplay?: string | null;
  }

  interface UsageView {
    todayTotalTokens: number;
    todayCostTotal: number;
    weekTotalTokens: number;
    monthTotalTokens: number;
    burnTier: string;
    snapshots: ProviderView[];
    limits?: LimitStatus | null;
  }

  interface Snapshot {
    companion: CompanionView;
    usage: UsageView;
  }

  let snap = $state<Snapshot | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function refresh() {
    loading = true;
    error = null;
    try {
      snap = await invoke<Snapshot>("refresh");
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function buy(kind: string) {
    snap = await invoke<Snapshot>("buy_item", { kind });
  }

  async function useCandy() {
    snap = await invoke<Snapshot>("use_rare_candy");
  }

  async function useMint() {
    snap = await invoke<Snapshot>("use_mint");
  }

  async function buyEgg(tier: string | null) {
    snap = await invoke<Snapshot>("buy_egg", { tier });
  }

  async function hideWindow() {
    try {
      await getCurrentWindow().hide();
    } catch {
      // ignore
    }
  }

  async function togglePet() {
    try {
      await invoke("toggle_pet_window");
    } catch {
      // ignore
    }
  }

  async function startDrag(e: MouseEvent) {
    if (e.button === 0 && !(e.target as HTMLElement).closest("button")) {
      try {
        await getCurrentWindow().startDragging();
      } catch (err) {
        console.error("Failed to start dragging:", err);
      }
    }
  }

  let autostartActive = $state(false);
  let showSettings = $state(false);
  let refreshIntervalSec = $state(60);
  let spriteCache = $state<Record<string, string>>({});

  async function checkAutostart() {
    try {
      autostartActive = await isEnabled();
    } catch {
      // ignore
    }
  }

  async function toggleAutostart() {
    try {
      if (autostartActive) {
        await disable();
        autostartActive = false;
      } else {
        await enable();
        autostartActive = true;
      }
    } catch (e) {
      console.error("Autostart toggle failed:", e);
    }
  }

  function changeRefreshInterval(sec: number) {
    refreshIntervalSec = sec;
    try {
      localStorage.setItem("ptb_refresh_interval", sec.toString());
    } catch {
      // ignore
    }
  }

  async function cacheSprite(id: number, shiny: boolean) {
    const key = `${id}_${shiny}`;
    if (spriteCache[key]) return;
    try {
      const data = await invoke<string | null>("get_sprite", { id, shiny });
      if (data) {
        spriteCache[key] = data;
      }
    } catch {
      // ignore
    }
  }

  onMount(() => {
    try {
      const saved = localStorage.getItem("ptb_refresh_interval");
      if (saved) {
        const sec = parseInt(saved, 10);
        if (!isNaN(sec) && sec >= 5) {
          refreshIntervalSec = sec;
        }
      }
    } catch {
      // ignore
    }

    invoke<Snapshot>("snapshot")
      .then((s) => {
        if (!snap) snap = s;
      })
      .catch(() => {});
    refresh();
    checkAutostart();

    let interval = setInterval(() => {
      refresh();
    }, refreshIntervalSec * 1000);

    const unlisten = listen("tray-refresh", () => refresh());
    return () => {
      clearInterval(interval);
      unlisten.then((f) => f());
    };
  });

  function compact(n: number): string {
    const v = Math.abs(n);
    const sign = n < 0 ? "-" : "";
    if (v < 1_000) return `${n}`;
    if (v < 1_000_000) return sign + trim(v / 1_000, 1) + "K";
    if (v < 1_000_000_000) return sign + trim(v / 1_000_000, 1) + "M";
    return sign + trim(v / 1_000_000_000, 2) + "B";
  }

  function trim(value: number, decimals: number): string {
    let s = value.toFixed(decimals);
    while (s.endsWith("0")) s = s.slice(0, -1);
    if (s.endsWith(".")) s = s.slice(0, -1);
    return s;
  }

  function tokens(n: number): string {
    return n.toLocaleString("en-US");
  }

  function formatUtilization(val: number): string {
    return `${val.toFixed(0)}%`;
  }

  function getUtilizationPercent(val: number): string {
    return Math.min(100, Math.max(0, val)).toFixed(1);
  }

  function spriteUrl(id: number, shiny: boolean): string {
    const key = `${id}_${shiny}`;
    if (spriteCache[key]) return spriteCache[key];
    cacheSprite(id, shiny);
    const dir = shiny ? "shiny/" : "";
    return `https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/${dir}${id}.png`;
  }

  const rarityLabel: Record<string, string> = {
    common: "Common",
    uncommon: "Uncommon",
    rare: "Rare",
    legendary: "Legendary",
  };

  const itemLabel: Record<string, string> = {
    rareCandy: "Rare Candy",
    mint: "Mint",
    shinyCharm: "Shiny Charm",
  };

  const burnLabel: Record<string, string> = {
    idle: "Idle",
    normal: "Working",
    fast: "Focus",
    blazing: "On fire",
  };

  const tierLabel: Record<string, string> = {
    uncommon: "Uncommon+",
    rare: "Rare+",
  };
</script>

<div class="app">
  <header data-tauri-drag-region onmousedown={startDrag} role="toolbar" aria-label="Window header" tabindex="-1">
    <div class="header-left" data-tauri-drag-region>
      <span class="dot" class:ok={!loading}></span>
      <span class="title" data-tauri-drag-region>PokeTokenBar</span>
    </div>
    <div class="header-right">
      <button
        class="ghost"
        class:active-tab={showSettings}
        onclick={() => (showSettings = !showSettings)}
        title={showSettings ? "Show Companion" : "Settings"}
      >
        ⚙
      </button>
      <button class="ghost" onclick={togglePet} title="Toggle Floating Desktop Pet">
        🐾
      </button>
      <button class="ghost" onclick={refresh} disabled={loading} title="Refresh">
        {loading ? "…" : "↻"}
      </button>
      <button class="ghost close-btn" onclick={hideWindow} title="Hide window">
        ✕
      </button>
    </div>
  </header>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#if showSettings}
    <div class="settings-view">
      <div class="settings-header">
        <h3>Settings</h3>
        <button class="ghost back-btn" onclick={() => (showSettings = false)}>← Back</button>
      </div>

      <div class="settings-group">
        <h4>General</h4>
        <div class="settings-row">
          <div class="setting-info">
            <span class="setting-title">Launch at Login</span>
            <span class="setting-desc">Start PokeTokenBar automatically on startup</span>
          </div>
          <input
            type="checkbox"
            checked={autostartActive}
            onchange={toggleAutostart}
            aria-label="Launch at login"
          />
        </div>

        <div class="settings-row">
          <div class="setting-info">
            <span class="setting-title">Refresh Interval</span>
            <span class="setting-desc">Background polling frequency for token logs</span>
          </div>
          <select
            class="custom-select"
            value={refreshIntervalSec}
            onchange={(e) => changeRefreshInterval(parseInt((e.target as HTMLSelectElement).value, 10))}
            aria-label="Refresh interval"
          >
            <option value={15}>15 seconds</option>
            <option value={30}>30 seconds</option>
            <option value={60}>1 minute (default)</option>
            <option value={120}>2 minutes</option>
            <option value={300}>5 minutes</option>
          </select>
        </div>
      </div>

      <div class="settings-group">
        <h4>Desktop Companion</h4>
        <div class="settings-row">
          <div class="setting-info">
            <span class="setting-title">Floating Pet</span>
            <span class="setting-desc">Always-on-top circular Pokémon widget</span>
          </div>
          <button class="ghost" onclick={togglePet}>Toggle Pet</button>
        </div>
      </div>

      <div class="settings-group">
        <h4>About</h4>
        <div class="about-card">
          <div class="about-title">
            <span>PokeTokenBar</span>
            <span class="version-tag">v0.1.3</span>
          </div>
          <p class="sub">Pokémon companion for your AI coding tokens on Windows & Linux.</p>
        </div>
      </div>
    </div>
  {:else if snap}
    {@const c = snap.companion}
    {@const u = snap.usage}

    <section class="today">
      <div class="metric">
        <span class="value">{compact(u.todayTotalTokens)}</span>
        <span class="label">tokens today</span>
      </div>
      <div class="metric">
        <span class="value">${u.todayCostTotal.toFixed(2)}</span>
        <span class="label">spent today</span>
      </div>
      <div class="metric">
        <span class="value">{burnLabel[u.burnTier] ?? u.burnTier}</span>
        <span class="label">pace</span>
      </div>
    </section>

    <section class="companion">
      {#if c.isEgg}
        <div class="sprite egg">🥚</div>
        <div class="info">
          <h2>Token Egg</h2>
          <p class="sub">burn tokens to hatch it</p>
          <div class="bar"><div class="fill" style="width: {(c.eggProgress * 100).toFixed(1)}%"></div></div>
          <p class="sub">{tokens(c.eggTokensToHatch)} tokens to hatch</p>
        </div>
      {:else}
        <img
          class="sprite"
          src={spriteUrl(c.currentSpeciesId ?? 0, c.isShiny)}
          alt={c.displayName}
          onerror={(e) => ((e.currentTarget as HTMLImageElement).style.display = "none")}
        />
        <div class="info">
          <h2>
            {c.displayName}
            {#if c.isShiny}<span class="shiny">✨</span>{/if}
          </h2>
          <p class="sub">
            {rarityLabel[c.rarity ?? ""] ?? c.rarity} · {c.stageText}
            {c.isFinalStage ? " · final" : ""}
          </p>
          <div class="bar"><div class="fill" style="width: {(c.progress * 100).toFixed(1)}%"></div></div>
          <p class="sub">{tokens(c.tokensToNext)} tokens to next stage</p>
          {#if c.justEvolvedTo}<p class="flash">Evolved into {c.justEvolvedTo}!</p>{/if}
          {#if c.justGraduated}<p class="flash">{c.justGraduated} graduated! 🎉</p>{/if}
        </div>
      {/if}
    </section>

    <section class="row">
      <div class="metric">
        <span class="value">{compact(u.weekTotalTokens)}</span>
        <span class="label">this week</span>
      </div>
      <div class="metric">
        <span class="value">{compact(u.monthTotalTokens)}</span>
        <span class="label">this month</span>
      </div>
      <div class="metric">
        <span class="value">{tokens(c.availableTokens)}</span>
        <span class="label">shop balance</span>
      </div>
    </section>

    {#if u.limits && (u.limits.fiveHour?.utilization != null || u.limits.sevenDay?.utilization != null)}
      <section class="limits-card">
        <div class="limits-header">
          <h3>Claude Limits</h3>
          {#if u.limits.planDisplay}
            <span class="plan-badge">{u.limits.planDisplay}</span>
          {/if}
        </div>
        {#if u.limits.fiveHour && u.limits.fiveHour.utilization != null}
          <div class="limit-row">
            <div class="limit-labels">
              <span>5-hour session</span>
              <span>{formatUtilization(u.limits.fiveHour.utilization)}</span>
            </div>
            <div class="bar"><div class="fill limit-fill" style="width: {getUtilizationPercent(u.limits.fiveHour.utilization)}%"></div></div>
          </div>
        {/if}
        {#if u.limits.sevenDay && u.limits.sevenDay.utilization != null}
          <div class="limit-row">
            <div class="limit-labels">
              <span>Weekly limit</span>
              <span>{formatUtilization(u.limits.sevenDay.utilization)}</span>
            </div>
            <div class="bar"><div class="fill limit-fill" style="width: {getUtilizationPercent(u.limits.sevenDay.utilization)}%"></div></div>
          </div>
        {/if}
      </section>
    {/if}

    {#if u.snapshots.length}
      <section>
        <h3>Sources</h3>
        {#each u.snapshots as s (s.id)}
          <div class="source">
            <span>{s.displayName}</span>
            <span>{compact(s.todayTotalTokens)}</span>
          </div>
        {/each}
      </section>
    {/if}

    <section>
      <h3>Bag</h3>
      {#if c.ownedItems.length === 0}
        <p class="sub">Nothing yet.</p>
      {:else}
        <div class="bag">
          {#each c.ownedItems as [kind, count] (kind)}
            <div class="bag-item">
              <span>{itemLabel[kind] ?? kind}</span>
              <span>×{count}</span>
            </div>
          {/each}
        </div>
      {/if}
      <div class="actions">
        <button disabled={!c.hasActive} onclick={useCandy}>Use Rare Candy</button>
        <button disabled={!c.hasActive} onclick={useMint}>Use Mint</button>
      </div>
    </section>

    <section>
      <h3>Shop</h3>
      {#each c.shop as item (item.kind + (item.tier ?? ""))}
        <div class="shop-row">
          <span>
            {item.kind === "egg"
              ? `Egg (${tierLabel[item.tier ?? ""] ?? "basic"})`
              : itemLabel[item.kind] ?? item.kind}
          </span>
          <button
            disabled={!item.canBuy}
            onclick={() => (item.kind === "egg" ? buyEgg(item.tier) : buy(item.kind))}
          >
            {tokens(item.price)} tk
          </button>
        </div>
      {/each}
    </section>

    <section>
      <h3>Pokédex ({c.dex.length})</h3>
      {#if c.dex.length === 0}
        <p class="sub">Hatch your first Pokémon to start your collection.</p>
      {:else}
        <div class="dex">
          {#each c.dex as d (d.id)}
            <div class="dex-item">
              <span class="dex-name">{d.name}{#if d.isShiny} ✨{/if}</span>
              <span class="dex-rarity">{rarityLabel[d.rarity] ?? d.rarity}</span>
            </div>
          {/each}
        </div>
      {/if}
    </section>
  {:else if !loading}
    <p class="sub">Loading…</p>
  {/if}
</div>

<style>
  :global(html, body) {
    margin: 0;
    padding: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
    color-scheme: dark;
  }

  * {
    box-sizing: border-box;
    margin: 0;
  }

  .app {
    font-family: "Inter", system-ui, -apple-system, sans-serif;
    color: #e6e6e6;
    background: linear-gradient(180deg, #1b1e26, #12141a);
    padding: 12px 14px;
    height: 100vh;
    overflow-y: auto;
    overflow-x: hidden;
    user-select: none;
    scrollbar-width: thin;
    scrollbar-color: #2c3240 transparent;
  }

  .app::-webkit-scrollbar {
    width: 6px;
  }

  .app::-webkit-scrollbar-track {
    background: transparent;
  }

  .app::-webkit-scrollbar-thumb {
    background: #2c3240;
    border-radius: 3px;
  }

  .app::-webkit-scrollbar-thumb:hover {
    background: #3d4659;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 10px;
    cursor: grab;
  }

  header:active {
    cursor: grabbing;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: grab;
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: default;
  }

  .close-btn {
    font-size: 11px;
    padding: 4px 7px;
  }

  .close-btn:hover {
    background: #4a2525;
    border-color: #703333;
    color: #ff9999;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #ff5d5d;
  }

  .dot.ok {
    background: #49c66c;
  }

  .title {
    font-weight: 600;
    font-size: 14px;
  }

  button {
    background: #2a2f3a;
    color: #e6e6e6;
    border: 1px solid #3a4150;
    border-radius: 8px;
    padding: 6px 10px;
    font-size: 13px;
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    background: #343b48;
  }

  button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  button.ghost {
    padding: 4px 8px;
  }

  section {
    margin-bottom: 14px;
  }

  h2 {
    font-size: 18px;
    font-weight: 600;
  }

  h3 {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: #8a93a3;
    margin-bottom: 6px;
  }

  .today,
  .row {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
  }

  .metric {
    display: flex;
    flex-direction: column;
    background: #1f232d;
    border: 1px solid #2c3240;
    border-radius: 10px;
    padding: 10px;
  }

  .metric .value {
    font-size: 17px;
    font-weight: 600;
  }

  .metric .label {
    font-size: 11px;
    color: #8a93a3;
  }

  .companion {
    display: flex;
    gap: 14px;
    align-items: center;
    background: #1f232d;
    border: 1px solid #2c3240;
    border-radius: 12px;
    padding: 14px;
  }

  .sprite {
    width: 84px;
    height: 84px;
    flex-shrink: 0;
    object-fit: contain;
  }

  .sprite.egg {
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 48px;
  }

  .info {
    flex: 1;
    min-width: 0;
  }

  .sub {
    font-size: 12px;
    color: #8a93a3;
    margin-top: 2px;
  }

  .shiny {
    margin-left: 4px;
  }

  .bar {
    height: 8px;
    background: #2c3240;
    border-radius: 999px;
    overflow: hidden;
    margin: 8px 0;
  }

  .fill {
    height: 100%;
    background: linear-gradient(90deg, #4f8cff, #6bd0ff);
    transition: width 0.3s ease;
  }

  .flash {
    font-size: 12px;
    color: #ffd75e;
    margin-top: 4px;
  }

  .source,
  .shop-row,
  .bag-item,
  .dex-item,
  .settings-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 7px 10px;
    background: #1f232d;
    border: 1px solid #2c3240;
    border-radius: 8px;
    margin-bottom: 6px;
    font-size: 13px;
  }

  .bag {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 6px;
  }

  .actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

  .dex {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 6px;
  }

  .dex-name {
    font-weight: 500;
  }

  .dex-rarity {
    font-size: 11px;
    color: #8a93a3;
  }

  .limits-card {
    background: #1f232d;
    border: 1px solid #2c3240;
    border-radius: 10px;
    padding: 10px 12px;
    margin-bottom: 10px;
  }

  .limits-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  .limits-header h3 {
    margin-bottom: 0;
  }

  .plan-badge {
    font-size: 11px;
    font-weight: 600;
    padding: 2px 7px;
    border-radius: 6px;
    background: #343b48;
    color: #92a4ff;
    border: 1px solid #4a5470;
  }

  .limit-row {
    margin-top: 6px;
  }

  .limit-labels {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: #a0a8b7;
    margin-bottom: 3px;
  }

  .limit-fill {
    background: linear-gradient(90deg, #49c66c, #e5a93c);
  }

  .error {
    color: #ff5d5d;
    font-size: 12px;
    margin-bottom: 8px;
  }

  .active-tab {
    background: #3a4459;
    border-color: #586684;
    color: #ffffff;
  }

  .settings-view {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding-bottom: 8px;
  }

  .settings-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 2px;
  }

  .settings-header h3 {
    margin-bottom: 0;
  }

  .back-btn {
    font-size: 12px;
    padding: 3px 8px;
  }

  .settings-group {
    background: #1a1e27;
    border: 1px solid #272d3b;
    border-radius: 10px;
    padding: 10px 12px;
  }

  .settings-group h4 {
    margin: 0 0 8px 0;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: #7b8599;
  }

  .setting-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .setting-title {
    font-size: 13px;
    font-weight: 500;
    color: #e6e6e6;
  }

  .setting-desc {
    font-size: 11px;
    color: #7b8599;
  }

  .custom-select {
    background: #252b38;
    color: #e6e6e6;
    border: 1px solid #3d4659;
    border-radius: 6px;
    padding: 5px 8px;
    font-size: 12px;
    outline: none;
    cursor: pointer;
  }

  .custom-select:focus {
    border-color: #4f8cff;
  }

  .about-card {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .about-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 600;
  }

  .version-tag {
    font-size: 11px;
    padding: 1px 6px;
    border-radius: 4px;
    background: #2b3342;
    color: #72a7ff;
    border: 1px solid #3e4b63;
  }
</style>
