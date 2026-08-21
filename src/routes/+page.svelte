<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { isEnabled, enable, disable } from "@tauri-apps/plugin-autostart";
  import { onMount } from "svelte";
  import { resolveOverdrive } from "$lib/mega";
  import { ALL_RIBBONS, ORDERED_RIBBON_IDS, getRibbon } from "$lib/ribbons";

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
    ribbons?: string[];
  }

  interface JournalEntry {
    id: string;
    timestamp: string;
    kind: string;
    title: string;
    description: string;
    icon: string;
    speciesId?: number | null;
    isShiny?: boolean;
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
    isMegaOverdrive?: boolean;
    megaOverdriveEnabled?: boolean;
    ribbons?: string[];
    trainerName?: string;
    trainerId?: string;
    trainerTitle?: string;
    avatarSpeciesId?: number | null;
    journal?: JournalEntry[];
    dailyHistory?: Record<string, number>;
    bp?: number;
    battleStats?: BattleStatsRecord;
    activeBattle?: ActiveBattleState | null;
    state?: {
      usedSinceInstall?: number;
    };
  }

  interface BattleMove {
    id: string;
    name: string;
    element: string;
    category: "Physical" | "Special" | "Status";
    power: number;
    accuracy: number;
    currentPp: number;
    maxPp: number;
    description: string;
    effect?: string | null;
  }

  interface BattleFighter {
    speciesId: number;
    name: string;
    isShiny: boolean;
    level: number;
    stage: number;
    elementTypes: string[];
    maxHp: number;
    currentHp: number;
    attack: number;
    defense: number;
    spAttack: number;
    spDefense: number;
    speed: number;
    ribbonCount: number;
    isOverdrive: boolean;
    atkStage: number;
    defStage: number;
    moves: BattleMove[];
  }

  interface BattleLogEntry {
    id: string;
    text: string;
    actor: "player" | "opponent" | "system";
    damage?: number | null;
    isCrit: boolean;
    effectiveness: "super" | "not_very" | "immune" | "normal";
    timestamp: number;
  }

  interface ActiveBattleState {
    battleId: string;
    turnCount: number;
    player: BattleFighter;
    opponent: BattleFighter;
    isPlayerTurn: boolean;
    battlePhase: "selecting" | "resolving" | "won" | "lost" | "fled";
    battleLog: BattleLogEntry[];
    rewardBp: number;
    rewardCoins: number;
    won?: boolean | null;
  }

  interface BattleStatsRecord {
    wins: number;
    losses: number;
    winStreak: number;
    bestStreak: number;
    totalBattles: number;
    totalBpEarned: number;
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

  type Tab = "buddy" | "usage" | "shop" | "pokedex" | "trainer" | "arena";
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

  let isEditingTrainerName = $state(false);
  let trainerNameInput = $state("");
  let showAvatarPickerModal = $state(false);
  let hoveredHeatmapCell = $state<{
    dateStr: string;
    tokens: number;
    formattedDate: string;
    x: number;
    y: number;
  } | null>(null);

  function focusOnMount(node: HTMLElement) {
    node.focus();
    if (node instanceof HTMLInputElement) {
      node.select();
    }
  }

  function startEditingTrainerName() {
    trainerNameInput = snap?.companion.trainerName ?? "Trainer";
    isEditingTrainerName = true;
  }

  async function saveTrainerName() {
    const trimmed = trainerNameInput.trim();
    if (!trimmed) {
      isEditingTrainerName = false;
      return;
    }
    try {
      const res = await invoke<Snapshot>("set_trainer_name", { name: trimmed });
      if (res) snap = res;
    } catch (e) {
      console.error("Failed to set trainer name:", e);
    } finally {
      isEditingTrainerName = false;
    }
  }

  async function chooseAvatar(speciesId: number | null) {
    try {
      const res = await invoke<Snapshot>("set_trainer_avatar", { speciesId });
      if (res) snap = res;
    } catch (e) {
      console.error("Failed to set trainer avatar:", e);
    } finally {
      showAvatarPickerModal = false;
    }
  }

  // ⚔️ Battle Arena State & Actions
  let selectedFighterSpeciesId = $state<number | null>(null);
  let isExecutingMove = $state(false);
  let arenaNotice = $state<string | null>(null);

  async function startArenaBattle(speciesId?: number | null) {
    try {
      const res = await invoke<Snapshot>("start_battle", { speciesId: speciesId ?? selectedFighterSpeciesId });
      if (res) snap = res;
    } catch (e) {
      console.error("Failed to start battle:", e);
    }
  }

  async function sendBattleMove(moveIdx: number) {
    if (isExecutingMove) return;
    isExecutingMove = true;
    try {
      const res = await invoke<Snapshot>("execute_battle_move", { moveIndex: moveIdx });
      if (res) snap = res;
    } catch (e) {
      console.error("Failed to execute move:", e);
    } finally {
      isExecutingMove = false;
    }
  }

  async function fleeArenaBattle() {
    try {
      const res = await invoke<Snapshot>("flee_battle");
      if (res) snap = res;
    } catch (e) {
      console.error("Failed to flee battle:", e);
    }
  }

  async function clearArenaBattle() {
    try {
      const res = await invoke<Snapshot>("clear_battle");
      if (res) snap = res;
    } catch (e) {
      console.error("Failed to clear battle:", e);
    }
  }

  async function buyBpShopItem(itemId: string) {
    try {
      const res = await invoke<Snapshot>("buy_bp_item", { itemId });
      if (res) snap = res;
      arenaNotice = "Item acquired successfully!";
      setTimeout(() => { arenaNotice = null; }, 2500);
    } catch (e: any) {
      arenaNotice = typeof e === "string" ? e : "Not enough Battle Points!";
      setTimeout(() => { arenaNotice = null; }, 3000);
    }
  }

  interface DayCell {
    dateStr: string;
    tokens: number;
    level: number;
    formattedDate: string;
    dayOfWeek: number;
  }

  interface WeekCol {
    monthLabel: string | null;
    days: DayCell[];
  }

  const heatmapInfo = $derived.by(() => {
    const history = snap?.companion.dailyHistory ?? {};
    const totalBurned = snap?.companion.state?.usedSinceInstall ?? 0;

    const today = new Date();
    const todayStr = today.toISOString().split("T")[0];

    const dates = Object.keys(history).sort();
    let currentStreak = 0;
    let maxStreak = 0;
    let activeDays = 0;

    let tempStreak = 0;
    let checkDate = new Date(today);

    const todayTokens = history[todayStr] ?? 0;
    if (todayTokens === 0) {
      checkDate.setDate(checkDate.getDate() - 1);
    }

    for (let i = 0; i < 365; i++) {
      const dStr = checkDate.toISOString().split("T")[0];
      if ((history[dStr] ?? 0) > 0) {
        currentStreak++;
        checkDate.setDate(checkDate.getDate() - 1);
      } else {
        break;
      }
    }

    let prevTime = 0;
    for (const d of dates) {
      if ((history[d] ?? 0) > 0) {
        activeDays++;
        const currTime = new Date(d).getTime();
        if (prevTime > 0 && currTime - prevTime === 86400000) {
          tempStreak++;
        } else {
          tempStreak = 1;
        }
        if (tempStreak > maxStreak) maxStreak = tempStreak;
        prevTime = currTime;
      }
    }
    if (currentStreak > maxStreak) maxStreak = currentStreak;

    const weeks: WeekCol[] = [];
    const endDate = new Date(today);
    const dayOfWeek = (endDate.getDay() + 6) % 7;
    const daysUntilSunday = 6 - dayOfWeek;
    const gridEnd = new Date(endDate);
    gridEnd.setDate(gridEnd.getDate() + daysUntilSunday);

    const totalDays = 26 * 7;
    const gridStart = new Date(gridEnd);
    gridStart.setDate(gridStart.getDate() - totalDays + 1);

    let lastMonth = -1;
    const currentDayIter = new Date(gridStart);

    for (let w = 0; w < 26; w++) {
      const weekDays: DayCell[] = [];
      let monthForWeek: string | null = null;

      for (let d = 0; d < 7; d++) {
        const dStr = currentDayIter.toISOString().split("T")[0];
        const dayTokens = history[dStr] ?? 0;

        let lvl = 0;
        if (dayTokens > 10_000_000) lvl = 4;
        else if (dayTokens >= 2_500_000) lvl = 3;
        else if (dayTokens >= 500_000) lvl = 2;
        else if (dayTokens > 0) lvl = 1;

        const m = currentDayIter.getMonth();
        if (m !== lastMonth && d === 0) {
          lastMonth = m;
          monthForWeek = currentDayIter.toLocaleDateString("en-US", { month: "short" });
        }

        const dateOptions: Intl.DateTimeFormatOptions = {
          weekday: "short",
          month: "short",
          day: "numeric",
          year: "numeric",
        };
        weekDays.push({
          dateStr: dStr,
          tokens: dayTokens,
          level: lvl,
          formattedDate: currentDayIter.toLocaleDateString("en-US", dateOptions),
          dayOfWeek: d,
        });

        currentDayIter.setDate(currentDayIter.getDate() + 1);
      }

      weeks.push({
        monthLabel: monthForWeek,
        days: weekDays,
      });
    }

    return {
      weeks,
      currentStreak,
      maxStreak,
      activeDays,
      dailyAverage: activeDays > 0 ? Math.round(totalBurned / activeDays) : 0,
    };
  });

  let selectedDexMon = $state<{
    id: number;
    name: string;
    isShiny: boolean;
    isRaising?: boolean;
    ribbons?: string[];
  } | null>(null);
  let dexDetails = $state<PokedexDetails | null>(null);
  let dexDetailsLoading = $state(false);

  let showRibbonModal = $state(false);
  let selectedRibbonMonName = $state("");
  let selectedRibbonList = $state<string[]>([]);

  function openRibbonModal(monName: string, ribbons: string[] = []) {
    selectedRibbonMonName = monName;
    selectedRibbonList = ribbons;
    showRibbonModal = true;
  }

  async function openDexDetails(mon: { id: number; name: string; isShiny: boolean; isRaising?: boolean; ribbons?: string[] }) {
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
  let heroAnimState = $state<"normal" | "hop" | "wiggle" | "wake" | "eating">("normal");
  let heroAnimTimeout: ReturnType<typeof setTimeout> | null = null;
  let heroWakeTimeout: ReturnType<typeof setTimeout> | null = null;
  let heroWakingUp = $state(false);

  let isSleepingHero = $derived.by(() => {
    if (!snap) return false;
    if (snap.companion.isEgg) return false;
    if (heroWakingUp) return false;
    return snap.companion.displayState === "sleep";
  });

  function triggerHeroPet(specificAnim?: "hop" | "wiggle" | "wake" | "eating") {
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

    const rolls: Array<"hop" | "wiggle"> = ["hop", "wiggle", "hop", "wiggle"];
    const chosen = specificAnim ?? rolls[Math.floor(Math.random() * rolls.length)];
    heroAnimState = chosen;
    const dur = chosen === "eating" ? 900 : chosen === "wake" ? 1000 : 600;
    heroAnimTimeout = setTimeout(() => {
      heroAnimState = "normal";
    }, dur);

    const emojis = specificAnim === "eating" ? ["😋", "✨", "💖"] : ["❤️", "💖", "✨", "🥰", "⭐"];
    spawnHeroHearts(emojis);
    invoke<Snapshot>("pet_buddy").then((res) => { if (res) snap = res; }).catch(() => {});
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
    if (
      e.button === 0 &&
      !(e.target as HTMLElement).closest(
        "button, select, input, .tab-btn, .window-btn, .interactive-hero-box, .pokedex-card, .buy-action-btn, .bag-use-btn, .quick-btn"
      )
    ) {
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

  let megaOverdriveSetting = $state(false);

  function toggleAnimatedSprites() {
    animatedSprites = !animatedSprites;
    try {
      localStorage.setItem("ptb_animated_sprites", animatedSprites ? "true" : "false");
    } catch {
      // ignore
    }
  }

  async function toggleMegaOverdrive() {
    megaOverdriveSetting = !megaOverdriveSetting;
    try {
      localStorage.setItem("ptb_mega_overdrive", megaOverdriveSetting ? "true" : "false");
      snap = await invoke<Snapshot>("set_mega_overdrive_enabled", { enabled: megaOverdriveSetting });
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
      const savedOverdrive = localStorage.getItem("ptb_mega_overdrive");
      if (savedOverdrive !== null) {
        megaOverdriveSetting = savedOverdrive === "true";
        invoke<Snapshot>("set_mega_overdrive_enabled", { enabled: megaOverdriveSetting })
          .then((s) => {
            if (s) snap = s;
          })
          .catch(() => {});
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

  const ELEMENT_COLORS: Record<string, { text: string; bg: string; border: string }> = {
    normal: { text: "#A8A878", bg: "rgba(168,168,120,0.18)", border: "1px solid rgba(168,168,120,0.4)" },
    fire: { text: "#F08030", bg: "rgba(240,128,48,0.18)", border: "1px solid rgba(240,128,48,0.4)" },
    water: { text: "#6890F0", bg: "rgba(104,144,240,0.18)", border: "1px solid rgba(104,144,240,0.4)" },
    grass: { text: "#78C850", bg: "rgba(120,200,80,0.18)", border: "1px solid rgba(120,200,80,0.4)" },
    electric: { text: "#F8D030", bg: "rgba(248,208,48,0.18)", border: "1px solid rgba(248,208,48,0.4)" },
    ice: { text: "#98D8D8", bg: "rgba(152,216,216,0.18)", border: "1px solid rgba(152,216,216,0.4)" },
    fighting: { text: "#C03028", bg: "rgba(192,48,40,0.18)", border: "1px solid rgba(192,48,40,0.4)" },
    poison: { text: "#A040A0", bg: "rgba(160,64,160,0.18)", border: "1px solid rgba(160,64,160,0.4)" },
    ground: { text: "#E0C068", bg: "rgba(224,192,104,0.18)", border: "1px solid rgba(224,192,104,0.4)" },
    flying: { text: "#A890F0", bg: "rgba(168,144,240,0.18)", border: "1px solid rgba(168,144,240,0.4)" },
    psychic: { text: "#F85888", bg: "rgba(248,88,136,0.18)", border: "1px solid rgba(248,88,136,0.4)" },
    bug: { text: "#A8B820", bg: "rgba(168,184,32,0.18)", border: "1px solid rgba(168,184,32,0.4)" },
    rock: { text: "#B8A038", bg: "rgba(184,160,56,0.18)", border: "1px solid rgba(184,160,56,0.4)" },
    ghost: { text: "#705898", bg: "rgba(112,88,152,0.18)", border: "1px solid rgba(112,88,152,0.4)" },
    dragon: { text: "#7038F8", bg: "rgba(112,56,248,0.18)", border: "1px solid rgba(112,56,248,0.4)" },
    dark: { text: "#705848", bg: "rgba(112,88,72,0.18)", border: "1px solid rgba(112,88,72,0.4)" },
    steel: { text: "#B8B8D0", bg: "rgba(184,184,208,0.18)", border: "1px solid rgba(184,184,208,0.4)" },
    fairy: { text: "#EE99AC", bg: "rgba(238,153,172,0.18)", border: "1px solid rgba(238,153,172,0.4)" },
  };

  function getElementColor(element: string): { text: string; bg: string; border: string } {
    const key = element.toLowerCase();
    return ELEMENT_COLORS[key] || ELEMENT_COLORS.normal;
  }

  function getStageInfo(stageText: string, isFinal: boolean): { stage: number; total: number } {
    const match = stageText.match(/(\d+)\s*(?:\/|of)\s*(\d+)/i);
    if (match) {
      return { stage: parseInt(match[1], 10), total: parseInt(match[2], 10) };
    }
    if (isFinal || stageText.toLowerCase().includes("final")) {
      return { stage: 3, total: 3 };
    }
    return { stage: 1, total: 3 };
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
    { id: "trainer", label: "Passport" },
    { id: "arena", label: "Arena" },
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
          {:else if tab.id === "trainer"}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <rect x="3" y="4" width="18" height="16" rx="3"></rect>
              <circle cx="9" cy="10" r="2.5"></circle>
              <path d="M15 8h4"></path>
              <path d="M15 12h4"></path>
              <path d="M5 18c0-2 2-3.5 4-3.5s4 1.5 4 3.5"></path>
            </svg>
          {:else if tab.id === "arena"}
            <span style="font-size: 13px;">⚔️</span>
          {/if}
          <span>{tab.label}</span>
          {#if tab.id === "pokedex" && snap && snap.companion.dex.length > 0}
            <span class="tab-badge">{snap.companion.dex.length}</span>
          {:else if tab.id === "trainer" && snap && (snap.companion.journal ?? []).length > 0}
            <span class="tab-badge">{(snap.companion.journal ?? []).length}</span>
          {:else if tab.id === "arena" && snap && (snap.companion.bp ?? 0) > 0}
            <span class="tab-badge bp-badge">{snap.companion.bp} BP</span>
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
                <span class="setting-label">⚡ Mega Overdrive & G-Max</span>
                <span class="setting-sub">Mega Evolve & 2× Coin Rush during Fast / Blazing sprints</span>
              </div>
              <button class="toggle-switch" class:active={megaOverdriveSetting} onclick={toggleMegaOverdrive} aria-label="Toggle Mega Overdrive">
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
              <span class="version-tag">v0.4.0</span>
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
              {@const isOverdriveActive = Boolean(c.isMegaOverdrive || (megaOverdriveSetting && (u.burnTier === "fast" || u.burnTier === "blazing")))}
              {@const overdriveInfo = isOverdriveActive ? resolveOverdrive(c.currentSpeciesId, c.displayName) : null}
              {@const effectiveSpeciesId = overdriveInfo ? overdriveInfo.spriteId : c.currentSpeciesId}
              {@const effectiveName = overdriveInfo ? overdriveInfo.displayName : c.displayName}
              {@const typeInfo = getTypes(c.currentSpeciesId)}
              {@const stageInfo = getStageInfo(c.stageText, c.isFinalStage)}
              {@const growthPct = Math.round(c.progress * 100)}

              <div
                class="buddy-hero"
                class:sleeping-hero={isSleepingHero}
                class:golden-hero={c.hasGoldenAura}
                class:overdrive-hero={isOverdriveActive}
              >
                <div class="hero-glow" style="background: radial-gradient(circle, {isOverdriveActive ? 'rgba(255, 0, 122, 0.28)' : typeInfo.primary.bg.replace('0.14', '0.22')}, transparent 70%);"></div>
                <div class="hero-header-row">
                  <div class="hero-tag-group">
                    <span class="section-tag">ACTIVE BUDDY</span>
                    {#if isOverdriveActive && overdriveInfo}
                      <span class="hero-overdrive-pill">{overdriveInfo.badge}</span>
                    {/if}
                    {#if isSleepingHero}
                      <span class="hero-sleep-pill">💤 Sleeping</span>
                    {:else if c.hasGoldenAura}
                      <span class="hero-gold-pill">✨ Sitrus Sparkle</span>
                    {/if}
                  </div>
                  <div class="hero-actions-right">
                    <button
                      class="pet-buddy-btn"
                      onclick={() => triggerHeroPet()}
                      title="Pet and play with your companion!"
                      type="button"
                    >
                      <span>🥰 Pet</span>
                    </button>
                    <div class="stage-pips">
                      {#each Array(stageInfo.total) as _, i}
                        <span class="pip" class:filled={i < stageInfo.stage}></span>
                      {/each}
                    </div>
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
                      class:wiggle={heroAnimState === "wiggle"}
                      class:wake-up={heroAnimState === "wake"}
                      class:eating={heroAnimState === "eating"}
                      class:sleeping-mon={isSleepingHero}
                      class:overdrive-sprite={isOverdriveActive}
                      src={spriteUrl(effectiveSpeciesId, c.isShiny)}
                      alt={effectiveName}
                      onerror={(e) => fallbackStaticSprite(e, effectiveSpeciesId ?? 1, c.isShiny)}
                    />

                    {#if isOverdriveActive}
                      <div class="hero-overdrive-sparks">
                        <span class="od-spark od-1">⚡</span>
                        <span class="od-spark od-2">🧬</span>
                        <span class="od-spark od-3">💥</span>
                      </div>
                    {/if}

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
                        {effectiveName}
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
                      <span class="stage-label">{c.stageText ? c.stageText : `Stage ${stageInfo.stage}/${stageInfo.total}`}</span>
                      <span class="stage-pct">{growthPct}%</span>
                    </div>

                    <!-- Interactive Ribbon Bar -->
                    <div
                      class="hero-ribbons-bar"
                      onclick={() => openRibbonModal(effectiveName, c.ribbons ?? [])}
                      role="button"
                      tabindex="0"
                      onkeydown={(e) => e.key === 'Enter' && openRibbonModal(effectiveName, c.ribbons ?? [])}
                      title="Click to open {effectiveName}’s Ribbon Case & Achievements"
                    >
                      <div class="ribbons-left">
                        <span class="ribbon-bar-icon">🏅</span>
                        <span class="ribbon-count-text">Ribbons ({(c.ribbons ?? []).length}):</span>
                      </div>
                      <div class="ribbons-pills-row">
                        {#each (c.ribbons ?? []).slice(0, 3) as rId}
                          {@const r = getRibbon(rId)}
                          {#if r}
                            <span class="hero-mini-ribbon" style="--rib-color: {r.color}; --rib-glow: {r.glow};">
                              <span class="hm-icon">{r.icon}</span>
                              <span class="hm-text">{r.name}</span>
                            </span>
                          {/if}
                        {/each}
                        {#if (c.ribbons ?? []).length > 3}
                          <span class="hero-mini-ribbon more-ribbons">+{(c.ribbons ?? []).length - 3}</span>
                        {/if}
                      </div>
                      <span class="ribbons-arrow">➔</span>
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
          {@const isOverdriveActive = Boolean(c.isMegaOverdrive || (megaOverdriveSetting && (u.burnTier === "fast" || u.burnTier === "blazing")))}
          <div class="tab-pane">
            {#if isOverdriveActive}
              <div class="coin-rush-banner-top">
                <span class="rush-icon">⚡</span>
                <div class="rush-text-col">
                  <span class="rush-title">2× COIN RUSH ACTIVE</span>
                  <span class="rush-sub">Mega Overdrive Multiplier (Fast/Blazing Sprint)</span>
                </div>
              </div>
            {/if}

            <div class="wallet-card" class:overdrive-wallet={isOverdriveActive}>
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

        {#if currentTab === "trainer"}
          {@const avatarId = c.avatarSpeciesId ?? c.currentSpeciesId ?? (c.dex[0]?.id ?? 25)}
          {@const trainerTitle = c.trainerTitle ?? "Novice Pokémon Trainer"}
          {@const trainerId = c.trainerId ?? "TR-0001"}
          {@const trainerName = c.trainerName ?? "Trainer"}
          <div class="tab-pane trainer-pane">

            <!-- 🪪 Holographic Trainer Passport Card -->
            <div class="trainer-passport-card">
              <div class="passport-foil-glow"></div>

              <div class="passport-header">
                <div class="passport-title-badge">
                  <span class="passport-icon">🪪</span>
                  <span class="passport-title-text">POKÉMON TRAINER PASSPORT</span>
                </div>
                <span class="passport-region-badge">KANTO REGION</span>
              </div>

              <div class="passport-body">
                <!-- Avatar column -->
                <div class="passport-avatar-box">
                  <button
                    type="button"
                    class="avatar-frame"
                    onclick={() => { showAvatarPickerModal = true; }}
                    title="Click to Change Trainer Avatar"
                    aria-label="Change Trainer Avatar"
                  >
                    <img
                      class="avatar-sprite-img"
                      src={spriteUrl(avatarId, c.isShiny, true)}
                      alt="Trainer Avatar"
                      onerror={(e) => fallbackStaticSprite(e, avatarId, false)}
                    />
                    <div class="avatar-edit-overlay">
                      <span>Change ✎</span>
                    </div>
                  </button>
                  <span class="avatar-helper-text">Avatar</span>
                </div>

                <!-- Info column -->
                <div class="passport-info-col">
                  <div class="passport-name-row">
                    {#if isEditingTrainerName}
                      <div class="name-edit-box">
                        <input
                          use:focusOnMount
                          type="text"
                          class="trainer-name-input"
                          bind:value={trainerNameInput}
                          maxlength="24"
                          placeholder="Enter trainer nickname…"
                          onkeydown={(e) => {
                            if (e.key === "Enter") saveTrainerName();
                            if (e.key === "Escape") isEditingTrainerName = false;
                          }}
                        />
                        <button class="save-name-btn" onclick={saveTrainerName}>✓</button>
                        <button class="cancel-name-btn" onclick={() => isEditingTrainerName = false}>✕</button>
                      </div>
                    {:else}
                      <button
                        type="button"
                        class="trainer-name-display"
                        onclick={startEditingTrainerName}
                        title="Click to Edit Nickname"
                        aria-label="Click to Edit Nickname"
                      >
                        <span class="trainer-nickname">{trainerName}</span>
                        <span class="edit-pencil-icon">✎</span>
                      </button>
                    {/if}
                    <span class="trainer-id-pill">{trainerId}</span>
                  </div>

                  <div class="passport-rank-badge">
                    <span class="rank-star-icon">🎖️</span>
                    <span class="rank-title-text">{trainerTitle}</span>
                  </div>

                  <div class="passport-stats-mini-row">
                    <div class="mini-stat">
                      <span class="stat-lbl">LIFETIME</span>
                      <span class="stat-val">{tokens(c.state?.usedSinceInstall ?? 0)}</span>
                    </div>
                    <div class="mini-stat">
                      <span class="stat-lbl">HATCHED</span>
                      <span class="stat-val">{c.dex.length} Mon</span>
                    </div>
                    <div class="mini-stat">
                      <span class="stat-lbl">RIBBONS</span>
                      <span class="stat-val">{(c.ribbons ?? []).length} / 11</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <!-- 🟩 GitHub-Style Coding Activity Heatmap -->
            <div class="heatmap-section-card">
              <div class="heatmap-header">
                <div class="heatmap-title-box">
                  <span class="heatmap-title-icon">🟩</span>
                  <div>
                    <h3 class="heatmap-heading">Coding Activity Heatmap</h3>
                    <p class="heatmap-subheading">Daily AI token burn history across all providers (past 26 weeks)</p>
                  </div>
                </div>
              </div>

              <!-- Streak & Productivity Metrics Grid -->
              <div class="streak-metrics-grid">
                <div class="streak-card">
                  <span class="streak-icon">🔥</span>
                  <div class="streak-info">
                    <span class="streak-num">{heatmapInfo.currentStreak} Days</span>
                    <span class="streak-label">CURRENT STREAK</span>
                  </div>
                </div>
                <div class="streak-card">
                  <span class="streak-icon">🏆</span>
                  <div class="streak-info">
                    <span class="streak-num">{heatmapInfo.maxStreak} Days</span>
                    <span class="streak-label">BEST STREAK</span>
                  </div>
                </div>
                <div class="streak-card">
                  <span class="streak-icon">📅</span>
                  <div class="streak-info">
                    <span class="streak-num">{heatmapInfo.activeDays} Days</span>
                    <span class="streak-label">ACTIVE DAYS</span>
                  </div>
                </div>
                <div class="streak-card">
                  <span class="streak-icon">⚡</span>
                  <div class="streak-info">
                    <span class="streak-num">{tokens(heatmapInfo.dailyAverage)}</span>
                    <span class="streak-label">DAILY AVG</span>
                  </div>
                </div>
              </div>

              <!-- Pixel Grid Container -->
              <div class="heatmap-grid-scroll pk-scroll">
                <div class="heatmap-grid-inner">
                  <!-- Month labels row -->
                  <div class="heatmap-months-row">
                    <div class="month-spacer"></div>
                    {#each heatmapInfo.weeks as w}
                      <div class="month-col-header">
                        {#if w.monthLabel}
                          <span class="month-text">{w.monthLabel}</span>
                        {/if}
                      </div>
                    {/each}
                  </div>

                  <!-- Days Grid Row (Days on left + Week columns) -->
                  <div class="heatmap-body-row">
                    <div class="heatmap-days-labels">
                      <span>Mon</span>
                      <span>Wed</span>
                      <span>Fri</span>
                    </div>

                    <div class="heatmap-weeks-cols">
                      {#each heatmapInfo.weeks as w}
                        <div class="week-col">
                          {#each w.days as d}
                            <div
                              class="day-cell level-{d.level}"
                              title="{d.formattedDate}: {tokens(d.tokens)} tokens"
                              onmouseenter={(e) => {
                                const rect = e.currentTarget.getBoundingClientRect();
                                hoveredHeatmapCell = {
                                  dateStr: d.dateStr,
                                  tokens: d.tokens,
                                  formattedDate: d.formattedDate,
                                  x: rect.left + rect.width / 2,
                                  y: rect.top,
                                };
                              }}
                              onmouseleave={() => { hoveredHeatmapCell = null; }}
                            ></div>
                          {/each}
                        </div>
                      {/each}
                    </div>
                  </div>

                  <!-- Legend footer -->
                  <div class="heatmap-footer-legend">
                    <span class="legend-text">Less</span>
                    <div class="legend-cell level-0"></div>
                    <div class="legend-cell level-1"></div>
                    <div class="legend-cell level-2"></div>
                    <div class="legend-cell level-3"></div>
                    <div class="legend-cell level-4"></div>
                    <span class="legend-text">More</span>
                  </div>
                </div>
              </div>
            </div>

            <!-- 📜 Trainer PokéJournal & Milestone Feed -->
            <div class="journal-section-card">
              <div class="journal-header">
                <div class="journal-title-box">
                  <span class="journal-icon">📜</span>
                  <div>
                    <h3 class="journal-heading">Trainer’s PokéJournal</h3>
                    <p class="journal-subheading">Chronological record of evolutionary feats, achievements & milestones</p>
                  </div>
                </div>
                <span class="journal-counter">{(c.journal ?? []).length} Memories</span>
              </div>

              <div class="journal-feed pk-scroll">
                {#if (c.journal ?? []).length === 0}
                  <div class="journal-empty">
                    <span class="empty-icon">📖</span>
                    <p class="empty-title">Your journal is ready for memories!</p>
                    <p class="empty-sub">Hatch Pokémon, sprint in Overdrive, feed berries, and earn Ribbons to fill your diary.</p>
                  </div>
                {:else}
                  {#each c.journal ?? [] as item (item.id)}
                    <div class="journal-card kind-{item.kind}">
                      <div class="journal-card-left">
                        <div class="journal-icon-bubble">{item.icon}</div>
                        {#if item.speciesId}
                          <img
                            class="journal-mon-thumb"
                            src={spriteUrl(item.speciesId, Boolean(item.isShiny), true)}
                            alt="Pokémon"
                            onerror={(e) => fallbackStaticSprite(e, item.speciesId ?? 1, Boolean(item.isShiny))}
                          />
                        {/if}
                      </div>
                      <div class="journal-card-body">
                        <div class="journal-card-top">
                          <span class="journal-card-title">{item.title}</span>
                          <span class="journal-card-time">{new Date(item.timestamp).toLocaleDateString("en-US", { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" })}</span>
                        </div>
                        <p class="journal-card-desc">{item.description}</p>
                      </div>
                    </div>
                  {/each}
                {/if}
              </div>
            </div>

          </div>
        {/if}

        {#if currentTab === "arena"}
          {@const b = c.activeBattle}
          {@const stats = c.battleStats ?? { wins: 0, losses: 0, winStreak: 0, bestStreak: 0, totalBattles: 0, totalBpEarned: 0 }}
          {@const activeSpeciesId = c.currentSpeciesId ?? (c.dex[0]?.id ?? 25)}
          {@const chosenFighterId = selectedFighterSpeciesId ?? activeSpeciesId}
          {@const chosenDexMon = c.dex.find(d => d.id === chosenFighterId)}
          {@const fighterName = chosenDexMon?.name ?? (c.displayName || "Pokémon")}
          {@const fighterTypes = getTypes(chosenFighterId)}
          {@const fighterIsShiny = chosenDexMon ? chosenDexMon.isShiny : c.isShiny}

          <div class="tab-pane arena-pane">

            {#if b}
              <!-- ⚔️ Active Turn-Based 1v1 Battle View -->
              <div class="battle-stage-card">
                <div class="battle-arena-glow"></div>

                <!-- Battle Top Header -->
                <div class="battle-stage-header">
                  <div class="battle-header-left">
                    <span class="battle-live-indicator">LIVE</span>
                    <span class="turn-counter">Turn {b.turnCount}</span>
                  </div>
                  <button class="battle-flee-top-btn" onclick={fleeArenaBattle} title="Forfeit / Run from battle">
                    🏃 Forfeit
                  </button>
                </div>

                <!-- Battle Arena Field (Opponent Top Right, Player Bottom Left) -->
                <div class="battle-field">
                  
                  <!-- Opponent Platform (Top Right) -->
                  <div class="combatant-pod opponent-pod" class:combatant-fainted={b.opponent.currentHp === 0}>
                    <div class="combatant-hud">
                      <div class="hud-name-row">
                        <span class="combatant-name">{b.opponent.name}</span>
                        {#if b.opponent.isShiny}<span class="shiny-tag">✨</span>{/if}
                        <span class="combatant-level">Lv. {b.opponent.level}</span>
                      </div>
                      <div class="hud-hp-bar-outer">
                        <div
                          class="hud-hp-bar-fill"
                          class:hp-healthy={b.opponent.currentHp / b.opponent.maxHp > 0.5}
                          class:hp-caution={b.opponent.currentHp / b.opponent.maxHp <= 0.5 && b.opponent.currentHp / b.opponent.maxHp > 0.2}
                          class:hp-danger={b.opponent.currentHp / b.opponent.maxHp <= 0.2}
                          style="width: {Math.max(0, Math.min(100, (b.opponent.currentHp / b.opponent.maxHp) * 100))}%"
                        ></div>
                      </div>
                      <div class="hud-hp-text-row">
                        <span class="hp-num-label">HP</span>
                        <span class="hp-num-values">{b.opponent.currentHp} / {b.opponent.maxHp}</span>
                      </div>
                    </div>

                    <div class="sprite-platform opp-platform">
                      <img
                        class="arena-sprite opponent-sprite"
                        src={spriteUrl(b.opponent.speciesId, b.opponent.isShiny, true)}
                        alt={b.opponent.name}
                        onerror={(e) => fallbackStaticSprite(e, b.opponent.speciesId, b.opponent.isShiny)}
                      />
                      <div class="platform-shadow"></div>
                    </div>
                  </div>

                  <!-- Player Platform (Bottom Left) -->
                  <div class="combatant-pod player-pod" class:combatant-fainted={b.player.currentHp === 0}>
                    <div class="sprite-platform player-platform">
                      <img
                        class="arena-sprite player-sprite"
                        src={spriteUrl(b.player.speciesId, b.player.isShiny, true)}
                        alt={b.player.name}
                        onerror={(e) => fallbackStaticSprite(e, b.player.speciesId, b.player.isShiny)}
                      />
                      <div class="platform-shadow"></div>
                      {#if b.player.isOverdrive}
                        <div class="player-aura-sparkles">⚡ MEGA OVERDRIVE</div>
                      {/if}
                    </div>

                    <div class="combatant-hud player-hud">
                      <div class="hud-name-row">
                        <span class="combatant-name">{b.player.name}</span>
                        {#if b.player.isShiny}<span class="shiny-tag">✨</span>{/if}
                        <span class="combatant-level">Lv. {b.player.level}</span>
                      </div>
                      <div class="hud-hp-bar-outer">
                        <div
                          class="hud-hp-bar-fill"
                          class:hp-healthy={b.player.currentHp / b.player.maxHp > 0.5}
                          class:hp-caution={b.player.currentHp / b.player.maxHp <= 0.5 && b.player.currentHp / b.player.maxHp > 0.2}
                          class:hp-danger={b.player.currentHp / b.player.maxHp <= 0.2}
                          style="width: {Math.max(0, Math.min(100, (b.player.currentHp / b.player.maxHp) * 100))}%"
                        ></div>
                      </div>
                      <div class="hud-hp-text-row">
                        <span class="hp-num-label">HP</span>
                        <span class="hp-num-values">{b.player.currentHp} / {b.player.maxHp}</span>
                      </div>
                    </div>
                  </div>

                </div>

                <!-- Battle Controls & Moves Dashboard -->
                <div class="battle-dashboard">
                  {#if b.battlePhase === "selecting"}
                    <div class="moves-header">
                      <span class="moves-title">What will {b.player.name} do?</span>
                    </div>

                    <div class="moves-grid">
                      {#each b.player.moves as m, idx}
                        {@const elCol = getElementColor(m.element)}
                        <button
                          type="button"
                          class="move-btn"
                          disabled={isExecutingMove || m.currentPp === 0}
                          onclick={() => sendBattleMove(idx)}
                        >
                          <div class="move-btn-top">
                            <span class="move-name">{m.name}</span>
                            <span class="move-type-pill" style="color: {elCol.text}; background: {elCol.bg}; border: {elCol.border};">
                              {m.element}
                            </span>
                          </div>
                          <div class="move-btn-bottom">
                            <span class="move-cat-tag">
                              {m.category === 'Physical' ? '⚔️' : m.category === 'Special' ? '🔮' : '✨'} {m.category}
                            </span>
                            <span class="move-pp-text">PP {m.currentPp}/{m.maxPp}</span>
                            {#if m.power > 0}
                              <span class="move-power-text">{m.power} PWR</span>
                            {/if}
                          </div>
                        </button>
                      {/each}
                    </div>

                  {:else if b.battlePhase === "won"}
                    <div class="battle-result-card victory-card">
                      <div class="result-icon-burst">🎉</div>
                      <h3 class="result-title victory-title">VICTORY!</h3>
                      <p class="result-desc">You defeated {b.opponent.name} and claimed glorious arena spoils!</p>
                      
                      <div class="rewards-pills-row">
                        <span class="reward-pill bp-pill">🏆 +{b.rewardBp} BP</span>
                        <span class="reward-pill coin-pill">🪙 +{b.rewardCoins} Coins</span>
                        <span class="reward-pill ribbon-pill">🏅 Arena Champion</span>
                      </div>

                      <div class="result-actions-row">
                        <button class="result-action-btn next-battle-btn" onclick={() => startArenaBattle()}>
                          ⚔️ Battle Next Opponent ➔
                        </button>
                        <button class="result-action-btn leave-arena-btn" onclick={clearArenaBattle}>
                          Back to Lobby
                        </button>
                      </div>
                    </div>

                  {:else if b.battlePhase === "lost"}
                    <div class="battle-result-card defeat-card">
                      <div class="result-icon-burst">💔</div>
                      <h3 class="result-title defeat-title">DEFEATED</h3>
                      <p class="result-desc">{b.player.name} fainted. You fought hard and gained battle experience!</p>
                      
                      <div class="rewards-pills-row">
                        <span class="reward-pill bp-pill">🏆 +{b.rewardBp} BP Consolation</span>
                        <span class="reward-pill coin-pill">🪙 +{b.rewardCoins} Coins</span>
                      </div>

                      <div class="result-actions-row">
                        <button class="result-action-btn retry-battle-btn" onclick={() => startArenaBattle()}>
                          🔄 Rematch / Try Again
                        </button>
                        <button class="result-action-btn leave-arena-btn" onclick={clearArenaBattle}>
                          Back to Lobby
                        </button>
                      </div>
                    </div>
                  {/if}
                </div>

                <!-- Battle Live Log Terminal -->
                <div class="battle-log-card">
                  <div class="battle-log-header">
                    <span class="log-title">📜 Combat Terminal</span>
                  </div>
                  <div class="battle-log-feed pk-scroll">
                    {#each b.battleLog as logItem (logItem.id)}
                      <div class="log-line actor-{logItem.actor}">
                        <span class="log-bullet">›</span>
                        <span class="log-text">{logItem.text}</span>
                      </div>
                    {/each}
                  </div>
                </div>

              </div>

            {:else}
              <!-- 🏆 Arena Lobby & Roster Preparation View -->
              <div class="arena-lobby-card">
                <div class="arena-hero-banner">
                  <div class="arena-hero-icon">⚔️</div>
                  <div class="arena-hero-texts">
                    <h2 class="arena-heading">Pokémon Battle Arena</h2>
                    <p class="arena-subheading">Turn-based tactical 1v1 duels powered by your coding tokens</p>
                  </div>
                  <div class="arena-bp-pill">
                    <span class="bp-icon">🏆</span>
                    <span class="bp-amount">{c.bp ?? 0} BP</span>
                  </div>
                </div>

                {#if arenaNotice}
                  <div class="arena-notice-banner">
                    <span>✨ {arenaNotice}</span>
                  </div>
                {/if}

                <!-- Selected Combatant Preparation Card -->
                <div class="fighter-prep-card">
                  <div class="prep-header">
                    <span class="prep-title">CHOOSE YOUR CHAMPION</span>
                    <span class="prep-stage-tag">Stage {chosenDexMon ? 3 : getStageInfo(c.stageText, c.isFinalStage).stage}/3</span>
                  </div>

                  <div class="prep-body">
                    <div class="prep-sprite-box">
                      <img
                        class="prep-sprite"
                        src={spriteUrl(chosenFighterId, fighterIsShiny, true)}
                        alt={fighterName}
                        onerror={(e) => fallbackStaticSprite(e, chosenFighterId, fighterIsShiny)}
                      />
                    </div>

                    <div class="prep-info-col">
                      <div class="prep-name-row">
                        <span class="prep-fighter-name">{fighterName}</span>
                        {#if fighterIsShiny}<span class="shiny-tag">✨</span>{/if}
                      </div>

                      <div class="prep-types-row">
                        {#each fighterTypes.list as t}
                          <span class="prep-type-pill" style="color: {t.text}; background: {t.bg}; border: {t.border};">
                            {t.name}
                          </span>
                        {/each}
                        {#if (chosenDexMon?.ribbons ?? c.ribbons ?? []).length > 0}
                          <span class="prep-ribbon-count">
                            🏅 {(chosenDexMon?.ribbons ?? c.ribbons ?? []).length} Ribbons (+{(chosenDexMon?.ribbons ?? c.ribbons ?? []).length * 5}% Power)
                          </span>
                        {/if}
                      </div>

                      <!-- Fighter Select Dropdown / Selector -->
                      <div class="fighter-select-dropdown-box">
                        <label for="fighter-select" class="dropdown-lbl">Active Roster:</label>
                        <select
                          id="fighter-select"
                          class="fighter-select-dropdown"
                          bind:value={selectedFighterSpeciesId}
                        >
                          {#if c.hasActive && c.currentSpeciesId}
                            <option value={c.currentSpeciesId}>🌟 Active Buddy: {c.displayName}</option>
                          {/if}
                          {#each c.dex as d}
                            {#if !c.currentSpeciesId || d.id !== c.currentSpeciesId}
                              <option value={d.id}>{d.name} {d.isShiny ? '✨' : ''} (Pokédex)</option>
                            {/if}
                          {/each}
                        </select>
                      </div>
                    </div>
                  </div>

                  <!-- Action Matchmaking Button -->
                  <button
                    type="button"
                    class="start-battle-main-btn"
                    onclick={() => startArenaBattle()}
                  >
                    <span class="btn-sword-icon">⚔️</span>
                    <span>ENTER BATTLE ARENA</span>
                  </button>
                </div>

                <!-- Arena Career Stats Grid -->
                <div class="arena-stats-grid">
                  <div class="arena-stat-box">
                    <span class="astat-val">{stats.wins}</span>
                    <span class="astat-lbl">VICTORIES</span>
                  </div>
                  <div class="arena-stat-box">
                    <span class="astat-val">{stats.losses}</span>
                    <span class="astat-lbl">DEFEATS</span>
                  </div>
                  <div class="arena-stat-box">
                    <span class="astat-val">{stats.totalBattles > 0 ? Math.round((stats.wins / stats.totalBattles) * 100) : 0}%</span>
                    <span class="astat-lbl">WIN RATE</span>
                  </div>
                  <div class="arena-stat-box highlight-streak">
                    <span class="astat-val">🔥 {stats.winStreak}</span>
                    <span class="astat-lbl">STREAK</span>
                  </div>
                  <div class="arena-stat-box">
                    <span class="astat-val">🏆 {stats.bestStreak}</span>
                    <span class="astat-lbl">BEST STREAK</span>
                  </div>
                  <div class="arena-stat-box">
                    <span class="astat-val">👑 {stats.totalBpEarned}</span>
                    <span class="astat-lbl">LIFETIME BP</span>
                  </div>
                </div>

                <!-- Battle Points (BP) Rewards Exchange -->
                <div class="bp-exchange-section">
                  <div class="bpx-header">
                    <span class="bpx-title">🏆 Battle Points (BP) Exchange Shelf</span>
                    <span class="bpx-sub">Redeem arena trophies for premium rare items</span>
                  </div>

                  <div class="bpx-grid">
                    <div class="bpx-card">
                      <div class="bpx-icon">🍬</div>
                      <div class="bpx-info">
                        <span class="bpx-name">Rare Candy</span>
                        <span class="bpx-desc">+10M Evolutionary XP</span>
                      </div>
                      <button
                        class="bpx-buy-btn"
                        disabled={(c.bp ?? 0) < 20}
                        onclick={() => buyBpShopItem("rareCandy")}
                      >
                        20 BP
                      </button>
                    </div>

                    <div class="bpx-card">
                      <div class="bpx-icon">🍃</div>
                      <div class="bpx-info">
                        <span class="bpx-name">Nature Mint</span>
                        <span class="bpx-desc">Re-roll nature buffs</span>
                      </div>
                      <button
                        class="bpx-buy-btn"
                        disabled={(c.bp ?? 0) < 15}
                        onclick={() => buyBpShopItem("mint")}
                      >
                        15 BP
                      </button>
                    </div>

                    <div class="bpx-card">
                      <div class="bpx-icon">🍊</div>
                      <div class="bpx-info">
                        <span class="bpx-name">Oran Berry</span>
                        <span class="bpx-desc">+15M XP treat</span>
                      </div>
                      <button
                        class="bpx-buy-btn"
                        disabled={(c.bp ?? 0) < 15}
                        onclick={() => buyBpShopItem("oranBerry")}
                      >
                        15 BP
                      </button>
                    </div>

                    <div class="bpx-card">
                      <div class="bpx-icon">🌟</div>
                      <div class="bpx-info">
                        <span class="bpx-name">Sitrus Berry</span>
                        <span class="bpx-desc">+50M XP + Golden Sparkle Aura</span>
                      </div>
                      <button
                        class="bpx-buy-btn"
                        disabled={(c.bp ?? 0) < 35}
                        onclick={() => buyBpShopItem("sitrusBerry")}
                      >
                        35 BP
                      </button>
                    </div>

                    <div class="bpx-card">
                      <div class="bpx-icon">💫</div>
                      <div class="bpx-info">
                        <span class="bpx-name">Shiny Charm</span>
                        <span class="bpx-desc">Permanent 1/48 shiny rate</span>
                      </div>
                      <button
                        class="bpx-buy-btn"
                        disabled={(c.bp ?? 0) < 100}
                        onclick={() => buyBpShopItem("shinyCharm")}
                      >
                        100 BP
                      </button>
                    </div>

                    <div class="bpx-card">
                      <div class="bpx-icon">🔮</div>
                      <div class="bpx-info">
                        <span class="bpx-name">Epic Egg</span>
                        <span class="bpx-desc">Guaranteed 3-stage powerhouse</span>
                      </div>
                      <button
                        class="bpx-buy-btn"
                        disabled={(c.bp ?? 0) < 50}
                        onclick={() => buyBpShopItem("epicEgg")}
                      >
                        50 BP
                      </button>
                    </div>

                    <div class="bpx-card">
                      <div class="bpx-icon">👑</div>
                      <div class="bpx-info">
                        <span class="bpx-name">Legendary Egg</span>
                        <span class="bpx-desc">Ultra-rare mythical creature</span>
                      </div>
                      <button
                        class="bpx-buy-btn"
                        disabled={(c.bp ?? 0) < 120}
                        onclick={() => buyBpShopItem("legendaryEgg")}
                      >
                        120 BP
                      </button>
                    </div>
                  </div>
                </div>

              </div>
            {/if}

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

            <!-- Dex Pokémon Ribbons -->
            {#if (selectedDexMon.ribbons ?? []).length > 0}
              <div
                class="dex-ribbons-card"
                onclick={() => openRibbonModal(selectedDexMon?.name ?? "Pokémon", selectedDexMon?.ribbons ?? [])}
                role="button"
                tabindex="0"
                onkeydown={(e) => e.key === 'Enter' && openRibbonModal(selectedDexMon?.name ?? "Pokémon", selectedDexMon?.ribbons ?? [])}
                title="Click to view full Ribbon Case"
              >
                <div class="dex-ribbons-header">
                  <span class="dex-stat-label">EARNED RIBBONS ({(selectedDexMon.ribbons ?? []).length})</span>
                  <span class="dex-ribbons-case-btn">Open Case ➔</span>
                </div>
                <div class="dex-ribbons-pills">
                  {#each selectedDexMon.ribbons ?? [] as rId}
                    {@const r = getRibbon(rId)}
                    {#if r}
                      <span class="dex-ribbon-tag" style="--rib-color: {r.color}; --rib-glow: {r.glow};">
                        <span class="drt-icon">{r.icon}</span>
                        <span class="drt-text">{r.name}</span>
                      </span>
                    {/if}
                  {/each}
                </div>
              </div>
            {/if}
          </div>
        </div>
      </div>
    {/if}

    <!-- Ribbon Case & Achievements Modal -->
    {#if showRibbonModal}
      <div
        class="modal-backdrop"
        onclick={() => { showRibbonModal = false; }}
        role="button"
        tabindex="0"
        onkeydown={(e) => e.key === 'Escape' && (showRibbonModal = false)}
      >
        <div class="modal-card ribbon-modal" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true">
          <div class="modal-header">
            <div class="ribbon-modal-title-box">
              <span class="ribbon-modal-icon">🏅</span>
              <div class="ribbon-modal-title-texts">
                <h3 class="modal-title">Ribbon Case</h3>
                <p class="modal-subtitle">{selectedRibbonMonName}’s Honors ({selectedRibbonList.length}/{ORDERED_RIBBON_IDS.length} Unlocked)</p>
              </div>
            </div>
            <button class="modal-close" onclick={() => { showRibbonModal = false; }}>✕</button>
          </div>

          <div class="ribbons-grid pk-scroll">
            {#each ORDERED_RIBBON_IDS as rId}
              {@const rib = getRibbon(rId)}
              {#if rib}
                {@const isUnlocked = selectedRibbonList.includes(rId)}
                <div class="ribbon-card" class:unlocked={isUnlocked} class:locked={!isUnlocked} style="--rib-color: {rib.color}; --rib-glow: {rib.glow};">
                  <div class="ribbon-card-icon-box">
                    <span class="ribbon-card-icon">{rib.icon}</span>
                    {#if isUnlocked}
                      <span class="ribbon-check-badge" title="Unlocked!">✓</span>
                    {/if}
                  </div>
                  <div class="ribbon-card-info">
                    <div class="ribbon-card-header">
                      <span class="ribbon-card-name">{rib.name}</span>
                      <span class="ribbon-card-tag">{rib.badge}</span>
                    </div>
                    <span class="ribbon-card-title">{rib.title}</span>
                    <p class="ribbon-card-desc">{rib.description}</p>
                    <div class="ribbon-status-row">
                      {#if isUnlocked}
                        <span class="ribbon-unlocked-pill">✨ Unlocked & Honored</span>
                      {:else}
                        <span class="ribbon-locked-pill">🔒 Locked Milestone</span>
                      {/if}
                    </div>
                  </div>
                </div>
              {/if}
            {/each}
          </div>
        </div>
      </div>
    {/if}

    <!-- Avatar Picker Modal -->
    {#if showAvatarPickerModal}
      <div
        class="modal-backdrop"
        onclick={() => { showAvatarPickerModal = false; }}
        role="button"
        tabindex="0"
        onkeydown={(e) => e.key === 'Escape' && (showAvatarPickerModal = false)}
      >
        <div class="modal-card avatar-modal" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true">
          <div class="modal-header">
            <div class="ribbon-modal-title-box">
              <span class="ribbon-modal-icon">🪪</span>
              <div class="ribbon-modal-title-texts">
                <h3 class="modal-title">Choose Trainer Avatar</h3>
                <p class="modal-subtitle">Select any Pokémon from your registered Pokédex</p>
              </div>
            </div>
            <button class="modal-close" onclick={() => { showAvatarPickerModal = false; }}>✕</button>
          </div>

          <div class="avatar-grid-select pk-scroll">
            <button class="avatar-select-card default-active-opt" onclick={() => chooseAvatar(null)} type="button">
              <span class="avatar-opt-title">🌟 Follow Active Partner</span>
              <span class="avatar-opt-sub">Automatically matches your currently raised companion</span>
            </button>
            {#each snap?.companion.dex ?? [] as d}
              <button class="avatar-select-card" onclick={() => chooseAvatar(d.id)} type="button">
                <img
                  class="avatar-opt-sprite"
                  src={spriteUrl(d.id, d.isShiny, true)}
                  alt={d.name}
                  onerror={(e) => fallbackStaticSprite(e, d.id, d.isShiny)}
                />
                <span class="avatar-opt-name">
                  {d.name}
                  {#if d.isShiny}<span class="shiny-star">✨</span>{/if}
                </span>
              </button>
            {/each}
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

  .pk-scroll {
    scrollbar-width: thin;
    scrollbar-color: rgba(255, 255, 255, 0.16) transparent;
  }
  .pk-scroll::-webkit-scrollbar {
    width: 6px;
    height: 6px;
  }
  .pk-scroll::-webkit-scrollbar-track {
    background: rgba(0, 0, 0, 0.15);
    border-radius: 999px;
  }
  .pk-scroll::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.16);
    border-radius: 999px;
  }
  .pk-scroll::-webkit-scrollbar-thumb:hover {
    background: rgba(255, 255, 255, 0.28);
  }
  .pk-scroll::-webkit-scrollbar-corner {
    background: transparent;
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

  .hero-actions-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .pet-buddy-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 9px;
    border-radius: 999px;
    background: rgba(255, 107, 129, 0.16);
    border: 1px solid rgba(255, 107, 129, 0.35);
    color: #FFA5B4;
    font-size: 11px;
    font-weight: 700;
    cursor: pointer;
    transition: all 0.15s cubic-bezier(0.34, 1.56, 0.64, 1);
  }

  .pet-buddy-btn:hover {
    background: rgba(255, 107, 129, 0.28);
    border-color: rgba(255, 107, 129, 0.6);
    color: #FFF;
    transform: scale(1.05);
    box-shadow: 0 0 10px rgba(255, 107, 129, 0.35);
  }

  .pet-buddy-btn:active {
    transform: scale(0.95);
  }

  /* Keyframe Animations */
  @keyframes petHop {
    0% { transform: translateY(0) scale(1, 1); }
    20% { transform: translateY(2px) scale(1.15, 0.85); }
    45% { transform: translateY(-16px) scale(0.9, 1.15); }
    70% { transform: translateY(-4px) scale(1.05, 0.95); }
    100% { transform: translateY(0) scale(1, 1); }
  }

  @keyframes petWiggle {
    0%, 100% { transform: rotate(0deg) scale(1); }
    20% { transform: rotate(-12deg) scale(1.05); }
    40% { transform: rotate(10deg) scale(1.05); }
    60% { transform: rotate(-8deg) scale(1.02); }
    80% { transform: rotate(6deg) scale(1.02); }
  }

  @keyframes wakePop {
    0% { transform: scale(0.85) translateY(4px); }
    40% { transform: scale(1.25) translateY(-12px); }
    70% { transform: scale(0.95) translateY(0); }
    100% { transform: scale(1) translateY(0); }
  }

  @keyframes eatingNom {
    0%, 100% { transform: scale(1); }
    25% { transform: scale(1.12, 0.9) translateY(-2px); }
    50% { transform: scale(0.92, 1.08) translateY(-4px); }
    75% { transform: scale(1.08, 0.94) translateY(-1px); }
  }

  @keyframes sleepBreath {
    0%, 100% { transform: translateY(2px) scale(0.96) rotate(-2deg); opacity: 0.85; }
    50% { transform: translateY(0px) scale(1) rotate(1deg); opacity: 0.95; }
  }

  @keyframes floatZzz {
    0% { opacity: 0; transform: translate(0, 0) scale(0.6); }
    25% { opacity: 0.9; }
    75% { opacity: 0.7; }
    100% { opacity: 0; transform: translate(14px, -24px) scale(1.1); }
  }

  @keyframes floatSparkle {
    0%, 100% { transform: scale(0.7) rotate(0deg); opacity: 0.4; }
    50% { transform: scale(1.2) rotate(180deg); opacity: 1; }
  }

  @keyframes heartFly {
    0% {
      opacity: 1;
      transform: translate(0, 0) scale(0.6);
    }
    50% {
      opacity: 1;
      transform: translate(var(--target-x), calc(var(--target-y) * 0.6)) scale(var(--scale));
    }
    100% {
      opacity: 0;
      transform: translate(var(--target-x), var(--target-y)) scale(calc(var(--scale) * 1.2));
    }
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

  /* Mega Overdrive & Gigantamax Surge */
  .buddy-hero.overdrive-hero {
    border-color: rgba(255, 0, 122, 0.45);
    box-shadow: 0 0 25px rgba(255, 0, 122, 0.25), inset 0 0 16px rgba(0, 229, 255, 0.12);
    animation: overdriveHeroPulse 2.8s infinite ease-in-out;
  }

  @keyframes overdriveHeroPulse {
    0%, 100% {
      border-color: rgba(255, 0, 122, 0.45);
      box-shadow: 0 0 25px rgba(255, 0, 122, 0.25), inset 0 0 16px rgba(0, 229, 255, 0.12);
    }
    50% {
      border-color: rgba(0, 229, 255, 0.55);
      box-shadow: 0 0 35px rgba(0, 229, 255, 0.35), inset 0 0 20px rgba(255, 0, 122, 0.2);
    }
  }

  .hero-overdrive-pill {
    font-size: 10px;
    font-weight: 800;
    padding: 2px 8px;
    border-radius: 999px;
    background: linear-gradient(90deg, #FF007A, #7928CA, #0070F3);
    color: #FFFFFF;
    letter-spacing: 0.04em;
    box-shadow: 0 0 12px rgba(255, 0, 122, 0.6);
    animation: pulseMegaBadge 1.8s infinite ease-in-out;
  }

  @keyframes pulseMegaBadge {
    0%, 100% { transform: scale(1); filter: brightness(1); }
    50% { transform: scale(1.06); filter: brightness(1.25); }
  }

  .hero-sprite.overdrive-sprite {
    filter: drop-shadow(0 0 16px rgba(255, 0, 122, 0.75)) drop-shadow(0 0 24px rgba(0, 229, 255, 0.6)) !important;
  }

  .hero-overdrive-sparks {
    position: absolute;
    inset: -8px;
    pointer-events: none;
  }

  .od-spark {
    position: absolute;
    font-size: 15px;
    animation: sparkFlash 1.4s infinite ease-in-out;
    filter: drop-shadow(0 0 8px #FF007A);
  }
  .od-spark.od-1 { top: -2px; left: 4px; animation-delay: 0s; }
  .od-spark.od-2 { top: 2px; right: -2px; animation-delay: 0.5s; font-size: 13px; }
  .od-spark.od-3 { bottom: -2px; left: 24px; animation-delay: 0.9s; font-size: 14px; }

  @keyframes sparkFlash {
    0%, 100% { opacity: 0.2; transform: scale(0.7) translateY(0); }
    50% { opacity: 1; transform: scale(1.2) translateY(-4px); }
  }

  /* 2x Coin Rush Banner */
  .coin-rush-banner-top {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-radius: 12px;
    background: linear-gradient(90deg, rgba(255, 234, 0, 0.14), rgba(255, 107, 0, 0.2));
    border: 1px solid rgba(255, 234, 0, 0.45);
    box-shadow: 0 0 16px rgba(255, 234, 0, 0.15);
    animation: goldPulseText 2s infinite ease-in-out;
  }

  .coin-rush-banner-top .rush-icon {
    font-size: 16px;
    filter: drop-shadow(0 0 6px #FFEA00);
  }

  .coin-rush-banner-top .rush-text-col {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .coin-rush-banner-top .rush-title {
    font-size: 11px;
    font-weight: 800;
    color: #FFEA00;
    letter-spacing: 0.05em;
  }

  .coin-rush-banner-top .rush-sub {
    font-size: 9.5px;
    font-weight: 500;
    color: rgba(255, 255, 255, 0.7);
  }

  .wallet-card.overdrive-wallet {
    border-color: rgba(255, 234, 0, 0.4);
    box-shadow: 0 0 20px rgba(255, 234, 0, 0.18);
  }

  /* 🏅 Ribbons & Achievement Case */
  .hero-ribbons-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    margin-top: 4px;
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    cursor: pointer;
    transition: all 0.2s ease;
  }
  .hero-ribbons-bar:hover {
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(252, 211, 77, 0.35);
    transform: translateY(-1px);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.25);
  }
  .hero-ribbons-bar:active {
    transform: scale(0.98);
  }

  .ribbons-left {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }
  .ribbon-bar-icon {
    font-size: 13px;
  }
  .ribbon-count-text {
    font-size: 10px;
    font-weight: 700;
    color: #8B93A7;
    letter-spacing: 0.04em;
  }

  .ribbons-pills-row {
    display: flex;
    align-items: center;
    gap: 5px;
    overflow: hidden;
    flex: 1;
  }
  .hero-mini-ribbon {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 2px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--rib-color, #fff) 12%, rgba(0,0,0,0.3));
    border: 1px solid color-mix(in srgb, var(--rib-color, #fff) 35%, transparent);
    color: var(--rib-color, #fff);
    font-size: 9.5px;
    font-weight: 700;
    white-space: nowrap;
    box-shadow: 0 0 8px var(--rib-glow, transparent);
  }
  .hero-mini-ribbon.more-ribbons {
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(255, 255, 255, 0.15);
    color: #B0B7C6;
  }
  .ribbons-arrow {
    font-size: 10px;
    color: #8B93A7;
    margin-left: auto;
    transition: transform 0.2s ease;
  }
  .hero-ribbons-bar:hover .ribbons-arrow {
    color: #FCD34D;
    transform: translateX(2px);
  }

  /* Dex Ribbons Section */
  .dex-ribbons-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.07);
    cursor: pointer;
    transition: all 0.2s ease;
  }
  .dex-ribbons-card:hover {
    background: rgba(255, 255, 255, 0.06);
    border-color: rgba(252, 211, 77, 0.3);
  }
  .dex-ribbons-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .dex-ribbons-case-btn {
    font-size: 10px;
    font-weight: 700;
    color: #FCD34D;
  }
  .dex-ribbons-pills {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .dex-ribbon-tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 8px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--rib-color, #fff) 12%, rgba(0,0,0,0.3));
    border: 1px solid color-mix(in srgb, var(--rib-color, #fff) 35%, transparent);
    color: var(--rib-color, #fff);
    font-size: 10px;
    font-weight: 700;
    box-shadow: 0 0 8px var(--rib-glow, transparent);
  }

  /* Generic Modal Dialogs (Ribbon Case & Avatar Picker) */
  .modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 999;
    background: rgba(4, 6, 10, 0.82);
    backdrop-filter: blur(14px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    animation: fadeIn 0.2s ease;
  }

  .modal-card {
    position: relative;
    width: 100%;
    max-width: 440px;
    background: linear-gradient(180deg, #181C26 0%, #0E1017 100%);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 20px;
    padding: 20px 18px 18px;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.85), 0 0 30px rgba(255, 215, 0, 0.12);
    display: flex;
    flex-direction: column;
    animation: modalPop 0.25s cubic-bezier(0.16, 1, 0.3, 1);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-bottom: 12px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  }

  .modal-title {
    font-size: 15px;
    font-weight: 800;
    color: #F8FAFC;
    margin: 0;
  }

  .modal-subtitle {
    font-size: 11px;
    color: #8B93A7;
    margin: 2px 0 0 0;
  }

  .modal-close {
    width: 28px;
    height: 28px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: #94A3B8;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    font-size: 13px;
    transition: all 0.2s ease;
  }
  .modal-close:hover {
    background: rgba(255, 255, 255, 0.15);
    color: #F8FAFC;
    transform: scale(1.05);
  }

  /* Ribbon Case Modal */
  .modal-card.ribbon-modal {
    max-width: 520px;
    max-height: 82vh;
    display: flex;
    flex-direction: column;
    padding: 20px;
  }

  .ribbon-modal-title-box {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .ribbon-modal-icon {
    font-size: 26px;
    filter: drop-shadow(0 0 10px #FCD34D);
  }
  .ribbon-modal-title-texts {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .ribbons-grid {
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow-y: auto;
    padding-right: 4px;
    margin-top: 14px;
  }

  .ribbon-card {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    padding: 12px 14px;
    border-radius: 14px;
    transition: all 0.2s ease;
  }

  .ribbon-card.unlocked {
    background: linear-gradient(135deg, color-mix(in srgb, var(--rib-color, #fff) 12%, rgba(18,20,27,0.9)), rgba(18,20,27,0.95));
    border: 1px solid color-mix(in srgb, var(--rib-color, #fff) 40%, transparent);
    box-shadow: 0 0 16px var(--rib-glow, transparent);
  }

  .ribbon-card.locked {
    background: rgba(255, 255, 255, 0.02);
    border: 1px dashed rgba(255, 255, 255, 0.1);
    opacity: 0.6;
  }
  .ribbon-card.locked:hover {
    opacity: 0.85;
    background: rgba(255, 255, 255, 0.04);
  }

  .ribbon-card-icon-box {
    position: relative;
    width: 44px;
    height: 44px;
    border-radius: 12px;
    background: rgba(0, 0, 0, 0.35);
    border: 1px solid rgba(255, 255, 255, 0.1);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .ribbon-card.unlocked .ribbon-card-icon-box {
    border-color: color-mix(in srgb, var(--rib-color, #fff) 50%, transparent);
    background: color-mix(in srgb, var(--rib-color, #fff) 15%, rgba(0,0,0,0.5));
  }
  .ribbon-card-icon {
    font-size: 22px;
  }
  .ribbon-check-badge {
    position: absolute;
    bottom: -4px;
    right: -4px;
    width: 16px;
    height: 16px;
    border-radius: 999px;
    background: #39D98A;
    color: #000;
    font-size: 9.5px;
    font-weight: 900;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 0 6px #39D98A;
  }

  .ribbon-card-info {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
  }
  .ribbon-card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .ribbon-card-name {
    font-size: 13px;
    font-weight: 800;
    color: #F2F3F5;
  }
  .ribbon-card.unlocked .ribbon-card-name {
    color: var(--rib-color, #F2F3F5);
  }
  .ribbon-card-tag {
    font-size: 10px;
    font-weight: 700;
    padding: 1px 6px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.06);
    color: #8B93A7;
  }
  .ribbon-card.unlocked .ribbon-card-tag {
    background: color-mix(in srgb, var(--rib-color, #fff) 20%, transparent);
    color: var(--rib-color, #F2F3F5);
  }

  .ribbon-card-title {
    font-size: 10.5px;
    font-weight: 600;
    font-style: italic;
    color: #B0B7C6;
  }
  .ribbon-card-desc {
    font-size: 11px;
    color: #8B93A7;
    margin: 2px 0 0 0;
    line-height: 1.4;
  }

  .ribbon-status-row {
    margin-top: 4px;
  }
  .ribbon-unlocked-pill {
    font-size: 9.5px;
    font-weight: 700;
    color: #39D98A;
  }
  .ribbon-locked-pill {
    font-size: 9.5px;
    font-weight: 600;
    color: #64748B;
  }

  /* 🪪 Trainer Passport & Activity Tab */
  .trainer-pane {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  /* Holographic Trainer Passport Card */
  .trainer-passport-card {
    position: relative;
    overflow: hidden;
    padding: 16px;
    border-radius: 16px;
    background: linear-gradient(135deg, rgba(26, 32, 48, 0.95), rgba(15, 18, 26, 0.98));
    border: 1.5px solid rgba(255, 215, 0, 0.35);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.45), 0 0 20px rgba(255, 215, 0, 0.12);
  }

  .passport-foil-glow {
    position: absolute;
    inset: 0;
    background: linear-gradient(125deg, rgba(255,255,255,0.06) 0%, rgba(255,215,0,0.12) 35%, rgba(0,229,255,0.12) 70%, rgba(255,0,122,0.06) 100%);
    pointer-events: none;
    opacity: 0.8;
  }

  .passport-header {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-bottom: 10px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    margin-bottom: 14px;
  }
  .passport-title-badge {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .passport-icon {
    font-size: 15px;
  }
  .passport-title-text {
    font-size: 11px;
    font-weight: 900;
    letter-spacing: 0.08em;
    color: #FCD34D;
    text-shadow: 0 0 8px rgba(252, 211, 77, 0.4);
  }
  .passport-region-badge {
    font-size: 9px;
    font-weight: 800;
    letter-spacing: 0.06em;
    color: #94A3B8;
    padding: 2px 7px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .passport-body {
    position: relative;
    display: flex;
    gap: 16px;
    align-items: center;
  }

  /* Avatar Frame */
  .passport-avatar-box {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }
  .avatar-frame {
    position: relative;
    width: 72px;
    height: 72px;
    border-radius: 16px;
    background: rgba(0, 0, 0, 0.45);
    border: 2px solid rgba(255, 215, 0, 0.4);
    box-shadow: 0 0 16px rgba(255, 215, 0, 0.2);
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    cursor: pointer;
    transition: all 0.2s ease;
  }
  .avatar-frame:hover {
    transform: scale(1.04);
    border-color: #FCD34D;
    box-shadow: 0 0 20px rgba(255, 215, 0, 0.4);
  }
  .avatar-sprite-img {
    width: 60px;
    height: 60px;
    object-fit: contain;
    filter: drop-shadow(0 2px 6px rgba(0, 0, 0, 0.4));
  }
  .avatar-edit-overlay {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.65);
    color: #FCD34D;
    font-size: 10px;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    transition: opacity 0.2s ease;
  }
  .avatar-frame:hover .avatar-edit-overlay {
    opacity: 1;
  }
  .avatar-helper-text {
    font-size: 9.5px;
    font-weight: 700;
    color: #8B93A7;
  }

  /* Passport info col */
  .passport-info-col {
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex: 1;
    min-width: 0;
  }

  .passport-name-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .trainer-name-display {
    display: flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    border: none;
    padding: 2px 6px;
    margin: -2px -6px;
    border-radius: 8px;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s ease;
  }
  .trainer-name-display:hover {
    background: rgba(255, 255, 255, 0.08);
  }
  .trainer-name-display:hover .edit-pencil-icon {
    color: #FCD34D;
  }
  .trainer-nickname {
    font-size: 16px;
    font-weight: 900;
    color: #F8FAFC;
  }
  .edit-pencil-icon {
    font-size: 12px;
    color: #64748B;
    transition: color 0.2s ease;
  }

  .name-edit-box {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: 1;
  }
  .trainer-name-input {
    background: rgba(0, 0, 0, 0.5);
    border: 1px solid rgba(255, 215, 0, 0.5);
    color: #F8FAFC;
    padding: 3px 8px;
    font-size: 13px;
    font-weight: 700;
    border-radius: 6px;
    width: 130px;
    outline: none;
  }
  .save-name-btn, .cancel-name-btn {
    padding: 3px 6px;
    font-size: 11px;
    font-weight: 800;
    border-radius: 6px;
    cursor: pointer;
    border: none;
  }
  .save-name-btn { background: #39D98A; color: #000; }
  .cancel-name-btn { background: rgba(255,255,255,0.1); color: #fff; }

  .trainer-id-pill {
    font-size: 10px;
    font-weight: 800;
    font-family: 'JetBrains Mono', monospace;
    padding: 2px 7px;
    border-radius: 6px;
    background: rgba(0, 0, 0, 0.35);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: #94A3B8;
  }

  .passport-rank-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 8px;
    border-radius: 8px;
    background: rgba(252, 211, 77, 0.1);
    border: 1px solid rgba(252, 211, 77, 0.3);
    width: fit-content;
  }
  .rank-star-icon { font-size: 12px; }
  .rank-title-text {
    font-size: 10.5px;
    font-weight: 800;
    color: #FCD34D;
    letter-spacing: 0.02em;
  }

  .passport-stats-mini-row {
    display: flex;
    gap: 8px;
    margin-top: 4px;
  }
  .mini-stat {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 4px 8px;
    border-radius: 8px;
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid rgba(255, 255, 255, 0.05);
    flex: 1;
  }
  .mini-stat .stat-lbl {
    font-size: 8.5px;
    font-weight: 700;
    color: #8B93A7;
    letter-spacing: 0.05em;
  }
  .mini-stat .stat-val {
    font-size: 11px;
    font-weight: 800;
    color: #E2E8F0;
  }

  /* 🟩 GitHub-Style Heatmap Section */
  .heatmap-section-card {
    padding: 14px;
    border-radius: 14px;
    background: rgba(255, 255, 255, 0.025);
    border: 1px solid rgba(255, 255, 255, 0.06);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .heatmap-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .heatmap-title-box {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .heatmap-title-icon { font-size: 16px; }
  .heatmap-heading {
    font-size: 13px;
    font-weight: 800;
    color: #F8FAFC;
    margin: 0;
  }
  .heatmap-subheading {
    font-size: 10px;
    color: #8B93A7;
    margin: 2px 0 0 0;
  }

  /* Streak Metrics Grid */
  .streak-metrics-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
  }
  .streak-card {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: 10px;
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid rgba(255, 255, 255, 0.05);
  }
  .streak-icon { font-size: 15px; }
  .streak-info {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .streak-num {
    font-size: 11.5px;
    font-weight: 800;
    color: #F1F5F9;
  }
  .streak-label {
    font-size: 8px;
    font-weight: 700;
    color: #8B93A7;
    letter-spacing: 0.05em;
  }

  /* Pixel Grid Container */
  .heatmap-grid-scroll {
    overflow-x: auto;
    padding-bottom: 4px;
  }
  .heatmap-grid-inner {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 480px;
  }

  .heatmap-months-row {
    display: flex;
    gap: 3px;
    height: 12px;
  }
  .month-spacer {
    width: 24px;
    flex-shrink: 0;
  }
  .month-col-header {
    width: 12px;
    flex-shrink: 0;
    position: relative;
  }
  .month-text {
    position: absolute;
    left: 0;
    top: 0;
    font-size: 9px;
    font-weight: 700;
    color: #8B93A7;
    white-space: nowrap;
  }

  .heatmap-body-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .heatmap-days-labels {
    width: 18px;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    height: 88px;
    font-size: 8px;
    font-weight: 700;
    color: #64748B;
    padding: 1px 0;
  }

  .heatmap-weeks-cols {
    display: flex;
    gap: 3px;
  }
  .week-col {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .day-cell {
    width: 11px;
    height: 11px;
    border-radius: 2.5px;
    transition: transform 0.15s ease, box-shadow 0.15s ease;
    cursor: pointer;
  }
  .day-cell:hover {
    transform: scale(1.3);
    z-index: 5;
    box-shadow: 0 0 6px rgba(255, 255, 255, 0.4);
  }

  .day-cell.level-0 {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.03);
  }
  .day-cell.level-1 {
    background: #0E4429;
    border: 1px solid #166534;
  }
  .day-cell.level-2 {
    background: #006D32;
    border: 1px solid #15803D;
  }
  .day-cell.level-3 {
    background: #26A641;
    border: 1px solid #22C55E;
  }
  .day-cell.level-4 {
    background: #39D353;
    border: 1px solid #4ADE80;
    box-shadow: 0 0 4px #39D353;
  }

  .heatmap-footer-legend {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
    margin-top: 6px;
  }
  .legend-text {
    font-size: 9px;
    font-weight: 600;
    color: #8B93A7;
  }
  .legend-cell {
    width: 10px;
    height: 10px;
    border-radius: 2px;
  }

  /* 📜 Trainer PokéJournal Feed */
  .journal-section-card {
    padding: 14px;
    border-radius: 14px;
    background: rgba(255, 255, 255, 0.025);
    border: 1px solid rgba(255, 255, 255, 0.06);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .journal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .journal-title-box {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .journal-icon { font-size: 16px; }
  .journal-heading {
    font-size: 13px;
    font-weight: 800;
    color: #F8FAFC;
    margin: 0;
  }
  .journal-subheading {
    font-size: 10px;
    color: #8B93A7;
    margin: 2px 0 0 0;
  }
  .journal-counter {
    font-size: 10px;
    font-weight: 700;
    color: #FCD34D;
    padding: 2px 8px;
    border-radius: 999px;
    background: rgba(252, 211, 77, 0.1);
    border: 1px solid rgba(252, 211, 77, 0.25);
  }

  .journal-feed {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 320px;
    overflow-y: auto;
    padding-right: 4px;
  }

  .journal-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 28px 16px;
    text-align: center;
  }
  .journal-empty .empty-icon { font-size: 28px; margin-bottom: 6px; }
  .journal-empty .empty-title { font-size: 12px; font-weight: 800; color: #E2E8F0; margin: 0; }
  .journal-empty .empty-sub { font-size: 10.5px; color: #8B93A7; margin: 4px 0 0 0; max-width: 280px; }

  .journal-card {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 10px 12px;
    border-radius: 12px;
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid rgba(255, 255, 255, 0.05);
    transition: all 0.2s ease;
  }
  .journal-card:hover {
    background: rgba(255, 255, 255, 0.04);
    border-color: rgba(255, 255, 255, 0.1);
    transform: translateY(-1px);
  }

  .journal-card-left {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }
  .journal-icon-bubble {
    width: 28px;
    height: 28px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 13px;
  }
  .journal-mon-thumb {
    width: 24px;
    height: 24px;
    object-fit: contain;
  }

  .journal-card-body {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }
  .journal-card-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .journal-card-title {
    font-size: 12px;
    font-weight: 800;
    color: #F1F5F9;
  }
  .journal-card-time {
    font-size: 9.5px;
    font-weight: 600;
    color: #8B93A7;
    white-space: nowrap;
  }
  .journal-card-desc {
    font-size: 10.5px;
    color: #94A3B8;
    margin: 0;
    line-height: 1.35;
  }

  /* Avatar Picker Modal */
  .modal-card.avatar-modal {
    max-width: 440px;
    max-height: 75vh;
    display: flex;
    flex-direction: column;
  }
  .avatar-grid-select {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
    overflow-y: auto;
    margin-top: 14px;
    padding-right: 4px;
  }
  .avatar-select-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 10px;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.08);
    cursor: pointer;
    transition: all 0.2s ease;
  }
  .avatar-select-card:hover {
    background: rgba(255, 255, 255, 0.08);
    border-color: #FCD34D;
    transform: translateY(-2px);
  }
  .avatar-select-card.default-active-opt {
    grid-column: span 3;
    padding: 10px 14px;
    background: rgba(252, 211, 77, 0.08);
    border-color: rgba(252, 211, 77, 0.3);
    align-items: flex-start;
  }
  .avatar-opt-title {
    font-size: 12px;
    font-weight: 800;
    color: #FCD34D;
  }
  .avatar-opt-sub {
    font-size: 10px;
    color: #94A3B8;
  }
  .avatar-opt-sprite {
    width: 48px;
    height: 48px;
    object-fit: contain;
  }
  .avatar-opt-name {
    font-size: 11px;
    font-weight: 800;
    color: #F1F5F9;
    text-align: center;
  }

  /* ==========================================
     ⚔️ BATTLE ARENA (v0.4.0)
     ========================================== */
  .arena-pane {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .tab-badge.bp-badge {
    background: linear-gradient(135deg, #F59E0B, #EF4444);
    color: #FFF;
    font-weight: 800;
    box-shadow: 0 0 8px rgba(245, 158, 11, 0.4);
  }

  /* 🏆 Arena Lobby & Hero Banner */
  .arena-lobby-card {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .arena-hero-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 16px 18px;
    border-radius: 16px;
    background: linear-gradient(135deg, rgba(239, 68, 68, 0.15) 0%, rgba(245, 158, 11, 0.12) 100%);
    border: 1px solid rgba(239, 68, 68, 0.3);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
  }
  .arena-hero-icon {
    font-size: 28px;
    filter: drop-shadow(0 0 10px rgba(239, 68, 68, 0.5));
  }
  .arena-hero-texts {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
  }
  .arena-heading {
    font-size: 15px;
    font-weight: 800;
    color: #F8FAFC;
    margin: 0;
    letter-spacing: -0.2px;
  }
  .arena-subheading {
    font-size: 11px;
    color: #CBD5E1;
    margin: 0;
  }
  .arena-bp-pill {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border-radius: 999px;
    background: rgba(245, 158, 11, 0.15);
    border: 1px solid rgba(245, 158, 11, 0.35);
    box-shadow: 0 0 12px rgba(245, 158, 11, 0.2);
  }
  .arena-bp-pill .bp-icon { font-size: 14px; }
  .arena-bp-pill .bp-amount {
    font-size: 12px;
    font-weight: 800;
    color: #FCD34D;
  }

  .arena-notice-banner {
    padding: 8px 12px;
    border-radius: 10px;
    background: rgba(59, 130, 246, 0.15);
    border: 1px solid rgba(59, 130, 246, 0.3);
    color: #93C5FD;
    font-size: 11px;
    font-weight: 700;
    text-align: center;
  }

  /* 🛡️ Fighter Preparation Card */
  .fighter-prep-card {
    padding: 16px;
    border-radius: 16px;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.08);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .prep-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .prep-title {
    font-size: 10.5px;
    font-weight: 800;
    color: #94A3B8;
    letter-spacing: 0.8px;
  }
  .prep-stage-tag {
    font-size: 10px;
    font-weight: 700;
    color: #38BDF8;
    padding: 2px 8px;
    border-radius: 999px;
    background: rgba(56, 189, 248, 0.1);
    border: 1px solid rgba(56, 189, 248, 0.25);
  }

  .prep-body {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .prep-sprite-box {
    width: 72px;
    height: 72px;
    border-radius: 14px;
    background: radial-gradient(circle, rgba(255, 255, 255, 0.08) 0%, rgba(0, 0, 0, 0.3) 100%);
    border: 1px solid rgba(255, 255, 255, 0.08);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .prep-sprite {
    width: 60px;
    height: 60px;
    object-fit: contain;
    filter: drop-shadow(0 4px 8px rgba(0, 0, 0, 0.5));
  }
  .prep-info-col {
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex: 1;
    min-width: 0;
  }
  .prep-name-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .prep-fighter-name {
    font-size: 14px;
    font-weight: 800;
    color: #F8FAFC;
  }
  .prep-types-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
  }
  .prep-type-pill {
    font-size: 9.5px;
    font-weight: 700;
    padding: 2px 7px;
    border-radius: 6px;
  }
  .prep-ribbon-count {
    font-size: 9.5px;
    font-weight: 700;
    color: #FCD34D;
  }

  .fighter-select-dropdown-box {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 4px;
  }
  .dropdown-lbl {
    font-size: 9.5px;
    color: #8B93A7;
    font-weight: 700;
  }
  .fighter-select-dropdown {
    width: 100%;
    padding: 6px 10px;
    border-radius: 8px;
    background: rgba(0, 0, 0, 0.4);
    border: 1px solid rgba(255, 255, 255, 0.12);
    color: #F1F5F9;
    font-size: 11px;
    font-weight: 600;
    outline: none;
    cursor: pointer;
  }

  .start-battle-main-btn {
    width: 100%;
    padding: 12px 18px;
    border-radius: 12px;
    background: linear-gradient(135deg, #EF4444 0%, #DC2626 50%, #B91C1C 100%);
    border: 1px solid rgba(255, 255, 255, 0.2);
    color: #FFF;
    font-size: 12.5px;
    font-weight: 800;
    letter-spacing: 0.5px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    cursor: pointer;
    box-shadow: 0 4px 16px rgba(239, 68, 68, 0.4);
    transition: all 0.2s ease;
  }
  .start-battle-main-btn:hover {
    transform: translateY(-2px);
    box-shadow: 0 6px 20px rgba(239, 68, 68, 0.6);
    filter: brightness(1.1);
  }

  /* 📊 Arena Career Stats Grid */
  .arena-stats-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
  }
  .arena-stat-box {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 10px;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.025);
    border: 1px solid rgba(255, 255, 255, 0.06);
    gap: 2px;
  }
  .arena-stat-box.highlight-streak {
    background: rgba(239, 68, 68, 0.08);
    border-color: rgba(239, 68, 68, 0.25);
  }
  .astat-val {
    font-size: 14px;
    font-weight: 800;
    color: #F8FAFC;
  }
  .astat-lbl {
    font-size: 9px;
    font-weight: 700;
    color: #8B93A7;
    letter-spacing: 0.5px;
  }

  /* 🏆 BP Rewards Exchange Shelf */
  .bp-exchange-section {
    padding: 14px;
    border-radius: 14px;
    background: rgba(255, 255, 255, 0.025);
    border: 1px solid rgba(255, 255, 255, 0.06);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .bpx-header {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .bpx-title {
    font-size: 12px;
    font-weight: 800;
    color: #F8FAFC;
  }
  .bpx-sub {
    font-size: 10px;
    color: #8B93A7;
  }
  .bpx-grid {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .bpx-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 10px;
    background: rgba(0, 0, 0, 0.2);
    border: 1px solid rgba(255, 255, 255, 0.05);
    transition: all 0.2s ease;
  }
  .bpx-card:hover {
    background: rgba(255, 255, 255, 0.04);
    border-color: rgba(255, 255, 255, 0.1);
  }
  .bpx-icon { font-size: 18px; flex-shrink: 0; }
  .bpx-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }
  .bpx-name {
    font-size: 11.5px;
    font-weight: 800;
    color: #F1F5F9;
  }
  .bpx-desc {
    font-size: 9.5px;
    color: #94A3B8;
  }
  .bpx-buy-btn {
    padding: 5px 12px;
    border-radius: 8px;
    background: rgba(245, 158, 11, 0.15);
    border: 1px solid rgba(245, 158, 11, 0.35);
    color: #FCD34D;
    font-size: 11px;
    font-weight: 800;
    cursor: pointer;
    transition: all 0.2s ease;
    flex-shrink: 0;
  }
  .bpx-buy-btn:hover:not(:disabled) {
    background: #F59E0B;
    color: #1E1B4B;
    transform: scale(1.04);
  }
  .bpx-buy-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  /* ⚔️ RETRO BATTLE STAGE CANVAS */
  .battle-stage-card {
    position: relative;
    padding: 14px;
    border-radius: 16px;
    background: radial-gradient(ellipse at 50% 30%, rgba(30, 41, 59, 0.9) 0%, rgba(15, 23, 42, 0.98) 100%);
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.6);
    display: flex;
    flex-direction: column;
    gap: 14px;
    overflow: hidden;
  }

  .battle-arena-glow {
    position: absolute;
    top: -50px;
    left: 50%;
    transform: translateX(-50%);
    width: 250px;
    height: 120px;
    border-radius: 50%;
    background: radial-gradient(circle, rgba(239, 68, 68, 0.2) 0%, transparent 70%);
    pointer-events: none;
  }

  .battle-stage-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    z-index: 2;
  }
  .battle-header-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .battle-live-indicator {
    font-size: 9px;
    font-weight: 900;
    color: #EF4444;
    padding: 2px 6px;
    border-radius: 4px;
    background: rgba(239, 68, 68, 0.15);
    border: 1px solid rgba(239, 68, 68, 0.35);
    letter-spacing: 0.5px;
    animation: livePulse 1.5s infinite ease-in-out;
  }
  @keyframes livePulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }
  .turn-counter {
    font-size: 11px;
    font-weight: 800;
    color: #CBD5E1;
  }
  .battle-flee-top-btn {
    padding: 4px 10px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: #94A3B8;
    font-size: 10px;
    font-weight: 700;
    cursor: pointer;
    transition: all 0.2s ease;
  }
  .battle-flee-top-btn:hover {
    background: rgba(239, 68, 68, 0.2);
    border-color: #EF4444;
    color: #FCA5A5;
  }

  /* Battle Field Pods */
  .battle-field {
    display: flex;
    flex-direction: column;
    gap: 16px;
    position: relative;
    min-height: 200px;
    justify-content: space-between;
    padding: 8px 0;
  }

  .combatant-pod {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    transition: opacity 0.3s ease;
  }
  .combatant-pod.combatant-fainted {
    opacity: 0.3;
    filter: grayscale(1);
  }

  .opponent-pod {
    flex-direction: row;
  }
  .player-pod {
    flex-direction: row;
  }

  /* Retro HUD */
  .combatant-hud {
    width: 170px;
    padding: 8px 10px;
    border-radius: 10px;
    background: rgba(0, 0, 0, 0.55);
    border: 1px solid rgba(255, 255, 255, 0.12);
    display: flex;
    flex-direction: column;
    gap: 4px;
    backdrop-filter: blur(8px);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
  }
  .hud-name-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .combatant-name {
    font-size: 11.5px;
    font-weight: 800;
    color: #F8FAFC;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100px;
  }
  .combatant-level {
    font-size: 10px;
    font-weight: 800;
    color: #FCD34D;
  }

  .hud-hp-bar-outer {
    width: 100%;
    height: 8px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.1);
    overflow: hidden;
    position: relative;
  }
  .hud-hp-bar-fill {
    height: 100%;
    border-radius: 999px;
    transition: width 0.4s ease-out, background-color 0.4s ease;
  }
  .hud-hp-bar-fill.hp-healthy {
    background: linear-gradient(90deg, #10B981, #34D399);
    box-shadow: 0 0 8px rgba(16, 185, 129, 0.5);
  }
  .hud-hp-bar-fill.hp-caution {
    background: linear-gradient(90deg, #F59E0B, #FBBF24);
    box-shadow: 0 0 8px rgba(245, 158, 11, 0.5);
  }
  .hud-hp-bar-fill.hp-danger {
    background: linear-gradient(90deg, #EF4444, #F87171);
    box-shadow: 0 0 8px rgba(239, 68, 68, 0.6);
  }

  .hud-hp-text-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .hp-num-label {
    font-size: 8.5px;
    font-weight: 900;
    color: #FCD34D;
    letter-spacing: 0.5px;
  }
  .hp-num-values {
    font-size: 9.5px;
    font-weight: 700;
    color: #E2E8F0;
  }

  /* Sprite Platform */
  .sprite-platform {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    width: 100px;
    height: 100px;
  }
  .arena-sprite {
    width: 84px;
    height: 84px;
    object-fit: contain;
    z-index: 2;
    filter: drop-shadow(0 6px 12px rgba(0, 0, 0, 0.6));
    animation: readyBounce 2.5s ease-in-out infinite alternate;
  }
  @keyframes readyBounce {
    0% { transform: translateY(0); }
    100% { transform: translateY(-4px); }
  }

  .platform-shadow {
    position: absolute;
    bottom: 6px;
    width: 70px;
    height: 14px;
    border-radius: 50%;
    background: radial-gradient(ellipse, rgba(0, 0, 0, 0.6) 0%, transparent 80%);
    z-index: 1;
  }

  .player-aura-sparkles {
    position: absolute;
    top: -8px;
    font-size: 9px;
    font-weight: 900;
    color: #FCD34D;
    text-shadow: 0 0 6px rgba(252, 211, 77, 0.8);
    animation: livePulse 1s infinite alternate;
  }

  /* 🎮 Battle Dashboard & Moves Grid */
  .battle-dashboard {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .moves-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .moves-title {
    font-size: 11px;
    font-weight: 800;
    color: #CBD5E1;
  }

  .moves-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 8px;
  }
  .move-btn {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 10px 12px;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.1);
    cursor: pointer;
    text-align: left;
    transition: all 0.2s ease;
  }
  .move-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(255, 255, 255, 0.25);
    transform: translateY(-2px);
  }
  .move-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .move-btn-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
  }
  .move-name {
    font-size: 11.5px;
    font-weight: 800;
    color: #F8FAFC;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .move-type-pill {
    font-size: 8.5px;
    font-weight: 800;
    color: var(--move-type-col, #FFF);
    padding: 1px 5px;
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.08);
  }
  .move-btn-bottom {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 4px;
  }
  .move-cat-tag {
    font-size: 9px;
    font-weight: 600;
    color: #94A3B8;
  }
  .move-pp-text {
    font-size: 9px;
    font-weight: 700;
    color: #FCD34D;
  }
  .move-power-text {
    font-size: 9px;
    font-weight: 700;
    color: #38BDF8;
  }

  /* 🎉 Victory & Defeat Result Overlay Cards */
  .battle-result-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: 20px 16px;
    border-radius: 14px;
    gap: 8px;
    animation: resultPop 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  @keyframes resultPop {
    0% { transform: scale(0.9); opacity: 0; }
    100% { transform: scale(1); opacity: 1; }
  }
  .battle-result-card.victory-card {
    background: radial-gradient(circle at 50% 30%, rgba(245, 158, 11, 0.2) 0%, rgba(16, 185, 129, 0.15) 100%);
    border: 1px solid rgba(245, 158, 11, 0.4);
    box-shadow: 0 8px 24px rgba(245, 158, 11, 0.2);
  }
  .battle-result-card.defeat-card {
    background: radial-gradient(circle at 50% 30%, rgba(239, 68, 68, 0.2) 0%, rgba(15, 23, 42, 0.6) 100%);
    border: 1px solid rgba(239, 68, 68, 0.4);
    box-shadow: 0 8px 24px rgba(239, 68, 68, 0.2);
  }

  .result-icon-burst { font-size: 32px; }
  .result-title {
    font-size: 16px;
    font-weight: 900;
    letter-spacing: 0.5px;
    margin: 0;
  }
  .victory-title { color: #FCD34D; }
  .defeat-title { color: #F87171; }
  .result-desc {
    font-size: 11px;
    color: #CBD5E1;
    margin: 0;
    max-width: 280px;
  }

  .rewards-pills-row {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 4px;
  }
  .reward-pill {
    padding: 4px 10px;
    border-radius: 999px;
    font-size: 10.5px;
    font-weight: 800;
  }
  .reward-pill.bp-pill {
    background: rgba(245, 158, 11, 0.2);
    border: 1px solid rgba(245, 158, 11, 0.4);
    color: #FCD34D;
  }
  .reward-pill.coin-pill {
    background: rgba(56, 189, 248, 0.2);
    border: 1px solid rgba(56, 189, 248, 0.4);
    color: #38BDF8;
  }
  .reward-pill.ribbon-pill {
    background: rgba(239, 68, 68, 0.2);
    border: 1px solid rgba(239, 68, 68, 0.4);
    color: #FCA5A5;
  }

  .result-actions-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    width: 100%;
  }
  .result-action-btn {
    padding: 10px 14px;
    border-radius: 10px;
    font-size: 11.5px;
    font-weight: 800;
    cursor: pointer;
    transition: all 0.2s ease;
  }
  .next-battle-btn, .retry-battle-btn {
    flex: 1;
    background: linear-gradient(135deg, #EF4444, #DC2626);
    border: 1px solid rgba(255, 255, 255, 0.2);
    color: #FFF;
    box-shadow: 0 4px 12px rgba(239, 68, 68, 0.4);
  }
  .leave-arena-btn {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.15);
    color: #E2E8F0;
  }

  /* 📜 Live Battle Log Terminal */
  .battle-log-card {
    padding: 10px 12px;
    border-radius: 12px;
    background: rgba(0, 0, 0, 0.45);
    border: 1px solid rgba(255, 255, 255, 0.08);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .battle-log-header {
    display: flex;
    align-items: center;
  }
  .log-title {
    font-size: 9.5px;
    font-weight: 800;
    color: #8B93A7;
    letter-spacing: 0.5px;
  }
  .battle-log-feed {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 110px;
    overflow-y: auto;
    font-family: monospace;
    padding-right: 4px;
  }
  .log-line {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    font-size: 10px;
    line-height: 1.35;
  }
  .log-line.actor-player { color: #38BDF8; }
  .log-line.actor-opponent { color: #F87171; }
  .log-line.actor-system { color: #FCD34D; font-weight: 700; }
  .log-bullet { opacity: 0.6; flex-shrink: 0; }
  .log-text { word-break: break-word; }
</style>
