<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
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

  interface UsageView {
    todayTotalTokens: number;
    todayCostTotal: number;
    weekTotalTokens: number;
    monthTotalTokens: number;
    burnTier: string;
    snapshots: ProviderView[];
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

  onMount(() => {
    refresh();
    const unlisten = listen("tray-refresh", () => refresh());
    return () => {
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

  function spriteUrl(id: number, shiny: boolean): string {
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
  <header>
    <div class="header-left">
      <span class="dot" class:ok={!loading}></span>
      <span class="title">PokeTokenBar</span>
    </div>
    <button class="ghost" onclick={refresh} disabled={loading}>
      {loading ? "…" : "↻"}
    </button>
  </header>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#if snap}
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
  :global(:root) {
    color-scheme: dark;
    background: transparent;
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
    user-select: none;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 10px;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 8px;
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
  .dex-item {
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

  .error {
    color: #ff5d5d;
    font-size: 12px;
    margin-bottom: 8px;
  }
</style>
