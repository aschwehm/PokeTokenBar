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
    hasGoldenAura?: boolean;
    berryFeedback?: string | null;
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

  interface PokedexDetails {
    id: number;
    name: string;
    genus: string;
    flavorText: string;
    heightM: number;
    weightKg: number;
    captureRate: number;
    isLegendary: boolean;
    isMythical: boolean;
  }

  interface Snapshot {
    companion: CompanionView;
    usage: UsageView;
  }

  type Tab = "buddy" | "usage" | "shop" | "pokedex";
  type Theme = "midnight" | "oled" | "cyberpunk" | "retro";

  let snap = $state<Snapshot | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  let currentTab = $state<Tab>("buddy");
  let showSettings = $state(false);
  let currentTheme = $state<Theme>("midnight");
  let animatedSprites = $state(true);
  let refreshIntervalSec = $state(60);
  let autostartActive = $state(false);
  let celebrationDismissed = $state(false);
  let shakeItemId = $state<string | null>(null);

  let selectedDexMon = $state<{
    id: number;
    name: string;
    isShiny: boolean;
    isRaising?: boolean;
  } | null>(null);
  let dexDetails = $state<PokedexDetails | null>(null);
  let dexDetailsLoading = $state(false);

  async function openDexDetails(mon: { id: number; name: string; isShiny: boolean; isRaising?: boolean }) {
    selectedDexMon = mon;
    dexDetails = null;
    dexDetailsLoading = true;
    try {
      const res = await invoke<PokedexDetails | null>("get_pokedex_details", {
        id: mon.id,
        lang: "en",
      });
      if (selectedDexMon?.id === mon.id) {
        dexDetails = res;
      }
    } catch {
      // fallback handled gracefully
    } finally {
      if (selectedDexMon?.id === mon.id) {
        dexDetailsLoading = false;
      }
    }
  }

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

  async function buy(kind: string, price: number, canBuy: boolean) {
    if (!canBuy || (snap && snap.companion.availableTokens < price)) {
      triggerShake(kind);
      return;
    }
    snap = await invoke<Snapshot>("buy_item", { kind });
  }

  async function buyEgg(tier: string | null, price: number, canBuy: boolean) {
    const key = `egg_${tier ?? "basic"}`;
    if (!canBuy || (snap && snap.companion.availableTokens < price)) {
      triggerShake(key);
      return;
    }
    snap = await invoke<Snapshot>("buy_egg", { tier });
  }

  function triggerShake(id: string) {
    shakeItemId = id;
    setTimeout(() => {
      if (shakeItemId === id) shakeItemId = null;
    }, 400);
  }

  // Hero Petting & Interactive Moods
  interface HeroHeart {
    id: number;
    emoji: string;
    x: number;
    y: number;
    scale: number;
  }
  let heroHearts = $state<HeroHeart[]>([]);
  let heroHeartSeq = 0;
  let heroAnimState = $state<"normal" | "hop" | "backflip" | "wiggle" | "wake" | "eating">("normal");
  let heroAnimTimeout: ReturnType<typeof setTimeout> | null = null;
  let heroWakeTimeout: ReturnType<typeof setTimeout> | null = null;
  let heroWakingUp = $state(false);

  let isSleepingHero = $derived.by(() => {
    if (!snap) return false;
    if (snap.companion.isEgg) return false;
    if (heroWakingUp) return false;
    return snap.companion.displayState === "sleep";
  });

  function triggerHeroPet(specificAnim?: "hop" | "backflip" | "wiggle" | "wake" | "eating") {
    if (heroAnimTimeout) clearTimeout(heroAnimTimeout);

    if (isSleepingHero && !specificAnim) {
      heroWakingUp = true;
      heroAnimState = "wake";
      spawnHeroHearts(["✨", "⭐", "❗"]);
      if (heroWakeTimeout) clearTimeout(heroWakeTimeout);
      heroWakeTimeout = setTimeout(() => {
        heroWakingUp = false;
        heroAnimState = "normal";
      }, 1500);
      return;
    }

    const rolls: Array<"hop" | "backflip" | "wiggle"> = ["hop", "backflip", "wiggle", "hop"];
    const chosen = specificAnim ?? rolls[Math.floor(Math.random() * rolls.length)];
    heroAnimState = chosen;
    const dur = chosen === "backflip" ? 750 : chosen === "eating" ? 900 : chosen === "wake" ? 1000 : 600;
    heroAnimTimeout = setTimeout(() => {
      heroAnimState = "normal";
    }, dur);

    const emojis = specificAnim === "eating" ? ["😋", "✨", "💖"] : ["❤️", "💖", "✨", "🥰", "⭐"];
    spawnHeroHearts(emojis);
  }

  function spawnHeroHearts(emojis: string[]) {
    const count = 3 + Math.floor(Math.random() * 3);
    for (let i = 0; i < count; i++) {
      const p: HeroHeart = {
        id: ++heroHeartSeq,
        emoji: emojis[Math.floor(Math.random() * emojis.length)],
        x: (Math.random() - 0.5) * 60,
        y: -10 - Math.random() * 30,
        scale: 0.8 + Math.random() * 0.5,
      };
      heroHearts = [...heroHearts, p];
      setTimeout(() => {
        heroHearts = heroHearts.filter((h) => h.id !== p.id);
      }, 1200);
    }
  }

  async function useCandy() {
    snap = await invoke<Snapshot>("use_rare_candy");
    triggerHeroPet("hop");
  }

  async function useMint() {
    snap = await invoke<Snapshot>("use_mint");
    triggerHeroPet("wiggle");
  }

  async function useBerry(kind: string) {
    try {
      snap = await invoke<Snapshot>("use_berry", { kind });
      triggerHeroPet("eating");
    } catch {
      // ignore
    }
  }

  async function minimizeWindow() {
    try {
      await invoke("minimize_window");
    } catch {
      try {
        await getCurrentWindow().minimize();
      } catch {
        // ignore
      }
    }
  }

  async function hideWindow() {
    try {
      await invoke("hide_window");
    } catch {
      try {
        await getCurrentWindow().hide();
      } catch {
        // ignore
      }
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
    if (e.button === 0 && !(e.target as HTMLElement).closest("button, select, input, .tab-btn, .window-btn")) {
      try {
        await getCurrentWindow().startDragging();
      } catch {
        // ignore
      }
    }
  }

  async function checkAutostart() {
    try {
      autostartActive = await isEnabled();
    } catch {
      autostartActive = false;
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
    } catch {
      // ignore
    }
  }

  function changeRefreshInterval(sec: number) {
    refreshIntervalSec = sec;
    try {
      localStorage.setItem("ptb_refresh_interval", String(sec));
    } catch {
      // ignore
    }
  }

  function setTheme(t: Theme) {
    currentTheme = t;
    try {
      localStorage.setItem("ptb_theme", t);
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
    return Math.round(n).toLocaleString("en-US");
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

  interface TypeInfo {
    name: string;
    text: string;
    bg: string;
    border: string;
  }

  const POKEMON_TYPES: Record<number, { primary: TypeInfo; secondary?: TypeInfo }> = {
    1: { primary: { name: "Grass", text: "#78C850", bg: "rgba(120,200,80,0.14)", border: "1px solid rgba(120,200,80,0.35)" }, secondary: { name: "Poison", text: "#A040A0", bg: "rgba(160,64,160,0.14)", border: "1px solid rgba(160,64,160,0.35)" } },
    2: { primary: { name: "Grass", text: "#78C850", bg: "rgba(120,200,80,0.14)", border: "1px solid rgba(120,200,80,0.35)" }, secondary: { name: "Poison", text: "#A040A0", bg: "rgba(160,64,160,0.14)", border: "1px solid rgba(160,64,160,0.35)" } },
    3: { primary: { name: "Grass", text: "#78C850", bg: "rgba(120,200,80,0.14)", border: "1px solid rgba(120,200,80,0.35)" }, secondary: { name: "Poison", text: "#A040A0", bg: "rgba(160,64,160,0.14)", border: "1px solid rgba(160,64,160,0.35)" } },
    4: { primary: { name: "Fire", text: "#F08030", bg: "rgba(240,128,48,0.14)", border: "1px solid rgba(240,128,48,0.35)" } },
    5: { primary: { name: "Fire", text: "#F08030", bg: "rgba(240,128,48,0.14)", border: "1px solid rgba(240,128,48,0.35)" } },
    6: { primary: { name: "Fire", text: "#F08030", bg: "rgba(240,128,48,0.14)", border: "1px solid rgba(240,128,48,0.35)" }, secondary: { name: "Flying", text: "#A890F0", bg: "rgba(168,144,240,0.14)", border: "1px solid rgba(168,144,240,0.35)" } },
    7: { primary: { name: "Water", text: "#6890F0", bg: "rgba(104,144,240,0.14)", border: "1px solid rgba(104,144,240,0.35)" } },
    8: { primary: { name: "Water", text: "#6890F0", bg: "rgba(104,144,240,0.14)", border: "1px solid rgba(104,144,240,0.35)" } },
    9: { primary: { name: "Water", text: "#6890F0", bg: "rgba(104,144,240,0.14)", border: "1px solid rgba(104,144,240,0.35)" } },
    25: { primary: { name: "Electric", text: "#F8D030", bg: "rgba(248,208,48,0.14)", border: "1px solid rgba(248,208,48,0.35)" } },
    26: { primary: { name: "Electric", text: "#F8D030", bg: "rgba(248,208,48,0.14)", border: "1px solid rgba(248,208,48,0.35)" } },
    133: { primary: { name: "Normal", text: "#A8A878", bg: "rgba(168,168,120,0.14)", border: "1px solid rgba(168,168,120,0.35)" } },
    134: { primary: { name: "Water", text: "#6890F0", bg: "rgba(104,144,240,0.14)", border: "1px solid rgba(104,144,240,0.35)" } },
    135: { primary: { name: "Electric", text: "#F8D030", bg: "rgba(248,208,48,0.14)", border: "1px solid rgba(248,208,48,0.35)" } },
    136: { primary: { name: "Fire", text: "#F08030", bg: "rgba(240,128,48,0.14)", border: "1px solid rgba(240,128,48,0.35)" } },
    150: { primary: { name: "Psychic", text: "#F85888", bg: "rgba(248,88,136,0.14)", border: "1px solid rgba(248,88,136,0.35)" } },
    151: { primary: { name: "Psychic", text: "#F85888", bg: "rgba(248,88,136,0.14)", border: "1px solid rgba(248,88,136,0.35)" } },
    220: { primary: { name: "Ice", text: "#8FE0EC", bg: "rgba(143,224,236,0.14)", border: "1px solid rgba(143,224,236,0.35)" }, secondary: { name: "Ground", text: "#D9B57B", bg: "rgba(217,181,123,0.14)", border: "1px solid rgba(217,181,123,0.35)" } },
    221: { primary: { name: "Ice", text: "#8FE0EC", bg: "rgba(143,224,236,0.14)", border: "1px solid rgba(143,224,236,0.35)" }, secondary: { name: "Ground", text: "#D9B57B", bg: "rgba(217,181,123,0.14)", border: "1px solid rgba(217,181,123,0.35)" } },
    473: { primary: { name: "Ice", text: "#8FE0EC", bg: "rgba(143,224,236,0.14)", border: "1px solid rgba(143,224,236,0.35)" }, secondary: { name: "Ground", text: "#D9B57B", bg: "rgba(217,181,123,0.14)", border: "1px solid rgba(217,181,123,0.35)" } },
  };

  function getTypes(id: number): { primary: TypeInfo; secondary?: TypeInfo; list: TypeInfo[] } {
    const found = POKEMON_TYPES[id];
    if (found) {
      return {
        primary: found.primary,
        secondary: found.secondary,
        list: found.secondary ? [found.primary, found.secondary] : [found.primary],
      };
    }
    const defaultType: TypeInfo = {
      name: "Normal",
      text: "#5B8CFF",
      bg: "rgba(91,140,255,0.14)",
      border: "1px solid rgba(91,140,255,0.35)",
    };
    return { primary: defaultType, list: [defaultType] };
  }

  function getStageInfo(stageText: string, isFinal: boolean): { stage: number; total: number } {
    const match = stageText.match(/(\d+)\s+of\s+(\d+)/i);
    if (match) {
      return { stage: parseInt(match[1], 10), total: parseInt(match[2], 10) };
    }
    return { stage: isFinal ? 3 : 1, total: 3 };
  }

  function getPaceLabel(tier: string): { label: string; isHot: boolean } {
    switch (tier.toLowerCase()) {
      case "blazing":
        return { label: "On Fire", isHot: true };
      case "fast":
        return { label: "Fast Pace", isHot: true };
      case "normal":
        return { label: "Steady", isHot: false };
      default:
        return { label: "Idle", isHot: false };
    }
  }

  const rarityLabel: Record<string, string> = {
    common: "Common",
    uncommon: "Uncommon",
    rare: "Rare",
    legendary: "Legendary",
    mythical: "Mythical",
  };

  const itemDisplay: Record<string, { name: string; desc: string; tileBg: string; tileBorder: string; iconColor: string }> = {
    oranBerry: {
      name: "Oran Berry",
      desc: "Sweet berry that feeds your buddy (+15M XP) and boosts happiness.",
      tileBg: "rgba(59,130,246,0.12)",
      tileBorder: "1px solid rgba(59,130,246,0.3)",
      iconColor: "#3B82F6",
    },
    sitrusBerry: {
      name: "Sitrus Berry",
      desc: "Premium golden berry (+50M XP) granting a sparkling golden aura.",
      tileBg: "rgba(245,158,11,0.12)",
      tileBorder: "1px solid rgba(245,158,11,0.3)",
      iconColor: "#F59E0B",
    },
    mint: {
      name: "Nature Mint",
      desc: "Rerolls your active buddy's growth nature and temperament.",
      tileBg: "rgba(57,217,138,0.12)",
      tileBorder: "1px solid rgba(57,217,138,0.3)",
      iconColor: "#39D98A",
    },
    rareCandy: {
      name: "Rare Candy",
      desc: "Instantly levels up and evolves your active companion (+100M XP).",
      tileBg: "rgba(255,98,89,0.12)",
      tileBorder: "1px solid rgba(255,98,89,0.3)",
      iconColor: "#FF6259",
    },
    egg_basic: {
      name: "Pokémon Egg (Basic)",
      desc: "Hatches a new Gen I–V Pokémon partner to raise with tokens.",
      tileBg: "rgba(255,255,255,0.05)",
      tileBorder: "1px solid rgba(255,255,255,0.12)",
      iconColor: "#E8B84B",
    },
    egg_uncommon: {
      name: "Pokémon Egg (Uncommon+)",
      desc: "Guarantees hatching an Uncommon, Rare, or Legendary species.",
      tileBg: "rgba(57,217,138,0.10)",
      tileBorder: "1.5px solid rgba(57,217,138,0.45)",
      iconColor: "#E8B84B",
    },
    egg_rare: {
      name: "Pokémon Egg (Rare+)",
      desc: "Premium egg with high odds for Rare, Legendary & Mythical Pokémon.",
      tileBg: "rgba(91,140,255,0.10)",
      tileBorder: "1.5px solid rgba(91,140,255,0.45)",
      iconColor: "#E8B84B",
    },
    shinyCharm: {
      name: "Shiny Charm",
      desc: "Permanently boosts the odds of hatching rare Shiny Pokémon ✨",
      tileBg: "rgba(232,184,75,0.16)",
      tileBorder: "1.5px solid rgba(232,184,75,0.5)",
      iconColor: "#E8B84B",
    },
  };

  const toolColors: Record<string, string> = {
    claude_code: "#5B8CFF",
    antigravity: "#39D98A",
    gemini: "#39D98A",
    codex: "#A890F0",
    grok: "#FF8A59",
    cursor: "#78C850",
    copilot: "#6890F0",
    opencode: "#F85888",
  };

  const navTabs: { id: Tab; label: string }[] = [
    { id: "buddy", label: "Buddy" },
    { id: "usage", label: "Usage" },
    { id: "shop", label: "Shop & Bag" },
    { id: "pokedex", label: "Pokédex" },
  ];
</script>

<div class="window-root theme-{currentTheme}" onmousedown={startDrag} role="application">
  <div class="app-card">

    <header class="app-header" data-tauri-drag-region>
      <div class="header-brand">
        <svg width="17" height="17" viewBox="0 0 24 24" class="pokeball-svg">
          <circle cx="12" cy="12" r="9.5" fill="#f2f2f4"></circle>
          <path d="M2.5 12a9.5 9.5 0 0 1 19 0Z" fill="#E3372E"></path>
          <rect x="2.5" y="11.1" width="19" height="1.8" fill="#1b1b1f"></rect>
          <circle cx="12" cy="12" r="3" fill="#1b1b1f"></circle>
          <circle cx="12" cy="12" r="1.5" fill="#f2f2f4"></circle>
        </svg>
        <span class="brand-title">PokéTokenBar</span>
        <span class="live-indicator" title="Connected"></span>
      </div>

      <div class="header-actions">
        <button
          class="window-btn"
          onclick={togglePet}
          title="Toggle Desktop Pet Widget"
          aria-label="Toggle Desktop Pet"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
            <ellipse cx="12" cy="16" rx="5.5" ry="4.3"></ellipse>
            <ellipse cx="5.2" cy="9.5" rx="2.1" ry="2.6" transform="rotate(-15 5.2 9.5)"></ellipse>
            <ellipse cx="9.6" cy="6.3" rx="2.1" ry="2.7" transform="rotate(-5 9.6 6.3)"></ellipse>
            <ellipse cx="14.4" cy="6.3" rx="2.1" ry="2.7" transform="rotate(5 14.4 6.3)"></ellipse>
            <ellipse cx="18.8" cy="9.5" rx="2.1" ry="2.6" transform="rotate(15 18.8 9.5)"></ellipse>
          </svg>
        </button>

        <button
          class="window-btn"
          class:active-action={showSettings}
          onclick={() => (showSettings = !showSettings)}
          title="Settings"
          aria-label="Settings"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
            <line x1="4" y1="6" x2="20" y2="6"></line>
            <circle cx="9" cy="6" r="1.8" fill="#0e1016" stroke="currentColor"></circle>
            <line x1="4" y1="12" x2="20" y2="12"></line>
            <circle cx="16" cy="12" r="1.8" fill="#0e1016" stroke="currentColor"></circle>
            <line x1="4" y1="18" x2="20" y2="18"></line>
            <circle cx="7" cy="18" r="1.8" fill="#0e1016" stroke="currentColor"></circle>
          </svg>
        </button>

        <button
          class="window-btn"
          onclick={refresh}
          disabled={loading}
          title="Refresh token usage"
          aria-label="Refresh"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" class:spinning={loading}>
            <path d="M4 12a8 8 0 0 1 13.66-5.66"></path>
            <path d="M20 12a8 8 0 0 1-13.66 5.66"></path>
            <path d="M18 3v4h-4"></path>
            <path d="M6 21v-4h4"></path>
          </svg>
        </button>

        <span class="header-divider"></span>

        <button class="window-btn" onclick={minimizeWindow} title="Minimize" aria-label="Minimize">
          <svg width="10" height="10" viewBox="0 0 12 12">
            <line x1="2" y1="9" x2="10" y2="9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"></line>
          </svg>
        </button>

        <button class="window-btn close-btn" onclick={hideWindow} title="Hide to system tray" aria-label="Close">
          <svg width="10" height="10" viewBox="0 0 12 12">
            <line x1="2.5" y1="2.5" x2="9.5" y2="9.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"></line>
            <line x1="9.5" y1="2.5" x2="2.5" y2="9.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"></line>
          </svg>
        </button>
      </div>
    </header>

    <div class="nav-bar" role="tablist">
      {#each navTabs as tab (tab.id)}
        <button
          class="tab-btn"
          class:active={currentTab === tab.id && !showSettings}
          onclick={() => {
            currentTab = tab.id;
            showSettings = false;
          }}
          role="tab"
          aria-selected={currentTab === tab.id && !showSettings}
        >
          {#if tab.id === "buddy"}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
              <ellipse cx="12" cy="16" rx="5.5" ry="4.3"></ellipse>
              <ellipse cx="5.2" cy="9.5" rx="2.1" ry="2.6" transform="rotate(-15 5.2 9.5)"></ellipse>
              <ellipse cx="9.6" cy="6.3" rx="2.1" ry="2.7" transform="rotate(-5 9.6 6.3)"></ellipse>
              <ellipse cx="14.4" cy="6.3" rx="2.1" ry="2.7" transform="rotate(5 14.4 6.3)"></ellipse>
              <ellipse cx="18.8" cy="9.5" rx="2.1" ry="2.6" transform="rotate(15 18.8 9.5)"></ellipse>
            </svg>
          {:else if tab.id === "usage"}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <line x1="5" y1="19" x2="5" y2="13"></line>
              <line x1="12" y1="19" x2="12" y2="8"></line>
              <line x1="19" y1="19" x2="19" y2="4"></line>
            </svg>
          {:else if tab.id === "shop"}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
              <path d="M9 9V7a3 3 0 0 1 6 0v2"></path>
              <rect x="4.5" y="9" width="15" height="11.5" rx="2.2"></rect>
            </svg>
          {:else if tab.id === "pokedex"}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
              <rect x="4" y="4.5" width="16" height="15" rx="3"></rect>
              <circle cx="8.2" cy="8.6" r="1.5"></circle>
              <line x1="12.5" y1="8.6" x2="17" y2="8.6"></line>
              <line x1="7" y1="14.5" x2="17" y2="14.5"></line>
              <line x1="7" y1="17.2" x2="13" y2="17.2"></line>
            </svg>
          {/if}
          <span>{tab.label}</span>
          {#if tab.id === "pokedex" && snap && snap.companion.dex.length > 0}
            <span class="tab-badge">{snap.companion.dex.length}</span>
          {/if}
        </button>
      {/each}
    </div>

    <!-- Scrollable Main Content Container -->
    <main class="content-scroll pk-scroll">

      <!-- Settings Overlay View -->
      {#if showSettings}
        <div class="settings-container">
          <div class="settings-header">
            <span class="settings-title">Preferences</span>
            <button class="back-btn" onclick={() => (showSettings = false)}>Done</button>
          </div>

          <div class="settings-card">
            <div class="setting-row">
              <div class="setting-text">
                <span class="setting-label">Launch at Login</span>
                <span class="setting-sub">Start PokeTokenBar on system boot</span>
              </div>
              <button class="toggle-switch" class:active={autostartActive} onclick={toggleAutostart} aria-label="Toggle launch at login">
                <span class="toggle-handle"></span>
              </button>
            </div>

            <div class="setting-row">
              <div class="setting-text">
                <span class="setting-label">Animated Sprites</span>
                <span class="setting-sub">Use Showdown animated GIF sprites</span>
              </div>
              <button class="toggle-switch" class:active={animatedSprites} onclick={toggleAnimatedSprites} aria-label="Toggle animated sprites">
                <span class="toggle-handle"></span>
              </button>
            </div>

            <div class="setting-row">
              <div class="setting-text">
                <span class="setting-label">Desktop Pet Widget</span>
                <span class="setting-sub">Floating Pokémon companion on screen</span>
              </div>
              <button class="action-btn" onclick={togglePet}>Toggle Pet</button>
            </div>

            <div class="setting-row">
              <div class="setting-text">
                <span class="setting-label">Poll Frequency</span>
                <span class="setting-sub">How often token logs are parsed</span>
              </div>
              <div class="pill-options">
                {#each [10, 30, 60, 120] as sec}
                  <button
                    class="pill-choice"
                    class:selected={refreshIntervalSec === sec}
                    onclick={() => changeRefreshInterval(sec)}
                  >
                    {sec}s
                  </button>
                {/each}
              </div>
            </div>

            <div class="setting-row theme-row">
              <div class="setting-text">
                <span class="setting-label">Color Theme</span>
                <span class="setting-sub">Customize app palette</span>
              </div>
              <div class="theme-grid">
                {#each [["midnight", "Midnight Glass"], ["oled", "OLED Dark"], ["cyberpunk", "Neon Cyber"], ["retro", "Game Boy"]] as [themeKey, label]}
                  <button
                    class="theme-choice theme-btn-{themeKey}"
                    class:selected={currentTheme === themeKey}
                    onclick={() => setTheme(themeKey as Theme)}
                  >
                    {label}
                  </button>
                {/each}
              </div>
            </div>
          </div>

          <div class="about-box">
            <div class="about-header">
              <span>PokéTokenBar</span>
              <span class="version-tag">v0.3.1</span>
            </div>
            <p class="about-sub">Pokémon companion for AI coding tokens on Windows & Linux.</p>
          </div>
        </div>

      {:else if snap}
        {@const c = snap.companion}
        {@const u = snap.usage}

        {#if (c.justEvolvedTo || c.justGraduated) && !celebrationDismissed}
          <div class="celebration-banner">
            <span class="celebration-icon">✨</span>
            <span class="celebration-text">
              {#if c.justEvolvedTo}
                Your partner evolved into <strong>{c.justEvolvedTo}</strong>!
              {:else}
                Your partner graduated into the Hall of Fame!
              {/if}
            </span>
            <button class="celebration-close" onclick={() => (celebrationDismissed = true)}>✕</button>
          </div>
        {/if}

        {#if currentTab === "buddy"}
          {@const pace = getPaceLabel(u.burnTier)}
          <div class="tab-pane">
            {#if c.isEgg}
              <div class="buddy-hero egg-mode">
                <div class="hero-header-row">
                  <span class="section-tag">INCUBATING EGG</span>
                </div>
                <div class="hero-body">
                  <div class="sprite-box">
                    <div class="egg-sprite">🥚</div>
                  </div>
                  <div class="hero-details">
                    <div class="name-row">
                      <span class="mon-name">Token Egg</span>
                      <span class="rarity-badge">Egg</span>
                    </div>
                    <div class="types-row">
                      <span class="type-pill egg-pill">🐣 Mystery Hatch</span>
                    </div>
                    <div class="stage-row">
                      <span class="stage-label">Incubation</span>
                      <span class="stage-pct">{(c.eggProgress * 100).toFixed(0)}%</span>
                    </div>
                  </div>
                </div>
                <div class="progress-container">
                  <div class="progress-track">
                    <div class="progress-fill egg-gradient" style="width: {(c.eggProgress * 100).toFixed(1)}%"></div>
                  </div>
                  <div class="progress-sub">{tokens(c.eggTokensToHatch)} tokens to hatch</div>
                </div>
              </div>
            {:else if c.hasActive && c.currentSpeciesId}
              {@const typeInfo = getTypes(c.currentSpeciesId)}
              {@const stageInfo = getStageInfo(c.stageText, c.isFinalStage)}
              {@const growthPct = Math.round(c.progress * 100)}

              <div
                class="buddy-hero"
                class:sleeping-hero={isSleepingHero}
                class:golden-hero={c.hasGoldenAura}
              >
                <div class="hero-glow" style="background: radial-gradient(circle, {typeInfo.primary.bg.replace('0.14', '0.22')}, transparent 70%);"></div>
                <div class="hero-header-row">
                  <div class="hero-tag-group">
                    <span class="section-tag">ACTIVE BUDDY</span>
                    {#if isSleepingHero}
                      <span class="hero-sleep-pill">💤 Sleeping</span>
                    {:else if c.hasGoldenAura}
                      <span class="hero-gold-pill">✨ Sitrus Sparkle</span>
                    {/if}
                  </div>
                  <div class="stage-pips">
                    {#each Array(stageInfo.total) as _, i}
                      <span class="pip" class:filled={i < stageInfo.stage}></span>
                    {/each}
                  </div>
                </div>
                <div class="hero-body">
                  <div
                    class="sprite-box interactive-hero-box"
                    onclick={() => triggerHeroPet()}
                    role="button"
                    tabindex="0"
                    onkeydown={(e) => e.key === 'Enter' && triggerHeroPet()}
                    title="Click or pet your companion!"
                  >
                    <img
                      class="hero-sprite"
                      class:bounce={u.burnTier === "fast" || u.burnTier === "blazing"}
                      class:hop={heroAnimState === "hop"}
                      class:backflip={heroAnimState === "backflip"}
                      class:wiggle={heroAnimState === "wiggle"}
                      class:wake-up={heroAnimState === "wake"}
                      class:eating={heroAnimState === "eating"}
                      class:sleeping-mon={isSleepingHero}
                      src={spriteUrl(c.currentSpeciesId, c.isShiny)}
                      alt={c.displayName}
                      onerror={(e) => fallbackStaticSprite(e, c.currentSpeciesId ?? 1, c.isShiny)}
                    />

                    {#if isSleepingHero}
                      <div class="hero-zzz-box">
                        <span class="hero-zzz hero-zzz-1">z</span>
                        <span class="hero-zzz hero-zzz-2">Z</span>
                        <span class="hero-zzz hero-zzz-3">💤</span>
                      </div>
                    {/if}

                    {#if c.hasGoldenAura}
                      <div class="hero-gold-sparkles">
                        <span class="hero-sp sp-1">✨</span>
                        <span class="hero-sp sp-2">⭐</span>
                      </div>
                    {/if}

                    {#each heroHearts as h (h.id)}
                      <span
                        class="floating-heart hero-floating-heart"
                        style="--target-x: {h.x}px; --target-y: {h.y}px; --scale: {h.scale};"
                      >
                        {h.emoji}
                      </span>
                    {/each}
                  </div>
                  <div class="hero-details">
                    <div class="name-row">
                      <span class="mon-name">
                        {c.displayName}
                        {#if c.isShiny}<span class="shiny-star" title="Shiny!">✨</span>{/if}
                      </span>
                      <span class="rarity-badge">{rarityLabel[c.rarity ?? ""] ?? "Common"}</span>
                    </div>
                    <div class="types-row">
                      {#each typeInfo.list as t}
                        <span class="type-pill" style="background: {t.bg}; border: {t.border}; color: {t.text};">
                          {t.name}
                        </span>
                      {/each}
                    </div>
                    <div class="stage-row">
                      <span class="stage-label">Stage {stageInfo.stage}/{stageInfo.total}</span>
                      <span class="stage-pct">{growthPct}%</span>
                    </div>
                  </div>
                </div>
                <div class="progress-container">
                  <div class="progress-track">
                    <div
                      class="progress-fill"
                      style="background: linear-gradient(90deg, {typeInfo.primary.text}, {typeInfo.secondary?.text ?? typeInfo.primary.text}); width: {growthPct}%;"
                    ></div>
                  </div>
                  <div class="progress-sub">{tokens(c.tokensToNext)} tokens to next stage</div>
                </div>
              </div>
            {/if}

            <div class="metrics-grid">
              <div class="metric-card">
                <svg width="15" height="15" viewBox="0 0 24 24" class="metric-icon">
                  <circle cx="12" cy="12" r="9.2" fill="#E8B84B" stroke="#9C6B1F" stroke-width="1.1"></circle>
                  <circle cx="12" cy="12" r="5.6" fill="none" stroke="#9C6B1F" stroke-width="1" opacity=".55"></circle>
                  <ellipse cx="9" cy="8.3" rx="2.3" ry="1.3" fill="#fff" opacity=".3"></ellipse>
                </svg>
                <span class="metric-val">{compact(u.todayTotalTokens)}</span>
                <span class="metric-lbl">TOKENS TODAY</span>
              </div>
              <div class="metric-card">
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="#8B93A7" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" class="metric-icon">
                  <path d="M12 5v14"></path>
                  <path d="M16 8.5c0-1.7-1.8-2.8-4-2.8s-4 1-4 2.5c0 1.6 1.3 2.2 4 2.8s4 1.2 4 2.8c0 1.6-1.8 2.6-4 2.6s-4-1-4-2.6"></path>
                </svg>
                <span class="metric-val">${u.todayCostTotal.toFixed(2)}</span>
                <span class="metric-lbl">COST TODAY</span>
              </div>
              <div class="metric-card pace-card" class:hot={pace.isHot}>
                <svg width="15" height="15" viewBox="0 0 24 24" fill={pace.isHot ? "#FF8A59" : "#8B93A7"} class="metric-icon">
                  <path d="M12.5 2.3c.9 2.8-1.8 4-2.6 6.4-.5 1.6.1 2.9 1.1 3.6.2-1 .9-1.8 1.6-2.3.1 1 .5 1.7 1.1 2.3a3.6 3.6 0 0 1 1.1 2.6 4.3 4.3 0 0 1-8.6 0c0-3.9 2.9-6.5 6.3-12.6Z"></path>
                </svg>
                <span class="metric-val" style="color: {pace.isHot ? '#FF8A59' : '#F2F3F5'};">{pace.label}</span>
                <span class="metric-lbl">CURRENT PACE</span>
              </div>
            </div>

            <div class="quick-items-section">
              <span class="section-tag">QUICK ITEMS & BERRIES</span>
              <div class="quick-grid">
                <button
                  class="quick-btn berry-btn"
                  disabled={!c.hasActive || c.isEgg || (c.ownedItems.find(([k]) => k === 'oranBerry')?.[1] ?? 0) === 0}
                  onclick={() => useBerry('oranBerry')}
                  title="Feed Oran Berry (+15M XP)"
                >
                  <span class="btn-emoji">🫐</span>
                  <span>Feed Oran ({c.ownedItems.find(([k]) => k === 'oranBerry')?.[1] ?? 0})</span>
                </button>
                <button
                  class="quick-btn berry-btn gold"
                  disabled={!c.hasActive || c.isEgg || (c.ownedItems.find(([k]) => k === 'sitrusBerry')?.[1] ?? 0) === 0}
                  onclick={() => useBerry('sitrusBerry')}
                  title="Feed Sitrus Berry (+50M XP + Golden Aura)"
                >
                  <span class="btn-emoji">🍊</span>
                  <span>Feed Sitrus ({c.ownedItems.find(([k]) => k === 'sitrusBerry')?.[1] ?? 0})</span>
                </button>
                <button
                  class="quick-btn"
                  disabled={!c.hasActive || c.isEgg || (c.ownedItems.find(([k]) => k === 'rareCandy')?.[1] ?? 0) === 0}
                  onclick={useCandy}
                  title="Use Rare Candy (+100M XP)"
                >
                  <span class="btn-emoji">🍬</span>
                  <span>Rare Candy ({c.ownedItems.find(([k]) => k === 'rareCandy')?.[1] ?? 0})</span>
                </button>
                <button
                  class="quick-btn"
                  disabled={!c.hasActive || c.isEgg || (c.ownedItems.find(([k]) => k === 'mint')?.[1] ?? 0) === 0}
                  onclick={useMint}
                  title="Use Nature Mint (Rerolls nature)"
                >
                  <span class="btn-emoji">🌿</span>
                  <span>Nature Mint ({c.ownedItems.find(([k]) => k === 'mint')?.[1] ?? 0})</span>
                </button>
              </div>
            </div>
          </div>
        {/if}

        {#if currentTab === "usage"}
          <div class="tab-pane">
            <div class="metrics-grid">
              <div class="metric-card">
                <span class="metric-val">{compact(u.todayTotalTokens)}</span>
                <span class="metric-lbl">TODAY</span>
              </div>
              <div class="metric-card">
                <span class="metric-val">{compact(u.weekTotalTokens)}</span>
                <span class="metric-lbl">THIS WEEK</span>
              </div>
              <div class="metric-card">
                <span class="metric-val">{compact(u.monthTotalTokens)}</span>
                <span class="metric-lbl">THIS MONTH</span>
              </div>
            </div>

            <div class="panel-card">
              <div class="panel-title-red">Active AI Tools Today</div>
              {#if u.snapshots.length > 0}
                {@const totalToday = u.snapshots.reduce((acc, s) => acc + s.todayTotalTokens, 0) || 1}
                <div class="tool-bar">
                  {#each u.snapshots as s}
                    {#if s.todayTotalTokens > 0}
                      <div
                        class="tool-bar-seg"
                        style="width: {(s.todayTotalTokens / totalToday) * 100}%; background: {toolColors[s.id] ?? '#5B8CFF'};"
                      ></div>
                    {/if}
                  {/each}
                </div>
                <div class="tools-list">
                  {#each u.snapshots as s}
                    <div class="tool-row">
                      <div class="tool-name-group">
                        <span class="tool-dot" style="background: {toolColors[s.id] ?? '#5B8CFF'};"></span>
                        <span class="tool-name">{s.displayName}</span>
                      </div>
                      <div class="tool-val-group">
                        <span class="tool-val">{tokens(s.todayTotalTokens)}</span>
                        <span class="tool-sub">today</span>
                      </div>
                    </div>
                  {/each}
                </div>
              {:else}
                <p class="empty-hint">No active AI tools detected yet today.</p>
              {/if}
            </div>

            {#if u.limits}
              <div class="panel-card">
                <div class="limits-header">
                  <span class="panel-title-red">Claude API Limits</span>
                  {#if u.limits.planDisplay}
                    <span class="plan-pill">{u.limits.planDisplay}</span>
                  {/if}
                </div>
                {#if u.limits.fiveHour}
                  {@const fh = u.limits.fiveHour}
                  <div class="limit-block">
                    <div class="limit-labels">
                      <span>5-Hour Session Window</span>
                      <span class="limit-pct">{fh.utilization !== null && fh.utilization !== undefined ? formatUtilization(fh.utilization) : "—"}</span>
                    </div>
                    <div class="limit-track">
                      <div class="limit-fill" style="width: {fh.utilization !== null && fh.utilization !== undefined ? getUtilizationPercent(fh.utilization) : 0}%;"></div>
                    </div>
                  </div>
                {/if}
                {#if u.limits.sevenDay}
                  {@const sd = u.limits.sevenDay}
                  <div class="limit-block">
                    <div class="limit-labels">
                      <span>7-Day Weekly Window</span>
                      <span class="limit-pct">{sd.utilization !== null && sd.utilization !== undefined ? formatUtilization(sd.utilization) : "—"}</span>
                    </div>
                    <div class="limit-track">
                      <div class="limit-fill" style="width: {sd.utilization !== null && sd.utilization !== undefined ? getUtilizationPercent(sd.utilization) : 0}%;"></div>
                    </div>
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/if}

        {#if currentTab === "shop"}
          <div class="tab-pane">
            <div class="wallet-card">
              <svg width="120" height="120" viewBox="0 0 24 24" class="wallet-watermark">
                <circle cx="12" cy="12" r="9.5" fill="none" stroke="#fff" stroke-width="1.4"></circle>
                <path d="M2.5 12h19" stroke="#fff" stroke-width="1.4"></path>
                <circle cx="12" cy="12" r="3" fill="none" stroke="#fff" stroke-width="1.4"></circle>
              </svg>
              <div class="wallet-content">
                <svg width="30" height="30" viewBox="0 0 24 24" class="wallet-coin">
                  <circle cx="12" cy="12" r="9.2" fill="#E8B84B" stroke="#9C6B1F" stroke-width="1.1"></circle>
                  <circle cx="12" cy="12" r="5.6" fill="none" stroke="#9C6B1F" stroke-width="1" opacity=".55"></circle>
                  <ellipse cx="9" cy="8.3" rx="2.3" ry="1.3" fill="#fff" opacity=".3"></ellipse>
                </svg>
                <div class="wallet-text">
                  <span class="wallet-amount">{tokens(c.availableTokens)}</span>
                  <span class="wallet-label">TOKENS AVAILABLE TO SPEND</span>
                </div>
              </div>
            </div>

            <div class="panel-card">
              <div class="bag-header">
                <span class="panel-title">My Bag</span>
                {#if c.ownedItems.length > 0}
                  <span class="bag-count-badge">{c.ownedItems.reduce((a, [_, n]) => a + n, 0)} items</span>
                {/if}
              </div>
              {#if c.ownedItems.length > 0}
                <div class="bag-list">
                  {#each c.ownedItems as [kind, count]}
                    {@const disp = itemDisplay[kind] ?? { name: kind, desc: "A training item.", tileBg: "rgba(255,255,255,0.05)", tileBorder: "1px solid rgba(255,255,255,0.1)", iconColor: "#E8B84B" }}
                    <div class="bag-item-row">
                      <div class="item-tile" style="background: {disp.tileBg}; border: {disp.tileBorder};">
                        {#if kind === "oranBerry"}
                          <span class="item-tile-emoji">🫐</span>
                        {:else if kind === "sitrusBerry"}
                          <span class="item-tile-emoji">🍊</span>
                        {:else if kind === "mint"}
                          <svg width="14" height="14" viewBox="0 0 24 24" fill={disp.iconColor}><path d="M20 4C11 4 4 10 4 17c0 1.5 1 2.5 2.2 2.8C7 13 12 8 20 4Z"></path></svg>
                        {:else if kind === "rareCandy"}
                          <svg width="14" height="14" viewBox="0 0 24 24" fill={disp.iconColor}><path d="M9 9 4 6v12l5-3Z"></path><path d="M15 9l5-3v12l-5-3Z"></path><rect x="9" y="9" width="6" height="6" rx="1.5"></rect></svg>
                        {:else if kind.startsWith("egg")}
                          <svg width="14" height="14" viewBox="0 0 24 24" fill={disp.iconColor}><path d="M12 2C7.5 8.5 5 13 5 16.5A7 7 0 0 0 19 16.5C19 13 16.5 8.5 12 2Z"></path></svg>
                        {:else}
                          <svg width="14" height="14" viewBox="0 0 24 24" fill={disp.iconColor}><path d="M12 2.5c.3 3.2 1 5.6 2.1 7.1 1.5 1.1 3.9 1.8 7.1 2.1-3.2.3-5.6 1-7.1 2.1-1.1 1.5-1.8 3.9-2.1 7.1-.3-3.2-1-5.6-2.1-7.1-1.5-1.1-3.9-1.8-7.1-2.1 3.2-.3 5.6-1 7.1-2.1 1.1-1.5 1.8-3.9 2.1-7.1Z"></path></svg>
                        {/if}
                      </div>
                      <div class="bag-item-info">
                        <div class="bag-item-top">
                          <span class="bag-item-name">{disp.name}</span>
                          <span class="bag-item-qty">×{count}</span>
                        </div>
                        <span class="item-desc-text">{disp.desc}</span>
                      </div>
                      <div class="bag-item-action">
                        {#if kind === "oranBerry" || kind === "sitrusBerry"}
                          <button
                            class="bag-use-btn berry-feed-btn"
                            disabled={!c.hasActive || c.isEgg}
                            onclick={() => useBerry(kind)}
                          >
                            Feed
                          </button>
                        {:else if kind === "rareCandy"}
                          <button
                            class="bag-use-btn"
                            disabled={!c.hasActive || c.isEgg}
                            onclick={useCandy}
                          >
                            Use
                          </button>
                        {:else if kind === "mint"}
                          <button
                            class="bag-use-btn"
                            disabled={!c.hasActive || c.isEgg}
                            onclick={useMint}
                          >
                            Use
                          </button>
                        {:else if kind === "shinyCharm"}
                          <span class="bag-held-badge">Active ✨</span>
                        {/if}
                      </div>
                    </div>
                  {/each}
                </div>
              {:else}
                <div class="empty-bag-state">
                  <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="#3C4152" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9 9V7a3 3 0 0 1 6 0v2"></path><rect x="4.5" y="9" width="15" height="11.5" rx="2.2"></rect></svg>
                  <span class="empty-bag-text">Your bag is empty. Buy items below!</span>
                </div>
              {/if}
            </div>

            <div class="panel-card">
              <div class="shop-header">
                <span class="panel-title">PokéShop</span>
                <span class="shop-sub">Spend tokens to grow your buddy</span>
              </div>
              <div class="shop-items-list">
                {#each c.shop as item (item.kind + (item.tier ?? ""))}
                  {@const itemKey = item.kind === "egg" ? `egg_${item.tier ?? "basic"}` : item.kind}
                  {@const disp = itemDisplay[itemKey] ?? { name: item.kind, desc: "A training item.", tileBg: "rgba(255,255,255,0.05)", tileBorder: "1px solid rgba(255,255,255,0.1)", iconColor: "#E8B84B" }}
                  <div class="shop-item-row">
                    <div class="item-tile" style="background: {disp.tileBg}; border: {disp.tileBorder};">
                      {#if item.kind === "oranBerry"}
                        <span class="item-tile-emoji">🫐</span>
                      {:else if item.kind === "sitrusBerry"}
                        <span class="item-tile-emoji">🍊</span>
                      {:else if item.kind === "mint"}
                        <svg width="16" height="16" viewBox="0 0 24 24" fill={disp.iconColor}><path d="M20 4C11 4 4 10 4 17c0 1.5 1 2.5 2.2 2.8C7 13 12 8 20 4Z"></path></svg>
                      {:else if item.kind === "rareCandy"}
                        <svg width="16" height="16" viewBox="0 0 24 24" fill={disp.iconColor}><path d="M9 9 4 6v12l5-3Z"></path><path d="M15 9l5-3v12l-5-3Z"></path><rect x="9" y="9" width="6" height="6" rx="1.5"></rect></svg>
                      {:else if item.kind === "egg"}
                        <svg width="17" height="17" viewBox="0 0 24 24" fill={disp.iconColor}><path d="M12 2C7.5 8.5 5 13 5 16.5A7 7 0 0 0 19 16.5C19 13 16.5 8.5 12 2Z"></path></svg>
                      {:else}
                        <svg width="16" height="16" viewBox="0 0 24 24" fill={disp.iconColor}><path d="M12 2.5c.3 3.2 1 5.6 2.1 7.1 1.5 1.1 3.9 1.8 7.1 2.1-3.2.3-5.6 1-7.1 2.1-1.1 1.5-1.8 3.9-2.1 7.1-.3-3.2-1-5.6-2.1-7.1-1.5-1.1-3.9-1.8-7.1-2.1 3.2-.3 5.6-1 7.1-2.1 1.1-1.5 1.8-3.9 2.1-7.1Z"></path></svg>
                      {/if}
                    </div>
                    <div class="shop-item-info">
                      <span class="shop-item-name">{disp.name}</span>
                      <span class="item-desc-text">{disp.desc}</span>
                      <div class="shop-price-row">
                        <svg width="11" height="11" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9.2" fill="#E8B84B" stroke="#9C6B1F" stroke-width="1.3"></circle></svg>
                        <span class="shop-price-val">{tokens(item.price)}</span>
                        <span class="shop-price-unit">tokens</span>
                      </div>
                    </div>
                    <button
                      class="buy-action-btn"
                      class:pk-shake={shakeItemId === itemKey}
                      onclick={() => (item.kind === "egg" ? buyEgg(item.tier, item.price, item.canBuy) : buy(item.kind, item.price, item.canBuy))}
                    >
                      Buy
                    </button>
                  </div>
                {/each}
              </div>
            </div>
          </div>
        {/if}

        {#if currentTab === "pokedex"}
          <div class="tab-pane">
            <div class="pokedex-header-row">
              <span class="panel-title">Discovered Pokémon</span>
              <span class="dex-counter">{c.dex.length} Collected</span>
            </div>
            <div class="pokedex-2col-grid">
              {#each c.dex as d (d.id)}
                {@const typeInfo = getTypes(d.id)}
                <button
                  class="pokedex-card clickable-card"
                  onclick={() => openDexDetails(d)}
                  type="button"
                  title="Click to view Pokédex entry for {d.name}"
                >
                  <div class="card-top-stripe" style="background: {typeInfo.primary.text};"></div>
                  {#if d.isRaising}
                    <span class="active-tag">ACTIVE</span>
                  {/if}
                  <div class="dex-sprite-container">
                    <img
                      class="dex-sprite-img"
                      src={spriteUrl(d.id, d.isShiny, true)}
                      alt={d.name}
                      onerror={(e) => fallbackStaticSprite(e, d.id, d.isShiny)}
                      loading="lazy"
                    />
                  </div>
                  <span class="dex-species-name">
                    {d.name}
                    {#if d.isShiny}<span class="shiny-star">✨</span>{/if}
                  </span>
                  <div class="dex-types-list">
                    {#each typeInfo.list as t}
                      <span class="dex-type-pill" style="background: {t.bg}; border: {t.border}; color: {t.text};">
                        {t.name}
                      </span>
                    {/each}
                  </div>
                  <div class="dex-view-hint">
                    <span>View Entry ➔</span>
                  </div>
                </button>
              {/each}
              {#each Array(Math.max(4, 8 - c.dex.length)) as _, i}
                <div class="locked-slot-card">
                  <div class="locked-silhouette-box">
                    <span class="locked-question">?</span>
                  </div>
                  <span class="locked-label">???</span>
                </div>
              {/each}
            </div>
          </div>
        {/if}

      {:else if !loading}
        <div class="empty-state-loading">
          <div class="loading-spinner"></div>
          <p class="loading-text">Connecting to token providers…</p>
        </div>
      {/if}
    </main>

    <!-- Pokédex Entry Detail Modal Dialog -->
    {#if selectedDexMon}
      {@const mon = selectedDexMon}
      {@const typeInfo = getTypes(mon.id)}
      <div
        class="dex-modal-backdrop"
        onclick={() => (selectedDexMon = null)}
        onkeydown={(e) => e.key === "Escape" && (selectedDexMon = null)}
        role="dialog"
        tabindex="-1"
        aria-modal="true"
      >
        <div class="dex-modal-card" onclick={(e) => e.stopPropagation()} role="document">
          <button class="dex-modal-close" onclick={() => (selectedDexMon = null)} aria-label="Close Pokédex entry">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>
          </button>

          <div class="dex-modal-hero">
            <div class="dex-modal-glow" style="background: radial-gradient(circle, {typeInfo.primary.text}40 0%, transparent 70%);"></div>
            <img
              class="dex-modal-sprite"
              src={spriteUrl(mon.id, mon.isShiny, true)}
              alt={mon.name}
              onerror={(e) => fallbackStaticSprite(e, mon.id, mon.isShiny)}
            />
            {#if mon.isShiny}
              <span class="dex-modal-shiny-pill">✨ Shiny</span>
            {/if}
          </div>

          <div class="dex-modal-header">
            <div class="dex-modal-id">#{String(mon.id).padStart(3, "0")}</div>
            <div class="dex-modal-name">
              {dexDetails?.name ?? mon.name}
            </div>
            <div class="dex-modal-genus">
              {dexDetails?.genus ?? (typeInfo.primary.name + " Pokémon")}
            </div>
            <div class="dex-modal-types">
              {#each typeInfo.list as t}
                <span class="dex-type-pill" style="background: {t.bg}; border: {t.border}; color: {t.text};">
                  {t.name}
                </span>
              {/each}
            </div>
          </div>

          <div class="dex-modal-body">
            {#if dexDetailsLoading}
              <div class="dex-modal-loading">
                <div class="loading-spinner"></div>
                <span class="loading-dex-text">Consulting Pokédex…</span>
              </div>
            {:else if dexDetails}
              <div class="dex-flavor-card">
                <div class="dex-flavor-quote">“</div>
                <p class="dex-flavor-text">{dexDetails.flavorText}</p>
              </div>

              <div class="dex-stats-grid">
                <div class="dex-stat-item">
                  <span class="dex-stat-label">HEIGHT</span>
                  <span class="dex-stat-value">{dexDetails.heightM > 0 ? dexDetails.heightM.toFixed(1) + " m" : "--"}</span>
                </div>
                <div class="dex-stat-item">
                  <span class="dex-stat-label">WEIGHT</span>
                  <span class="dex-stat-value">{dexDetails.weightKg > 0 ? dexDetails.weightKg.toFixed(1) + " kg" : "--"}</span>
                </div>
                <div class="dex-stat-item">
                  <span class="dex-stat-label">CATEGORY</span>
                  <span class="dex-stat-value">{dexDetails.isLegendary ? "Legendary" : dexDetails.isMythical ? "Mythical" : "Standard"}</span>
                </div>
              </div>
            {:else}
              <div class="dex-flavor-card">
                <div class="dex-flavor-quote">“</div>
                <p class="dex-flavor-text">A loyal Pokémon companion raised with your AI coding tokens.</p>
              </div>
            {/if}
          </div>
        </div>
      </div>
    {/if}

    <div class="bottom-vignette"></div>
  </div>
</div>

<style>
  :global(*) {
    box-sizing: border-box;
    user-select: none;
  }

  :global(html, body) {
    margin: 0;
    padding: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent !important;
    background-color: transparent !important;
    font-family: "Space Grotesk", sans-serif;
    -webkit-font-smoothing: antialiased;
  }

  @keyframes pk-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.35; }
  }
  @keyframes pk-spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
  @keyframes pk-shake {
    0%, 100% { transform: translateX(0); }
    25% { transform: translateX(-4px); }
    75% { transform: translateX(4px); }
  }

  .theme-midnight {
    --bg-grad: linear-gradient(180deg, #111319, #0b0d12);
    --border-color: rgba(255, 255, 255, 0.08);
    --card-bg: rgba(255, 255, 255, 0.03);
    --accent-red: #E3372E;
    --accent-grad: linear-gradient(135deg, #E3372E, #B4241C);
  }
  .theme-oled {
    --bg-grad: linear-gradient(180deg, #070707, #000000);
    --border-color: rgba(255, 255, 255, 0.12);
    --card-bg: rgba(255, 255, 255, 0.04);
    --accent-red: #ff3333;
    --accent-grad: linear-gradient(135deg, #ff3333, #aa0000);
  }
  .theme-cyberpunk {
    --bg-grad: linear-gradient(180deg, #0d0c1d, #05050f);
    --border-color: rgba(0, 240, 255, 0.2);
    --card-bg: rgba(0, 240, 255, 0.04);
    --accent-red: #ff0055;
    --accent-grad: linear-gradient(135deg, #ff0055, #7a00ff);
  }
  .theme-retro {
    --bg-grad: linear-gradient(180deg, #1f2d1f, #0f1c0f);
    --border-color: rgba(155, 187, 89, 0.25);
    --card-bg: rgba(155, 187, 89, 0.06);
    --accent-red: #9bbb59;
    --accent-grad: linear-gradient(135deg, #9bbb59, #4f7324);
  }

  .window-root {
    width: 100vw;
    height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    overflow: hidden;
  }

  .app-card {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--bg-grad);
    border: 1px solid var(--border-color);
    border-radius: 16px;
    overflow: hidden;
    box-shadow: 0 30px 70px -20px rgba(0, 0, 0, 0.65);
    position: relative;
  }

  .app-header {
    flex-shrink: 0;
    height: 44px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 8px 0 14px;
    background: rgba(255, 255, 255, 0.02);
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
    cursor: grab;
  }
  .app-header:active {
    cursor: grabbing;
  }

  .header-brand {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .pokeball-svg {
    flex-shrink: 0;
  }

  .brand-title {
    font-size: 13px;
    font-weight: 600;
    color: #F2F3F5;
    letter-spacing: 0.01em;
    white-space: nowrap;
  }

  .live-indicator {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #39D98A;
    flex-shrink: 0;
    animation: pk-pulse 2s ease-in-out infinite;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .window-btn {
    width: 26px;
    height: 26px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 6px;
    color: #8B93A7;
    background: transparent;
    border: none;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .window-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #ffffff;
  }
  .window-btn.active-action {
    background: rgba(255, 255, 255, 0.12);
    color: #ffffff;
  }
  .window-btn.close-btn:hover {
    background: #E5484D;
    color: #ffffff;
  }

  .header-divider {
    width: 1px;
    height: 16px;
    background: rgba(255, 255, 255, 0.1);
    margin: 0 4px;
  }

  .spinning {
    animation: pk-spin 0.6s linear infinite;
  }

  .nav-bar {
    flex-shrink: 0;
    height: 46px;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 8px;
    background: rgba(255, 255, 255, 0.015);
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
  }

  .tab-btn {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    padding: 8px 4px;
    border-radius: 9px;
    cursor: pointer;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    transition: background 0.15s, color 0.15s;
    background: transparent;
    color: #8B93A7;
    border: none;
  }
  .tab-btn:hover {
    background: rgba(255, 255, 255, 0.06);
    color: #ffffff;
  }
  .tab-btn.active {
    background: var(--accent-grad);
    color: #ffffff;
    box-shadow: 0 4px 12px rgba(227, 55, 45, 0.35);
  }

  .tab-badge {
    padding: 1px 5px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.22);
    color: #ffffff;
    font-size: 9px;
    font-weight: 700;
    font-family: "JetBrains Mono", monospace;
  }

  .content-scroll {
    position: relative;
    flex: 1;
    overflow-y: auto;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .pk-scroll::-webkit-scrollbar {
    width: 6px;
  }
  .pk-scroll::-webkit-scrollbar-track {
    background: transparent;
  }
  .pk-scroll::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.14);
    border-radius: 999px;
  }
  .pk-scroll::-webkit-scrollbar-thumb:hover {
    background: rgba(255, 255, 255, 0.22);
  }

  .bottom-vignette {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 20px;
    background: linear-gradient(180deg, rgba(11, 13, 18, 0), #0b0d12);
    pointer-events: none;
  }

  .tab-pane {
    display: flex;
    flex-direction: column;
    gap: 12px;
    animation: fadeIn 0.2s ease;
  }

  @keyframes fadeIn {
    from { opacity: 0; transform: translateY(3px); }
    to { opacity: 1; transform: translateY(0); }
  }

  .section-tag {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: #5B6274;
  }

  .buddy-hero {
    position: relative;
    overflow: hidden;
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.035), rgba(255, 255, 255, 0.015));
    border: 1px solid rgba(255, 255, 255, 0.07);
    border-radius: 16px;
    padding: 16px;
  }

  .hero-glow {
    position: absolute;
    top: -50px;
    right: -50px;
    width: 170px;
    height: 170px;
    border-radius: 50%;
    pointer-events: none;
  }

  .hero-header-row {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
  }

  .stage-pips {
    display: flex;
    gap: 4px;
  }
  .pip {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.15);
  }
  .pip.filled {
    background: #E3372E;
  }

  .hero-body {
    position: relative;
    display: flex;
    gap: 14px;
    align-items: center;
  }

  .sprite-box {
    width: 84px;
    height: 84px;
    border-radius: 14px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.06);
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }

  .hero-sprite {
    max-width: 76px;
    max-height: 76px;
    width: auto;
    height: auto;
    object-fit: contain;
    image-rendering: pixelated;
    filter: drop-shadow(0 4px 10px rgba(0, 0, 0, 0.45));
  }

  .egg-sprite {
    font-size: 46px;
    filter: drop-shadow(0 4px 10px rgba(0, 0, 0, 0.45));
    animation: pk-shake 3s infinite ease-in-out;
  }

  .hero-details {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .name-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .mon-name {
    font-size: 19px;
    font-weight: 700;
    color: #F2F3F5;
  }
  .shiny-star {
    font-size: 14px;
    margin-left: 2px;
  }

  .rarity-badge {
    padding: 2px 8px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: #8B93A7;
    font-size: 9.5px;
    font-weight: 600;
    letter-spacing: 0.03em;
  }

  .types-row {
    display: flex;
    gap: 6px;
  }
  .type-pill {
    padding: 3px 9px;
    border-radius: 999px;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.02em;
  }
  .egg-pill {
    background: rgba(232, 184, 75, 0.14);
    border: 1px solid rgba(232, 184, 75, 0.35);
    color: #E8B84B;
  }

  .stage-row {
    display: flex;
    justify-content: space-between;
    margin-top: 2px;
  }
  .stage-label {
    font-size: 11.5px;
    font-weight: 600;
    color: #8B93A7;
  }
  .stage-pct {
    font-size: 11.5px;
    font-weight: 700;
    color: #F2F3F5;
    font-family: "JetBrains Mono", monospace;
  }

  .progress-container {
    position: relative;
    margin-top: 12px;
  }
  .progress-track {
    height: 8px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.08);
    overflow: hidden;
  }
  .progress-fill {
    height: 100%;
    border-radius: 999px;
    transition: width 0.3s ease;
  }
  .egg-gradient {
    background: linear-gradient(90deg, #E8B84B, #FF8A59);
  }
  .progress-sub {
    margin-top: 6px;
    font-size: 10.5px;
    font-family: "JetBrains Mono", monospace;
    color: #5B6274;
  }

  .metrics-grid {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 10px;
  }

  .metric-card {
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.07);
    border-radius: 12px;
    padding: 12px 10px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .metric-icon {
    flex-shrink: 0;
  }
  .metric-val {
    font-size: 16px;
    font-weight: 700;
    color: #F2F3F5;
    font-family: "JetBrains Mono", monospace;
  }
  .metric-lbl {
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.06em;
    color: #5B6274;
  }

  .pace-card.hot {
    background: rgba(255, 138, 89, 0.08);
    border-color: rgba(255, 138, 89, 0.25);
  }

  .quick-items-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .quick-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .quick-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.08);
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s;
    color: #F2F3F5;
    font-size: 12px;
    font-weight: 600;
  }
  .quick-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.06);
    border-color: rgba(255, 255, 255, 0.16);
  }
  .quick-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .panel-card {
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.07);
    border-radius: 14px;
    padding: 14px;
  }
  .panel-title {
    font-size: 13.5px;
    font-weight: 700;
    color: #F2F3F5;
  }
  .panel-title-red {
    font-size: 12.5px;
    font-weight: 700;
    color: #FF8178;
    margin-bottom: 12px;
  }

  .tool-bar {
    display: flex;
    height: 9px;
    border-radius: 999px;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.07);
    gap: 2px;
    margin-bottom: 14px;
  }
  .tool-bar-seg {
    height: 100%;
    transition: width 0.3s ease;
  }
  .tools-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .tool-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .tool-name-group {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .tool-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .tool-name {
    font-size: 12.5px;
    font-weight: 600;
    color: #F2F3F5;
  }
  .tool-val-group {
    display: flex;
    align-items: baseline;
    gap: 5px;
  }
  .tool-val {
    font-size: 12.5px;
    font-weight: 700;
    color: #F2F3F5;
    font-family: "JetBrains Mono", monospace;
  }
  .tool-sub {
    font-size: 10px;
    color: #5B6274;
  }

  .limits-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }
  .plan-pill {
    padding: 2px 7px;
    border-radius: 6px;
    background: rgba(91, 140, 255, 0.15);
    border: 1px solid rgba(91, 140, 255, 0.35);
    color: #5B8CFF;
    font-size: 10px;
    font-weight: 700;
  }
  .limit-block {
    margin-top: 10px;
  }
  .limit-labels {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: #8B93A7;
    margin-bottom: 5px;
  }
  .limit-pct {
    font-weight: 700;
    color: #F2F3F5;
  }
  .limit-track {
    height: 6px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.08);
    overflow: hidden;
  }
  .limit-fill {
    height: 100%;
    background: linear-gradient(90deg, #5B8CFF, #39D98A);
    border-radius: 999px;
  }

  .wallet-card {
    position: relative;
    overflow: hidden;
    border-radius: 16px;
    padding: 18px;
    background: radial-gradient(circle at 20% 15%, rgba(227, 55, 45, 0.16), transparent 60%), #12141b;
    border: 1px solid rgba(255, 255, 255, 0.07);
  }
  .wallet-watermark {
    position: absolute;
    right: -18px;
    bottom: -18px;
    opacity: 0.05;
  }
  .wallet-content {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .wallet-coin {
    flex-shrink: 0;
  }
  .wallet-text {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .wallet-amount {
    font-size: 24px;
    font-weight: 700;
    color: #F2F3F5;
    font-family: "JetBrains Mono", monospace;
    line-height: 1;
  }
  .wallet-label {
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.07em;
    color: #8B93A7;
  }

  .bag-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .bag-count-badge {
    font-size: 11px;
    font-weight: 600;
    color: #5B6274;
    font-family: "JetBrains Mono", monospace;
  }
  .bag-list {
    display: flex;
    flex-direction: column;
    margin-top: 8px;
  }
  .bag-item-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    border-top: 1px solid rgba(255, 255, 255, 0.05);
  }
  .item-tile {
    width: 32px;
    height: 32px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .bag-item-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .bag-item-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .bag-item-name {
    font-size: 12px;
    font-weight: 600;
    color: #F2F3F5;
  }
  .bag-item-qty {
    font-size: 11.5px;
    font-weight: 700;
    color: #8B93A7;
    font-family: "JetBrains Mono", monospace;
  }
  .item-desc-text {
    font-size: 10px;
    line-height: 1.35;
    color: #8B93A7;
    margin: 1px 0 2px;
  }
  .empty-bag-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 20px 0 6px;
  }
  .empty-bag-text {
    font-size: 11.5px;
    color: #5B6274;
    text-align: center;
  }

  .shop-header {
    margin-bottom: 6px;
  }
  .shop-sub {
    font-size: 10.5px;
    color: #5B6274;
    display: block;
    margin-top: 2px;
  }
  .shop-items-list {
    display: flex;
    flex-direction: column;
  }
  .shop-item-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 0;
    border-top: 1px solid rgba(255, 255, 255, 0.05);
  }
  .shop-item-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .shop-item-name {
    font-size: 12px;
    font-weight: 600;
    color: #F2F3F5;
  }
  .shop-price-row {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .shop-price-val {
    font-size: 11px;
    font-weight: 600;
    color: #8B93A7;
    font-family: "JetBrains Mono", monospace;
  }
  .shop-price-unit {
    font-size: 10px;
    color: #5B6274;
  }
  .buy-action-btn {
    flex-shrink: 0;
    padding: 6px 14px;
    border-radius: 8px;
    background: rgba(227, 55, 45, 0.14);
    border: 1px solid rgba(227, 55, 45, 0.4);
    color: #FF8178;
    font-size: 11.5px;
    font-weight: 700;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .buy-action-btn:hover {
    background: #E3372E;
    color: #ffffff;
  }
  .pk-shake {
    animation: pk-shake 0.35s ease;
  }

  .pokedex-header-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .dex-counter {
    font-size: 11.5px;
    font-weight: 600;
    color: #5B6274;
  }
  .pokedex-2col-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .pokedex-card {
    position: relative;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.07);
    border-radius: 14px;
    padding: 16px 10px 12px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    text-align: center;
    min-height: 148px;
  }
  .card-top-stripe {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 3px;
  }
  .active-tag {
    position: absolute;
    top: 8px;
    right: 8px;
    padding: 2px 7px;
    border-radius: 999px;
    background: #E3372E;
    color: #ffffff;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.04em;
  }
  .dex-sprite-container {
    width: 72px;
    height: 72px;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.04);
    display: flex;
    align-items: center;
    justify-content: center;
    margin-top: 4px;
    overflow: hidden;
  }
  .dex-sprite-img {
    max-width: 68px;
    max-height: 68px;
    width: auto;
    height: auto;
    object-fit: contain;
    image-rendering: pixelated;
    filter: drop-shadow(0 3px 6px rgba(0, 0, 0, 0.4));
  }
  .dex-species-name {
    font-size: 12.5px;
    font-weight: 700;
    color: #F2F3F5;
    margin-top: 2px;
  }
  .dex-types-list {
    display: flex;
    gap: 4px;
    justify-content: center;
  }
  .dex-type-pill {
    padding: 2px 7px;
    border-radius: 999px;
    font-size: 9px;
    font-weight: 700;
  }

  .clickable-card {
    cursor: pointer;
    transition: transform 0.18s ease, border-color 0.18s ease, box-shadow 0.18s ease, background 0.18s ease;
    border: 1px solid rgba(255, 255, 255, 0.07);
    outline: none;
    font-family: inherit;
  }
  .clickable-card:hover {
    transform: translateY(-2px);
    background: rgba(255, 255, 255, 0.06);
    border-color: rgba(255, 255, 255, 0.18);
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.4);
  }
  .clickable-card:active {
    transform: translateY(0);
  }
  .dex-view-hint {
    margin-top: 2px;
    font-size: 9.5px;
    font-weight: 600;
    color: #5B6274;
    transition: color 0.15s ease;
  }
  .clickable-card:hover .dex-view-hint {
    color: #FF8178;
  }

  /* Pokédex Entry Detail Modal */
  .dex-modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 999;
    background: rgba(4, 6, 10, 0.78);
    backdrop-filter: blur(12px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 18px;
    animation: fadeIn 0.2s ease;
  }

  .dex-modal-card {
    position: relative;
    width: 100%;
    max-width: 340px;
    background: linear-gradient(180deg, #181C26 0%, #0E1017 100%);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 20px;
    padding: 20px 18px 18px;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.8), 0 0 30px rgba(227, 55, 45, 0.12);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    animation: modalPop 0.25s cubic-bezier(0.16, 1, 0.3, 1);
  }

  @keyframes modalPop {
    from {
      opacity: 0;
      transform: scale(0.92) translateY(8px);
    }
    to {
      opacity: 1;
      transform: scale(1) translateY(0);
    }
  }

  .dex-modal-close {
    position: absolute;
    top: 12px;
    right: 12px;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: #8B93A7;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: background 0.15s, color 0.15s, transform 0.15s;
  }
  .dex-modal-close:hover {
    background: rgba(227, 55, 45, 0.2);
    color: #ffffff;
    border-color: rgba(227, 55, 45, 0.5);
    transform: rotate(90deg);
  }

  .dex-modal-hero {
    position: relative;
    width: 100px;
    height: 100px;
    display: flex;
    align-items: center;
    justify-content: center;
    margin-top: 4px;
  }

  .dex-modal-glow {
    position: absolute;
    inset: -14px;
    border-radius: 50%;
    pointer-events: none;
  }

  .dex-modal-sprite {
    max-width: 90px;
    max-height: 90px;
    width: auto;
    height: auto;
    object-fit: contain;
    image-rendering: pixelated;
    filter: drop-shadow(0 6px 12px rgba(0, 0, 0, 0.6));
    z-index: 1;
  }

  .dex-modal-shiny-pill {
    position: absolute;
    bottom: -6px;
    padding: 2px 8px;
    border-radius: 999px;
    background: linear-gradient(135deg, #E8B84B, #FF8A59);
    color: #111319;
    font-size: 9.5px;
    font-weight: 800;
    box-shadow: 0 2px 8px rgba(232, 184, 75, 0.4);
    z-index: 2;
  }

  .dex-modal-header {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    text-align: center;
    width: 100%;
  }

  .dex-modal-id {
    font-family: "JetBrains Mono", monospace;
    font-size: 11px;
    font-weight: 700;
    color: #8B93A7;
    letter-spacing: 0.06em;
  }

  .dex-modal-name {
    font-size: 18px;
    font-weight: 800;
    color: #F2F3F5;
    letter-spacing: -0.01em;
  }

  .dex-modal-genus {
    font-size: 11px;
    font-weight: 600;
    color: #8B93A7;
  }

  .dex-modal-types {
    display: flex;
    gap: 5px;
    margin-top: 4px;
  }

  .dex-modal-body {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .dex-flavor-card {
    position: relative;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.07);
    border-radius: 12px;
    padding: 12px 14px;
    min-height: 64px;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }

  .dex-flavor-quote {
    position: absolute;
    top: 4px;
    left: 8px;
    font-size: 24px;
    line-height: 1;
    color: rgba(255, 255, 255, 0.08);
    font-family: Georgia, serif;
    pointer-events: none;
  }

  .dex-flavor-text {
    margin: 0;
    font-size: 11.5px;
    line-height: 1.5;
    color: #C8CDD8;
    text-align: center;
    font-style: italic;
  }

  .dex-stats-grid {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 6px;
  }

  .dex-stat-item {
    background: rgba(255, 255, 255, 0.025);
    border: 1px solid rgba(255, 255, 255, 0.05);
    border-radius: 10px;
    padding: 7px 4px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    text-align: center;
  }

  .dex-stat-label {
    font-size: 8.5px;
    font-weight: 700;
    color: #5B6274;
    letter-spacing: 0.05em;
  }

  .dex-stat-value {
    font-size: 11.5px;
    font-weight: 700;
    font-family: "JetBrains Mono", monospace;
    color: #F2F3F5;
  }

  .dex-modal-loading {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 20px 0;
    gap: 8px;
  }

  .loading-dex-text {
    font-size: 11px;
    color: #5B6274;
  }

  .locked-slot-card {
    background: rgba(255, 255, 255, 0.015);
    border: 1.5px dashed rgba(255, 255, 255, 0.1);
    border-radius: 14px;
    padding: 16px 12px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-height: 148px;
  }
  .locked-silhouette-box {
    width: 72px;
    height: 72px;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.03);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .locked-question {
    font-size: 26px;
    font-weight: 700;
    color: rgba(255, 255, 255, 0.13);
  }
  .locked-label {
    font-size: 10.5px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.2);
    letter-spacing: 0.04em;
  }

  .celebration-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    background: linear-gradient(90deg, #ff8a00, #e52e71);
    color: white;
    border-radius: 10px;
    font-size: 12px;
    box-shadow: 0 4px 14px rgba(229, 46, 113, 0.4);
  }
  .celebration-icon {
    font-size: 16px;
  }
  .celebration-text {
    flex: 1;
  }
  .celebration-close {
    background: transparent;
    border: none;
    color: white;
    font-size: 12px;
    cursor: pointer;
    opacity: 0.8;
  }
  .celebration-close:hover {
    opacity: 1;
  }

  .settings-container {
    display: flex;
    flex-direction: column;
    gap: 12px;
    animation: fadeIn 0.2s ease;
  }
  .settings-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .settings-title {
    font-size: 14px;
    font-weight: 700;
    color: #F2F3F5;
  }
  .back-btn {
    padding: 4px 12px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.12);
    color: #F2F3F5;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }
  .settings-card {
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.07);
    border-radius: 14px;
    padding: 6px 14px;
    display: flex;
    flex-direction: column;
  }
  .setting-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 0;
    border-bottom: 1px solid rgba(255, 255, 255, 0.05);
  }
  .setting-row:last-child {
    border-bottom: none;
  }
  .setting-row.theme-row {
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
  }
  .setting-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .setting-label {
    font-size: 12.5px;
    font-weight: 600;
    color: #F2F3F5;
  }
  .setting-sub {
    font-size: 10.5px;
    color: #5B6274;
  }
  .toggle-switch {
    width: 38px;
    height: 22px;
    border-radius: 11px;
    background: rgba(255, 255, 255, 0.12);
    border: none;
    position: relative;
    cursor: pointer;
    transition: background 0.2s ease;
  }
  .toggle-switch.active {
    background: #E3372E;
  }
  .toggle-handle {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #ffffff;
    position: absolute;
    top: 3px;
    left: 3px;
    transition: transform 0.2s ease;
  }
  .toggle-switch.active .toggle-handle {
    transform: translateX(16px);
  }
  .action-btn {
    padding: 5px 12px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.15);
    color: #F2F3F5;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }
  .pill-options {
    display: flex;
    gap: 4px;
  }
  .pill-choice {
    padding: 4px 8px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.08);
    color: #8B93A7;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }
  .pill-choice.selected {
    background: #E3372E;
    color: #ffffff;
    border-color: #E3372E;
  }
  .theme-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
    width: 100%;
  }
  .theme-choice {
    padding: 8px 10px;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    color: #8B93A7;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    text-align: left;
  }
  .theme-choice.selected {
    border-color: #E3372E;
    color: #ffffff;
    background: rgba(227, 55, 45, 0.15);
  }

  .about-box {
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 12px;
    padding: 12px 14px;
  }
  .about-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 12.5px;
    font-weight: 700;
    color: #F2F3F5;
  }
  .version-tag {
    font-size: 10px;
    font-family: "JetBrains Mono", monospace;
    padding: 2px 6px;
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.08);
    color: #8B93A7;
  }
  .about-sub {
    margin: 6px 0 0;
    font-size: 11px;
    color: #5B6274;
  }

  .empty-state-loading {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 240px;
    gap: 12px;
  }
  .loading-spinner {
    width: 28px;
    height: 28px;
    border: 3px solid rgba(255, 255, 255, 0.1);
    border-top-color: #E3372E;
    border-radius: 50%;
    animation: pk-spin 0.8s linear infinite;
  }
  .loading-text {
    font-size: 12px;
    color: #5B6274;
    margin: 0;
  }
  .empty-hint {
    font-size: 11.5px;
    color: #5B6274;
    margin: 0;
  }

  /* Desktop Pet 2.0 & Interactive Moods in Main HUD */
  .hero-tag-group {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .hero-sleep-pill {
    font-size: 10px;
    font-weight: 700;
    padding: 2px 8px;
    border-radius: 999px;
    background: rgba(167, 139, 250, 0.18);
    border: 1px solid rgba(167, 139, 250, 0.4);
    color: #C084FC;
    letter-spacing: 0.02em;
    animation: fadeIn 0.3s ease;
  }

  .hero-gold-pill {
    font-size: 10px;
    font-weight: 700;
    padding: 2px 8px;
    border-radius: 999px;
    background: rgba(245, 158, 11, 0.18);
    border: 1px solid rgba(245, 158, 11, 0.45);
    color: #FCD34D;
    letter-spacing: 0.02em;
    animation: goldPulseText 2s infinite ease-in-out;
  }

  @keyframes goldPulseText {
    0%, 100% { opacity: 0.85; text-shadow: 0 0 6px rgba(245, 158, 11, 0.4); }
    50% { opacity: 1; text-shadow: 0 0 12px rgba(245, 158, 11, 0.8); }
  }

  .interactive-hero-box {
    position: relative;
    cursor: pointer;
    transition: transform 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
  }

  .interactive-hero-box:hover {
    transform: scale(1.06);
  }

  .interactive-hero-box:active {
    transform: scale(0.96);
  }

  .interactive-hero-box:focus {
    outline: none;
  }

  /* Hero Animations */
  .hero-sprite.hop {
    animation: petHop 0.55s cubic-bezier(0.34, 1.56, 0.64, 1) !important;
  }

  .hero-sprite.backflip {
    animation: petBackflip 0.72s cubic-bezier(0.34, 1.56, 0.64, 1) !important;
  }

  .hero-sprite.wiggle {
    animation: petWiggle 0.6s ease-in-out !important;
  }

  .hero-sprite.wake-up {
    animation: wakePop 0.8s cubic-bezier(0.34, 1.56, 0.64, 1) !important;
  }

  .hero-sprite.eating {
    animation: eatingNom 0.8s ease-in-out !important;
  }

  .hero-sprite.sleeping-mon {
    animation: sleepBreath 3.2s infinite ease-in-out !important;
  }

  /* Hero Sleep Zzz */
  .hero-zzz-box {
    position: absolute;
    top: -10px;
    right: -4px;
    display: flex;
    flex-direction: column;
    pointer-events: none;
  }

  .hero-zzz {
    position: absolute;
    color: #C084FC;
    font-weight: 800;
    text-shadow: 0 2px 6px rgba(0, 0, 0, 0.8);
    opacity: 0;
    animation: floatZzz 3s infinite cubic-bezier(0.25, 0.46, 0.45, 0.94);
  }

  .hero-zzz-1 { font-size: 11px; animation-delay: 0s; }
  .hero-zzz-2 { font-size: 14px; animation-delay: 1s; }
  .hero-zzz-3 { font-size: 17px; animation-delay: 2s; }

  /* Hero Golden Sparkles */
  .hero-gold-sparkles {
    position: absolute;
    inset: -6px;
    pointer-events: none;
  }

  .hero-sp {
    position: absolute;
    font-size: 14px;
    animation: floatSparkle 2s infinite ease-in-out;
    filter: drop-shadow(0 0 8px rgba(245, 158, 11, 0.85));
  }

  .hero-sp.sp-1 { top: 0; left: 0; animation-delay: 0s; }
  .hero-sp.sp-2 { bottom: 0; right: 0; animation-delay: 1s; font-size: 12px; }

  /* Hero Floating Hearts */
  .hero-floating-heart {
    position: absolute;
    font-size: 18px;
    pointer-events: none;
    animation: heartFly 1.1s cubic-bezier(0.22, 1, 0.36, 1) forwards;
    filter: drop-shadow(0 2px 8px rgba(0, 0, 0, 0.7));
    z-index: 20;
  }

  /* Bag Actions & Item Buttons */
  .item-tile-emoji {
    font-size: 16px;
    line-height: 1;
  }

  .bag-item-action {
    display: flex;
    align-items: center;
    margin-left: 8px;
  }

  .bag-use-btn {
    padding: 5px 12px;
    border-radius: 8px;
    font-size: 11px;
    font-weight: 700;
    cursor: pointer;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.14);
    color: #F2F3F5;
    transition: all 0.15s ease;
  }

  .bag-use-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.14);
    border-color: rgba(255, 255, 255, 0.25);
    transform: translateY(-1px);
  }

  .bag-use-btn:active:not(:disabled) {
    transform: translateY(0);
  }

  .bag-use-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .bag-use-btn.berry-feed-btn {
    background: rgba(59, 130, 246, 0.18);
    border-color: rgba(59, 130, 246, 0.45);
    color: #93C5FD;
  }

  .bag-use-btn.berry-feed-btn:hover:not(:disabled) {
    background: rgba(59, 130, 246, 0.3);
    border-color: rgba(59, 130, 246, 0.65);
    box-shadow: 0 0 10px rgba(59, 130, 246, 0.3);
  }

  .bag-held-badge {
    font-size: 10.5px;
    font-weight: 700;
    padding: 3px 8px;
    border-radius: 999px;
    background: rgba(252, 211, 77, 0.14);
    border: 1px solid rgba(252, 211, 77, 0.35);
    color: #FCD34D;
  }

  .quick-btn .btn-emoji {
    font-size: 15px;
    line-height: 1;
  }

  .quick-btn.berry-btn {
    background: rgba(59, 130, 246, 0.08);
    border-color: rgba(59, 130, 246, 0.25);
    color: #93C5FD;
  }

  .quick-btn.berry-btn:hover:not(:disabled) {
    background: rgba(59, 130, 246, 0.18);
    border-color: rgba(59, 130, 246, 0.45);
  }

  .quick-btn.berry-btn.gold {
    background: rgba(245, 158, 11, 0.08);
    border-color: rgba(245, 158, 11, 0.25);
    color: #FCD34D;
  }

  .quick-btn.berry-btn.gold:hover:not(:disabled) {
    background: rgba(245, 158, 11, 0.18);
    border-color: rgba(245, 158, 11, 0.45);
  }
</style>
