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

  type Tab = "companion" | "stats" | "shop" | "pokedex";
  type Theme = "midnight" | "oled" | "cyberpunk" | "retro";

  let snap = $state<Snapshot | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  let currentTab = $state<Tab>("companion");
  let showSettings = $state(false);
  let currentTheme = $state<Theme>("midnight");
  let animatedSprites = $state(true);
  let refreshIntervalSec = $state(60);
  let autostartActive = $state(false);
  let celebrationDismissed = $state(false);

  let isRefreshing = false;
  async function refresh() {
    if (isRefreshing) return;
    isRefreshing = true;
    loading = true;
    error = null;
    try {
      snap = await invoke<Snapshot>("refresh");
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
      isRefreshing = false;
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
    if (e.button === 0 && !(e.target as HTMLElement).closest("button, select, input, .tab-btn")) {
      try {
        await getCurrentWindow().startDragging();
      } catch (err) {
        console.error("Failed to start dragging:", err);
      }
    }
  }

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

  function changeTheme(theme: Theme) {
    currentTheme = theme;
    try {
      localStorage.setItem("ptb_theme", theme);
    } catch {
      // ignore
    }
  }

  function toggleAnimatedSprites() {
    animatedSprites = !animatedSprites;
    try {
      localStorage.setItem("ptb_animated_sprites", animatedSprites ? "true" : "false");
    } catch {
      // ignore
    }
  }

  onMount(() => {
    try {
      const savedInterval = localStorage.getItem("ptb_refresh_interval");
      if (savedInterval) {
        const sec = parseInt(savedInterval, 10);
        if (!isNaN(sec) && sec >= 5) refreshIntervalSec = sec;
      }
      const savedTheme = localStorage.getItem("ptb_theme") as Theme;
      if (savedTheme && ["midnight", "oled", "cyberpunk", "retro"].includes(savedTheme)) {
        currentTheme = savedTheme;
      }
      const savedAnimated = localStorage.getItem("ptb_animated_sprites");
      if (savedAnimated !== null) {
        animatedSprites = savedAnimated === "true";
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

    const interval = setInterval(() => {
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

  function spriteUrl(id: number, shiny: boolean, animated = true): string {
    const dir = shiny ? "shiny/" : "";
    if (animated && animatedSprites) {
      return `https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/other/showdown/${dir}${id}.gif`;
    }
    return `https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/${dir}${id}.png`;
  }

  function fallbackStaticSprite(e: Event, id: number, shiny: boolean) {
    const img = e.currentTarget as HTMLImageElement;
    const dir = shiny ? "shiny/" : "";
    img.src = `https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/${dir}${id}.png`;
  }

  // Type mappings for common species IDs
  function getPokemonType(id: number): { name: string; color: string; icon: string } {
    const typeMap: Record<number, { name: string; color: string; icon: string }> = {
      1: { name: "Grass", color: "#78C850", icon: "🌿" },
      2: { name: "Grass", color: "#78C850", icon: "🌿" },
      3: { name: "Grass", color: "#78C850", icon: "🌿" },
      4: { name: "Fire", color: "#F08030", icon: "🔥" },
      5: { name: "Fire", color: "#F08030", icon: "🔥" },
      6: { name: "Fire", color: "#F08030", icon: "🔥" },
      7: { name: "Water", color: "#6890F0", icon: "💧" },
      8: { name: "Water", color: "#6890F0", icon: "💧" },
      9: { name: "Water", color: "#6890F0", icon: "💧" },
      25: { name: "Electric", color: "#F8D030", icon: "⚡" },
      26: { name: "Electric", color: "#F8D030", icon: "⚡" },
      133: { name: "Normal", color: "#A8A878", icon: "⚪" },
      134: { name: "Water", color: "#6890F0", icon: "💧" },
      135: { name: "Electric", color: "#F8D030", icon: "⚡" },
      136: { name: "Fire", color: "#F08030", icon: "🔥" },
      150: { name: "Psychic", color: "#F85888", icon: "🔮" },
      151: { name: "Psychic", color: "#F85888", icon: "🔮" },
    };
    return typeMap[id] ?? { name: "Pokémon", color: "#4f8cff", icon: "⭐" };
  }

  const rarityLabel: Record<string, string> = {
    common: "Common",
    uncommon: "Uncommon",
    rare: "Rare",
    legendary: "Legendary",
  };

  const itemLabel: Record<string, string> = {
    rareCandy: "Rare Candy",
    mint: "Nature Mint",
    shinyCharm: "Shiny Charm",
  };

  const itemDesc: Record<string, string> = {
    rareCandy: "Grants +50,000 evolution progress to your active Pokémon.",
    mint: "Rerolls your active Pokémon's nature and growth bonuses.",
    shinyCharm: "Increases shiny hatch odds significantly.",
  };

  const burnLabel: Record<string, { label: string; icon: string; class: string }> = {
    idle: { label: "Resting", icon: "💤", class: "burn-idle" },
    normal: { label: "Coding", icon: "⚡", class: "burn-normal" },
    fast: { label: "Focus Flow", icon: "🔥", class: "burn-fast" },
    blazing: { label: "On Fire!", icon: "🚀", class: "burn-blazing" },
  };

  function getBurnPace(tier: string) {
    return burnLabel[tier] ?? burnLabel.normal;
  }

  const tierLabel: Record<string, string> = {
    uncommon: "Uncommon+",
    rare: "Rare+",
  };

  // Sparkline percentage calculation
  function getProviderRatio(snapshots: ProviderView[]): { name: string; pct: number; color: string }[] {
    const total = snapshots.reduce((acc, s) => acc + s.todayTotalTokens, 0);
    if (total === 0) return [];
    const colors = ["#4f8cff", "#00d2b4", "#ff8a3d", "#a855f7", "#ec4899"];
    return snapshots.map((s, idx) => ({
      name: s.displayName,
      pct: (s.todayTotalTokens / total) * 100,
      color: colors[idx % colors.length],
    }));
  }
</script>

<div class="app theme-{currentTheme}">
  <header data-tauri-drag-region onmousedown={startDrag} role="toolbar" aria-label="Window header" tabindex="-1">
    <div class="header-left" data-tauri-drag-region>
      <div class="logo-group">
        <span class="dot" class:ok={!loading}></span>
        <span class="title">PokeTokenBar</span>
      </div>
    </div>
    <div class="header-right">
      <button
        class="ghost icon-btn"
        class:active-btn={showSettings}
        onclick={() => (showSettings = !showSettings)}
        title={showSettings ? "Back to Dashboard" : "Settings"}
      >
        ⚙
      </button>
      <button class="ghost icon-btn" onclick={togglePet} title="Toggle Floating Desktop Pet">
        🐾
      </button>
      <button class="ghost icon-btn" onclick={refresh} disabled={loading} title="Refresh token counts">
        <span class:spinning={loading}>↻</span>
      </button>
      <button class="ghost icon-btn close-btn" onclick={hideWindow} title="Hide Window">
        ✕
      </button>
    </div>
  </header>

  {#if error}
    <div class="error-banner">
      <span>⚠️ {error}</span>
      <button class="ghost small-btn" onclick={refresh}>Retry</button>
    </div>
  {/if}

  {#if showSettings}
    <div class="settings-view">
      <div class="settings-header">
        <h3>App Settings</h3>
        <button class="ghost back-btn" onclick={() => (showSettings = false)}>← Back</button>
      </div>

      <div class="settings-group">
        <h4>Visual Theme</h4>
        <div class="theme-picker">
          <button
            class="theme-chip"
            class:active={currentTheme === "midnight"}
            onclick={() => changeTheme("midnight")}
          >
            <span class="theme-preview midnight"></span>
            <span>Midnight</span>
          </button>
          <button
            class="theme-chip"
            class:active={currentTheme === "oled"}
            onclick={() => changeTheme("oled")}
          >
            <span class="theme-preview oled"></span>
            <span>OLED Black</span>
          </button>
          <button
            class="theme-chip"
            class:active={currentTheme === "cyberpunk"}
            onclick={() => changeTheme("cyberpunk")}
          >
            <span class="theme-preview cyberpunk"></span>
            <span>Cyberpunk</span>
          </button>
          <button
            class="theme-chip"
            class:active={currentTheme === "retro"}
            onclick={() => changeTheme("retro")}
          >
            <span class="theme-preview retro"></span>
            <span>Game Boy</span>
          </button>
        </div>
      </div>

      <div class="settings-group">
        <h4>General</h4>
        <div class="settings-row">
          <div class="setting-info">
            <span class="setting-title">Launch at Login</span>
            <span class="setting-desc">Start PokeTokenBar automatically on system startup</span>
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
            <span class="setting-desc">How often token logs are polled in the background</span>
          </div>
          <select
            class="custom-select"
            value={refreshIntervalSec}
            onchange={(e) => changeRefreshInterval(parseInt((e.target as HTMLSelectElement).value, 10))}
            aria-label="Refresh Interval"
          >
            <option value={15}>15 seconds</option>
            <option value={30}>30 seconds</option>
            <option value={60}>1 minute (recommended)</option>
            <option value={120}>2 minutes</option>
            <option value={300}>5 minutes</option>
          </select>
        </div>

        <div class="settings-row">
          <div class="setting-info">
            <span class="setting-title">Animated Pokémon Sprites</span>
            <span class="setting-desc">Use animated Showdown GIFs instead of static sprites</span>
          </div>
          <input
            type="checkbox"
            checked={animatedSprites}
            onchange={toggleAnimatedSprites}
            aria-label="Animated Pokémon Sprites"
          />
        </div>
      </div>

      <div class="settings-group">
        <h4>Desktop Companion Pet</h4>
        <div class="settings-row">
          <div class="setting-info">
            <span class="setting-title">Floating Widget</span>
            <span class="setting-desc">Always-on-top transparent companion pet on desktop</span>
          </div>
          <button class="ghost small-btn" onclick={togglePet}>Toggle Pet</button>
        </div>
      </div>

      <div class="settings-group">
        <h4>About</h4>
        <div class="about-card">
          <div class="about-title">
            <span>PokeTokenBar</span>
            <span class="version-tag">v0.2.0</span>
          </div>
          <p class="sub">Cross-platform Pokémon companion for your AI coding tokens on Windows & Linux.</p>
        </div>
      </div>
    </div>
  {:else if snap}
    {@const c = snap.companion}
    {@const u = snap.usage}

    <!-- Evolution / Graduation Celebration Banner -->
    {#if (c.justEvolvedTo || c.justGraduated) && !celebrationDismissed}
      <div class="celebration-banner">
        <div class="celebration-sparkles">✨ 🎉 ✨</div>
        <div class="celebration-content">
          {#if c.justEvolvedTo}
            <strong>Evolution!</strong> Your buddy evolved into <strong>{c.justEvolvedTo}</strong>!
          {:else if c.justGraduated}
            <strong>Graduation!</strong> <strong>{c.justGraduated}</strong> reached final stage!
          {/if}
        </div>
        <button class="ghost close-celebration" onclick={() => (celebrationDismissed = true)}>✕</button>
      </div>
    {/if}

    <!-- Tab Bar Navigation -->
    <div class="nav-tabs" role="tablist" aria-label="Navigation Tabs">
      <button
        class="tab-btn"
        class:active={currentTab === "companion"}
        onclick={() => (currentTab = "companion")}
        role="tab"
        aria-selected={currentTab === "companion"}
      >
        <span class="tab-icon">🐾</span>
        <span class="tab-label">Buddy</span>
      </button>
      <button
        class="tab-btn"
        class:active={currentTab === "stats"}
        onclick={() => (currentTab = "stats")}
        role="tab"
        aria-selected={currentTab === "stats"}
      >
        <span class="tab-icon">📊</span>
        <span class="tab-label">Usage</span>
      </button>
      <button
        class="tab-btn"
        class:active={currentTab === "shop"}
        onclick={() => (currentTab = "shop")}
        role="tab"
        aria-selected={currentTab === "shop"}
      >
        <span class="tab-icon">🎒</span>
        <span class="tab-label">Shop & Bag</span>
      </button>
      <button
        class="tab-btn"
        class:active={currentTab === "pokedex"}
        onclick={() => (currentTab = "pokedex")}
        role="tab"
        aria-selected={currentTab === "pokedex"}
      >
        <span class="tab-icon">📖</span>
        <span class="tab-label">Pokédex</span>
        {#if c.dex.length > 0}
          <span class="badge-count">{c.dex.length}</span>
        {/if}
      </button>
    </div>

    <!-- Tab 1: Companion -->
    {#if currentTab === "companion"}
      <div class="tab-content">
        <section class="companion-hero">
          {#if c.isEgg}
            <div class="hero-left">
              <div class="sprite egg-anim">🥚</div>
            </div>
            <div class="hero-right">
              <div class="hero-header">
                <h2>Token Egg</h2>
                <span class="type-pill" style="background: #e5a93c;">🐣 Egg</span>
              </div>
              <p class="sub">Burn tokens while coding to hatch this egg!</p>
              <div class="progress-wrap">
                <div class="bar">
                  <div class="fill egg-fill" style="width: {(c.eggProgress * 100).toFixed(1)}%"></div>
                </div>
                <div class="progress-labels">
                  <span>{(c.eggProgress * 100).toFixed(0)}% Hatched</span>
                  <span>{tokens(c.eggTokensToHatch)} tokens left</span>
                </div>
              </div>
            </div>
          {:else if c.hasActive && c.currentSpeciesId}
            {@const pType = getPokemonType(c.currentSpeciesId)}
            <div class="hero-left">
              <img
                class="sprite mon-anim"
                src={spriteUrl(c.currentSpeciesId, c.isShiny)}
                alt={c.displayName}
                onerror={(e) => fallbackStaticSprite(e, c.currentSpeciesId ?? 1, c.isShiny)}
              />
            </div>
            <div class="hero-right">
              <div class="hero-header">
                <h2>
                  {c.displayName}
                  {#if c.isShiny}<span class="shiny-star" title="Shiny Pokémon!">✨</span>{/if}
                </h2>
                <div class="pill-group">
                  <span class="type-pill" style="background: {pType.color};">{pType.icon} {pType.name}</span>
                  <span class="rarity-pill {c.rarity}">{rarityLabel[c.rarity ?? ""] ?? c.rarity}</span>
                </div>
              </div>

              <p class="sub">
                {c.stageText} {#if c.isFinalStage}· <span class="max-badge">Mastered</span>{/if}
              </p>

              {#if !c.isFinalStage}
                <div class="progress-wrap">
                  <div class="bar">
                    <div class="fill mon-fill" style="width: {(c.progress * 100).toFixed(1)}%"></div>
                  </div>
                  <div class="progress-labels">
                    <span>{(c.progress * 100).toFixed(0)}% Growth</span>
                    <span>{tokens(c.tokensToNext)} to Next Stage</span>
                  </div>
                </div>
              {:else}
                <div class="maxed-notice">🌟 Final Evolution Reached!</div>
              {/if}
            </div>
          {:else}
            <div class="empty-state">
              <span class="empty-icon">🥚</span>
              <p>No active companion. Visit the PokéShop to adopt an egg!</p>
              <button class="ghost small-btn" onclick={() => (currentTab = "shop")}>Go to Shop</button>
            </div>
          {/if}
        </section>

        <!-- Quick Summary Strip -->
        <section class="metrics-grid">
          <div class="metric-card">
            <span class="metric-val">{compact(u.todayTotalTokens)}</span>
            <span class="metric-lbl">Tokens Today</span>
          </div>
          <div class="metric-card">
            <span class="metric-val">${u.todayCostTotal.toFixed(2)}</span>
            <span class="metric-lbl">Cost Today</span>
          </div>
          <div class="metric-card">
            <span class="metric-val {getBurnPace(u.burnTier).class}">
              {getBurnPace(u.burnTier).icon} {getBurnPace(u.burnTier).label}
            </span>
            <span class="metric-lbl">Current Pace</span>
          </div>
        </section>

        <!-- Quick Item Actions -->
        {#if c.hasActive && !c.isEgg}
          <section class="quick-actions-card">
            <div class="card-title">Quick Items</div>
            <div class="actions-row">
              <button class="item-btn" onclick={useCandy} title="Add +50,000 progress">
                <span>🍬 Use Rare Candy</span>
              </button>
              <button class="item-btn" onclick={useMint} title="Reroll nature">
                <span>🌱 Use Mint</span>
              </button>
            </div>
          </section>
        {/if}
      </div>
    {/if}

    <!-- Tab 2: Stats & Limits -->
    {#if currentTab === "stats"}
      <div class="tab-content">
        <!-- Usage Timeframe Cards -->
        <section class="metrics-grid">
          <div class="metric-card">
            <span class="metric-val">{compact(u.todayTotalTokens)}</span>
            <span class="metric-lbl">Today</span>
          </div>
          <div class="metric-card">
            <span class="metric-val">{compact(u.weekTotalTokens)}</span>
            <span class="metric-lbl">This Week</span>
          </div>
          <div class="metric-card">
            <span class="metric-val">{compact(u.monthTotalTokens)}</span>
            <span class="metric-lbl">This Month</span>
          </div>
        </section>

        <!-- Provider Ratio Breakdown Bar -->
        {#if u.snapshots.length > 0}
          {@const ratios = getProviderRatio(u.snapshots)}
          <section class="provider-section">
            <div class="section-title">Active AI Tools Today</div>
            <div class="ratio-bar">
              {#each ratios as r}
                <div
                  class="ratio-segment"
                  style="width: {r.pct}%; background: {r.color};"
                  title="{r.name}: {r.pct.toFixed(1)}%"
                ></div>
              {/each}
            </div>
            <div class="sources-list">
              {#each u.snapshots as s, idx (s.id)}
                <div class="source-row">
                  <div class="source-info">
                    <span class="source-dot" style="background: {ratios[idx]?.color ?? '#4f8cff'};"></span>
                    <span class="source-name">{s.displayName}</span>
                  </div>
                  <div class="source-counts">
                    <span class="token-num">{compact(s.todayTotalTokens)}</span>
                    <span class="token-sub">today</span>
                  </div>
                </div>
              {/each}
            </div>
          </section>
        {/if}

        <!-- Claude Limits Card -->
        {#if u.limits && (u.limits.fiveHour?.utilization != null || u.limits.sevenDay?.utilization != null)}
          <section class="limits-card">
            <div class="limits-header">
              <div class="limits-title-group">
                <h3>Claude Rate Limits</h3>
                {#if u.limits.planDisplay}
                  <span class="plan-badge">{u.limits.planDisplay}</span>
                {/if}
              </div>
            </div>

            {#if u.limits.fiveHour && u.limits.fiveHour.utilization != null}
              <div class="limit-row">
                <div class="limit-labels">
                  <span>5-Hour Session Limit</span>
                  <span class="limit-pct">{formatUtilization(u.limits.fiveHour.utilization)}</span>
                </div>
                <div class="bar">
                  <div class="fill limit-fill" style="width: {getUtilizationPercent(u.limits.fiveHour.utilization)}%"></div>
                </div>
              </div>
            {/if}

            {#if u.limits.sevenDay && u.limits.sevenDay.utilization != null}
              <div class="limit-row">
                <div class="limit-labels">
                  <span>7-Day Weekly Limit</span>
                  <span class="limit-pct">{formatUtilization(u.limits.sevenDay.utilization)}</span>
                </div>
                <div class="bar">
                  <div class="fill limit-fill" style="width: {getUtilizationPercent(u.limits.sevenDay.utilization)}%"></div>
                </div>
              </div>
            {/if}
          </section>
        {/if}
      </div>
    {/if}

    <!-- Tab 3: Shop & Bag -->
    {#if currentTab === "shop"}
      <div class="tab-content">
        <!-- Balance Header -->
        <div class="balance-card">
          <div class="balance-left">
            <span class="coin-icon">🪙</span>
            <div class="balance-text">
              <span class="balance-num">{tokens(c.availableTokens)}</span>
              <span class="balance-lbl">Tokens Available to Spend</span>
            </div>
          </div>
        </div>

        <!-- Bag / Inventory -->
        <section class="inventory-section">
          <div class="section-title">My Bag</div>
          {#if c.ownedItems.length === 0}
            <p class="sub empty-bag">Your bag is empty. Buy items below!</p>
          {:else}
            <div class="bag-grid">
              {#each c.ownedItems as [kind, count] (kind)}
                <div class="bag-card">
                  <div class="bag-header">
                    <span class="item-name">{itemLabel[kind] ?? kind}</span>
                    <span class="item-count">×{count}</span>
                  </div>
                  <p class="item-desc">{itemDesc[kind] ?? ""}</p>
                  {#if kind === "rareCandy"}
                    <button class="ghost small-btn" disabled={!c.hasActive || c.isEgg} onclick={useCandy}>
                      Use Candy
                    </button>
                  {:else if kind === "mint"}
                    <button class="ghost small-btn" disabled={!c.hasActive || c.isEgg} onclick={useMint}>
                      Use Mint
                    </button>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        </section>

        <!-- Shop -->
        <section class="shop-section">
          <div class="section-title">PokéShop</div>
          <div class="shop-list">
            {#each c.shop as item (item.kind + (item.tier ?? ""))}
              <div class="shop-card">
                <div class="shop-info">
                  <div class="shop-item-name">
                    {item.kind === "egg"
                      ? `Pokémon Egg (${tierLabel[item.tier ?? ""] ?? "Basic"})`
                      : itemLabel[item.kind] ?? item.kind}
                  </div>
                  <div class="shop-item-price">
                    <span class="coin-tiny">🪙</span> {tokens(item.price)} tokens
                  </div>
                </div>
                <button
                  class="buy-btn"
                  disabled={!item.canBuy}
                  onclick={() => (item.kind === "egg" ? buyEgg(item.tier) : buy(item.kind))}
                >
                  Buy
                </button>
              </div>
            {/each}
          </div>
        </section>
      </div>
    {/if}

    <!-- Tab 4: Pokédex -->
    {#if currentTab === "pokedex"}
      <div class="tab-content">
        <div class="pokedex-header">
          <div class="section-title">Discovered Pokémon</div>
          <span class="dex-counter">{c.dex.length} Collected</span>
        </div>

        {#if c.dex.length === 0}
          <div class="empty-state">
            <span class="empty-icon">📖</span>
            <p>Your Pokédex is currently empty.</p>
            <p class="sub">Hatch eggs with your coding tokens to fill your collection!</p>
          </div>
        {:else}
          <div class="pokedex-grid">
            {#each c.dex as d (d.id)}
              {@const type = getPokemonType(d.id)}
              <div class="dex-card" class:active-buddy={d.isRaising}>
                {#if d.isRaising}
                  <span class="active-badge">Active</span>
                {/if}
                <img
                  class="dex-sprite"
                  src={spriteUrl(d.id, d.isShiny, false)}
                  alt={d.name}
                  loading="lazy"
                />
                <div class="dex-info">
                  <div class="dex-name">
                    {d.name}
                    {#if d.isShiny}<span class="shiny-star">✨</span>{/if}
                  </div>
                  <span class="type-mini-pill" style="background: {type.color};">{type.name}</span>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

  {:else if !loading}
    <div class="loading-state">
      <div class="loading-spinner"></div>
      <p class="sub">Connecting to token providers…</p>
    </div>
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
    padding: 0;
  }

  /* Theme Foundations */
  .app {
    font-family: "Inter", system-ui, -apple-system, sans-serif;
    color: #e6e6e6;
    padding: 12px 14px;
    height: 100vh;
    overflow-y: auto;
    overflow-x: hidden;
    user-select: none;
    scrollbar-width: thin;
    scrollbar-color: rgba(255, 255, 255, 0.15) transparent;
    transition: background 0.3s ease, color 0.3s ease;
  }

  /* Midnight Glass Theme */
  .theme-midnight {
    background: linear-gradient(180deg, #151922 0%, #0d1017 100%);
    --card-bg: rgba(26, 32, 45, 0.7);
    --card-border: rgba(255, 255, 255, 0.08);
    --accent: #4f8cff;
    --accent-glow: rgba(79, 140, 255, 0.3);
  }

  /* OLED Pitch Black Theme */
  .theme-oled {
    background: #000000;
    --card-bg: #0c0d10;
    --card-border: #20222a;
    --accent: #00ff88;
    --accent-glow: rgba(0, 255, 136, 0.3);
  }

  /* Cyberpunk Neon Theme */
  .theme-cyberpunk {
    background: linear-gradient(180deg, #180d24 0%, #0d0615 100%);
    --card-bg: rgba(36, 17, 54, 0.75);
    --card-border: rgba(255, 0, 128, 0.25);
    --accent: #ff007f;
    --accent-glow: rgba(255, 0, 127, 0.4);
  }

  /* Game Boy Retro Theme */
  .theme-retro {
    background: #1f2a1a;
    color: #9bbc0f;
    --card-bg: #151e12;
    --card-border: #304226;
    --accent: #8bac0f;
    --accent-glow: rgba(139, 172, 15, 0.3);
  }

  /* Scrollbars */
  .app::-webkit-scrollbar {
    width: 5px;
  }
  .app::-webkit-scrollbar-track {
    background: transparent;
  }
  .app::-webkit-scrollbar-thumb {
    background: var(--card-border);
    border-radius: 3px;
  }

  /* Header */
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
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

  .logo-group {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #ff5d5d;
    transition: background 0.3s ease;
  }
  .dot.ok {
    background: #00e676;
    box-shadow: 0 0 8px rgba(0, 230, 118, 0.5);
  }

  .title {
    font-weight: 700;
    font-size: 14px;
    letter-spacing: -0.2px;
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .icon-btn {
    padding: 5px 8px;
    font-size: 13px;
    border-radius: 6px;
    background: transparent;
    border: 1px solid transparent;
    color: #a0a8b8;
    cursor: pointer;
    transition: all 0.15s ease;
  }
  .icon-btn:hover {
    background: var(--card-bg);
    border-color: var(--card-border);
    color: #ffffff;
  }
  .icon-btn.active-btn {
    background: var(--accent);
    color: #ffffff;
    border-color: var(--accent);
  }

  .close-btn:hover {
    background: rgba(255, 75, 75, 0.2);
    border-color: #ff4b4b;
    color: #ff8888;
  }

  .spinning {
    display: inline-block;
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    100% {
      transform: rotate(360deg);
    }
  }

  /* Navigation Tabs */
  .nav-tabs {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 4px;
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    border-radius: 10px;
    padding: 3px;
    margin-bottom: 12px;
  }

  .tab-btn {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    padding: 6px 4px;
    background: transparent;
    border: none;
    border-radius: 8px;
    color: #8b95a8;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.2s ease;
  }

  .tab-btn:hover {
    color: #e6e6e6;
  }

  .tab-btn.active {
    background: var(--accent);
    color: #ffffff;
    box-shadow: 0 2px 8px var(--accent-glow);
  }

  .badge-count {
    font-size: 9px;
    padding: 1px 5px;
    border-radius: 10px;
    background: rgba(0, 0, 0, 0.4);
    color: #ffffff;
  }

  .tab-content {
    display: flex;
    flex-direction: column;
    gap: 10px;
    animation: fadeIn 0.2s ease;
  }
  @keyframes fadeIn {
    from {
      opacity: 0;
      transform: translateY(3px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  /* Celebration Banner */
  .celebration-banner {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    background: linear-gradient(90deg, #ff8a00, #e52e71);
    color: white;
    border-radius: 10px;
    margin-bottom: 10px;
    font-size: 12px;
    box-shadow: 0 4px 14px rgba(229, 46, 113, 0.4);
    animation: bounceIn 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275);
  }
  @keyframes bounceIn {
    0% {
      opacity: 0;
      transform: scale(0.9);
    }
    100% {
      opacity: 1;
      transform: scale(1);
    }
  }
  .celebration-sparkles {
    font-size: 16px;
  }
  .celebration-content {
    flex: 1;
  }
  .close-celebration {
    background: transparent;
    border: none;
    color: white;
    font-size: 12px;
    cursor: pointer;
    opacity: 0.8;
  }
  .close-celebration:hover {
    opacity: 1;
  }

  /* Companion Hero Card */
  .companion-hero {
    display: flex;
    align-items: center;
    gap: 14px;
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    border-radius: 12px;
    padding: 14px 16px;
    backdrop-filter: blur(12px);
  }

  .hero-left {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 90px;
  }

  .sprite {
    width: 86px;
    height: 86px;
    image-rendering: pixelated;
    filter: drop-shadow(0 6px 12px rgba(0, 0, 0, 0.5));
  }

  .egg-anim {
    font-size: 56px;
    animation: eggWobble 2s infinite ease-in-out;
  }
  @keyframes eggWobble {
    0%, 100% {
      transform: rotate(0deg);
    }
    25% {
      transform: rotate(-6deg);
    }
    75% {
      transform: rotate(6deg);
    }
  }

  .mon-anim {
    animation: idleFloat 3s infinite ease-in-out;
  }
  @keyframes idleFloat {
    0%, 100% {
      transform: translateY(0);
    }
    50% {
      transform: translateY(-4px);
    }
  }

  .hero-right {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .hero-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
  }

  .hero-header h2 {
    font-size: 16px;
    font-weight: 700;
  }

  .shiny-star {
    font-size: 13px;
    margin-left: 2px;
  }

  .pill-group {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .type-pill {
    font-size: 10px;
    font-weight: 700;
    padding: 2px 6px;
    border-radius: 6px;
    color: #ffffff;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6);
  }

  .rarity-pill {
    font-size: 10px;
    font-weight: 600;
    padding: 2px 6px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.1);
    color: #b0bac9;
  }
  .rarity-pill.rare,
  .rarity-pill.legendary {
    background: rgba(255, 215, 0, 0.2);
    color: #ffd700;
    border: 1px solid rgba(255, 215, 0, 0.4);
  }

  .sub {
    font-size: 11px;
    color: #8893a7;
  }

  .progress-wrap {
    margin-top: 6px;
  }

  .bar {
    height: 7px;
    background: rgba(0, 0, 0, 0.35);
    border-radius: 999px;
    overflow: hidden;
    border: 1px solid rgba(255, 255, 255, 0.05);
  }

  .fill {
    height: 100%;
    border-radius: 999px;
    transition: width 0.4s ease;
  }
  .mon-fill {
    background: linear-gradient(90deg, #4f8cff, #00d2b4);
  }
  .egg-fill {
    background: linear-gradient(90deg, #f59e0b, #ef4444);
  }
  .limit-fill {
    background: linear-gradient(90deg, #10b981, #f59e0b);
  }

  .progress-labels {
    display: flex;
    justify-content: space-between;
    font-size: 10px;
    color: #8893a7;
    margin-top: 4px;
    font-weight: 500;
  }

  .max-badge {
    color: #ffd700;
    font-weight: 600;
  }
  .maxed-notice {
    font-size: 11px;
    color: #ffd700;
    margin-top: 6px;
    font-weight: 600;
  }

  /* Metrics Grid */
  .metrics-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
  }

  .metric-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    border-radius: 10px;
    padding: 10px 8px;
    text-align: center;
  }

  .metric-val {
    font-size: 15px;
    font-weight: 700;
    letter-spacing: -0.3px;
  }
  .metric-lbl {
    font-size: 10px;
    color: #7b8599;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    margin-top: 2px;
  }

  .burn-idle {
    color: #7b8599;
  }
  .burn-normal {
    color: #00e676;
  }
  .burn-fast {
    color: #ffb300;
  }
  .burn-blazing {
    color: #ff3d00;
    text-shadow: 0 0 8px rgba(255, 61, 0, 0.4);
  }

  /* Quick Actions Card */
  .quick-actions-card {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    border-radius: 10px;
    padding: 10px 12px;
  }

  .card-title {
    font-size: 11px;
    font-weight: 700;
    color: #7b8599;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 8px;
  }

  .actions-row {
    display: flex;
    gap: 8px;
  }

  .item-btn {
    flex: 1;
    padding: 7px 10px;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid var(--card-border);
    border-radius: 8px;
    color: #e6e6e6;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s ease;
  }
  .item-btn:hover {
    background: var(--accent);
    color: #ffffff;
    border-color: var(--accent);
  }

  /* Provider Ratio Section */
  .provider-section {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    border-radius: 10px;
    padding: 12px;
  }

  .section-title {
    font-size: 12px;
    font-weight: 700;
    margin-bottom: 8px;
  }

  .ratio-bar {
    display: flex;
    height: 8px;
    border-radius: 4px;
    overflow: hidden;
    margin-bottom: 10px;
    background: rgba(0, 0, 0, 0.3);
  }

  .ratio-segment {
    height: 100%;
    transition: width 0.3s ease;
  }

  .sources-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .source-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 12px;
  }

  .source-info {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .source-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .token-num {
    font-weight: 700;
  }

  .token-sub {
    font-size: 10px;
    color: #7b8599;
    margin-left: 3px;
  }

  /* Limits Card */
  .limits-card {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    border-radius: 10px;
    padding: 12px;
  }

  .limits-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 10px;
  }

  .limits-title-group {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .limits-title-group h3 {
    font-size: 13px;
    font-weight: 700;
  }

  .plan-badge {
    font-size: 10px;
    font-weight: 700;
    padding: 2px 7px;
    border-radius: 6px;
    background: rgba(79, 140, 255, 0.2);
    color: #72a7ff;
    border: 1px solid rgba(79, 140, 255, 0.4);
  }

  .limit-row {
    margin-top: 8px;
  }

  .limit-labels {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: #8893a7;
    margin-bottom: 4px;
  }
  .limit-pct {
    font-weight: 700;
    color: #e6e6e6;
  }

  /* Balance Card */
  .balance-card {
    display: flex;
    align-items: center;
    background: linear-gradient(90deg, rgba(79, 140, 255, 0.15), rgba(0, 210, 180, 0.15));
    border: 1px solid var(--card-border);
    border-radius: 10px;
    padding: 12px 14px;
  }

  .balance-left {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .coin-icon {
    font-size: 24px;
  }

  .balance-text {
    display: flex;
    flex-direction: column;
  }

  .balance-num {
    font-size: 17px;
    font-weight: 800;
    color: #ffffff;
  }

  .balance-lbl {
    font-size: 10px;
    color: #8893a7;
    text-transform: uppercase;
    letter-spacing: 0.4px;
  }

  /* Bag Grid */
  .inventory-section,
  .shop-section {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    border-radius: 10px;
    padding: 12px;
  }

  .empty-bag {
    padding: 8px 0;
  }

  .bag-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 8px;
  }

  .bag-card {
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid var(--card-border);
    border-radius: 8px;
    padding: 8px 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .bag-header {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    font-weight: 700;
  }

  .item-count {
    color: var(--accent);
  }

  .item-desc {
    font-size: 10px;
    color: #7b8599;
    line-height: 1.3;
    margin-bottom: 4px;
  }

  /* Shop List */
  .shop-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .shop-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid var(--card-border);
    border-radius: 8px;
    padding: 8px 10px;
  }

  .shop-item-name {
    font-size: 12px;
    font-weight: 600;
  }

  .shop-item-price {
    font-size: 11px;
    color: #ffd700;
    font-weight: 700;
    display: flex;
    align-items: center;
    gap: 3px;
  }

  .buy-btn {
    padding: 5px 12px;
    background: var(--accent);
    border: none;
    border-radius: 6px;
    color: #ffffff;
    font-size: 11px;
    font-weight: 700;
    cursor: pointer;
    transition: opacity 0.2s ease;
  }
  .buy-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  /* Pokédex Grid */
  .pokedex-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 2px;
  }

  .dex-counter {
    font-size: 11px;
    font-weight: 600;
    color: #7b8599;
  }

  .pokedex-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
  }

  .dex-card {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    border-radius: 10px;
    padding: 10px 6px;
    text-align: center;
    transition: transform 0.15s ease;
  }
  .dex-card:hover {
    transform: translateY(-2px);
    border-color: var(--accent);
  }

  .dex-card.active-buddy {
    border-color: var(--accent);
    box-shadow: 0 0 10px var(--accent-glow);
  }

  .active-badge {
    position: absolute;
    top: 4px;
    right: 4px;
    font-size: 8px;
    font-weight: 800;
    padding: 1px 4px;
    border-radius: 4px;
    background: var(--accent);
    color: white;
  }

  .dex-sprite {
    width: 56px;
    height: 56px;
    image-rendering: pixelated;
  }

  .dex-info {
    margin-top: 4px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
  }

  .dex-name {
    font-size: 11px;
    font-weight: 700;
  }

  .type-mini-pill {
    font-size: 9px;
    font-weight: 700;
    padding: 1px 5px;
    border-radius: 4px;
    color: white;
  }

  /* Settings View */
  .settings-view {
    display: flex;
    flex-direction: column;
    gap: 10px;
    animation: fadeIn 0.2s ease;
  }

  .settings-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .settings-header h3 {
    font-size: 14px;
    font-weight: 700;
  }

  .back-btn {
    font-size: 11px;
    padding: 4px 8px;
    border-radius: 6px;
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    color: #e6e6e6;
    cursor: pointer;
  }

  .settings-group {
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    border-radius: 10px;
    padding: 10px 12px;
  }

  .settings-group h4 {
    font-size: 10px;
    font-weight: 700;
    color: #7b8599;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 8px;
  }

  .theme-picker {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 6px;
  }

  .theme-chip {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    background: rgba(0, 0, 0, 0.3);
    border: 1px solid var(--card-border);
    border-radius: 6px;
    color: #e6e6e6;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s ease;
  }
  .theme-chip.active {
    border-color: var(--accent);
    background: rgba(79, 140, 255, 0.15);
  }

  .theme-preview {
    width: 12px;
    height: 12px;
    border-radius: 50%;
  }
  .theme-preview.midnight {
    background: #4f8cff;
  }
  .theme-preview.oled {
    background: #00ff88;
  }
  .theme-preview.cyberpunk {
    background: #ff007f;
  }
  .theme-preview.retro {
    background: #8bac0f;
  }

  .settings-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 0;
    border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  }
  .settings-row:last-child {
    border-bottom: none;
  }

  .setting-info {
    display: flex;
    flex-direction: column;
  }

  .setting-title {
    font-size: 12px;
    font-weight: 600;
  }

  .setting-desc {
    font-size: 10px;
    color: #7b8599;
  }

  .custom-select {
    background: rgba(0, 0, 0, 0.3);
    color: #e6e6e6;
    border: 1px solid var(--card-border);
    border-radius: 6px;
    padding: 4px 6px;
    font-size: 11px;
    outline: none;
  }

  .about-card {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .about-title {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 700;
  }

  .version-tag {
    font-size: 10px;
    padding: 1px 5px;
    border-radius: 4px;
    background: rgba(79, 140, 255, 0.2);
    color: #72a7ff;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 24px 12px;
    text-align: center;
    gap: 6px;
  }
  .empty-icon {
    font-size: 32px;
  }

  .error-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: rgba(255, 93, 93, 0.15);
    border: 1px solid #ff5d5d;
    color: #ff9999;
    padding: 6px 10px;
    border-radius: 8px;
    font-size: 11px;
    margin-bottom: 8px;
  }

  .small-btn {
    padding: 4px 8px;
    font-size: 11px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid var(--card-border);
    color: #e6e6e6;
    cursor: pointer;
  }

  .loading-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 48px 0;
    gap: 12px;
  }

  .loading-spinner {
    width: 24px;
    height: 24px;
    border: 2px solid rgba(255, 255, 255, 0.15);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
</style>
