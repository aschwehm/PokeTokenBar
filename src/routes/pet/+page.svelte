<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { resolveOverdrive } from "$lib/mega";

  interface CompanionView {
    displayName: string;
    isEgg: boolean;
    hasActive: boolean;
    currentSpeciesId: number | null;
    isShiny: boolean;
    displayState: string;
    progress: number;
    eggProgress: number;
    hasGoldenAura?: boolean;
    berryFeedback?: string | null;
    isMegaOverdrive?: boolean;
    megaOverdriveEnabled?: boolean;
  }

  interface UsageView {
    burnTier: string;
    todayTotalTokens: number;
  }

  interface Snapshot {
    companion: CompanionView;
    usage: UsageView;
  }

  interface HeartParticle {
    id: number;
    emoji: string;
    x: number;
    y: number;
    scale: number;
  }

  let snap = $state<Snapshot | null>(null);

  // Interaction State
  let animState = $state<"normal" | "hop" | "wiggle" | "wake" | "eating">("normal");
  let hearts = $state<HeartParticle[]>([]);
  let heartSeq = 0;
  let animTimeout: ReturnType<typeof setTimeout> | null = null;
  let isWakingUp = $state(false);
  let wasSleeping = $state(false);

  // Drag vs Click detection
  let isPointerDown = false;
  let hasDragged = false;
  let pointerDownX = 0;
  let pointerDownY = 0;
  let pointerDownTime = 0;

  // Track idle sleep (15 minutes idle override)
  let lastActiveTimestamp = $state(Date.now());
  let lastTokenCount = $state(0);

  let isSleeping = $derived.by(() => {
    if (!snap) return false;
    if (snap.companion.isEgg) return false;
    if (isWakingUp) return false;
    if (snap.companion.displayState === "sleep") return true;
    if (snap.usage.burnTier === "idle" && Date.now() - lastActiveTimestamp > 15 * 60 * 1000) {
      return true;
    }
    return false;
  });

  async function refresh() {
    try {
      const prev = snap;
      snap = await invoke<Snapshot>("snapshot");

      if (snap) {
        // Track token changes to update last activity
        if (snap.usage.todayTotalTokens !== lastTokenCount) {
          lastTokenCount = snap.usage.todayTotalTokens;
          lastActiveTimestamp = Date.now();
          if (wasSleeping) {
            triggerWakeUp();
          }
        }

        if (snap.usage.burnTier !== "idle") {
          lastActiveTimestamp = Date.now();
          if (wasSleeping) {
            triggerWakeUp();
          }
        }

        // Berry eating feedback from backend
        if (snap.companion.berryFeedback) {
          triggerEatingAnimation(snap.companion.berryFeedback);
          invoke("consume_feedback").catch(() => {});
        }

        wasSleeping = isSleeping;
      }
    } catch {
      // ignore
    }
  }

  function triggerWakeUp() {
    isWakingUp = true;
    playAnimation("wake");
    spawnHearts(["✨", "⭐", "❗"]);
    setTimeout(() => {
      isWakingUp = false;
    }, 1800);
  }

  function triggerEatingAnimation(kind: string) {
    lastActiveTimestamp = Date.now();
    playAnimation("eating");
    const emoji = kind === "sitrusBerry" ? "🍊" : "🫐";
    spawnHearts([emoji, "✨", "💖", "😋"]);
  }

  function playAnimation(anim: "hop" | "wiggle" | "wake" | "eating") {
    if (animTimeout) clearTimeout(animTimeout);
    animState = anim;
    const duration = anim === "wake" ? 1000 : anim === "eating" ? 900 : 600;
    animTimeout = setTimeout(() => {
      animState = "normal";
    }, duration);
  }

  function spawnHearts(customEmojis?: string[]) {
    const emojis = customEmojis ?? ["❤️", "💖", "✨", "🥰", "⭐"];
    const count = 3 + Math.floor(Math.random() * 3);
    for (let i = 0; i < count; i++) {
      const particle: HeartParticle = {
        id: ++heartSeq,
        emoji: emojis[Math.floor(Math.random() * emojis.length)],
        x: (Math.random() - 0.5) * 50,
        y: -10 - Math.random() * 25,
        scale: 0.8 + Math.random() * 0.5,
      };
      hearts = [...hearts, particle];
      setTimeout(() => {
        hearts = hearts.filter((h) => h.id !== particle.id);
      }, 1200);
    }
  }

  // Interactive Petting Handler
  function handlePetInteraction() {
    lastActiveTimestamp = Date.now();
    if (isSleeping) {
      triggerWakeUp();
      return;
    }

    // Pick between hop and wiggle
    const rolls: Array<"hop" | "wiggle"> = ["hop", "wiggle", "hop", "wiggle"];
    const chosen = rolls[Math.floor(Math.random() * rolls.length)];
    playAnimation(chosen);
    spawnHearts();
  }

  function handleMouseDown(e: MouseEvent) {
    if (e.button === 0) {
      isPointerDown = true;
      hasDragged = false;
      pointerDownX = e.clientX;
      pointerDownY = e.clientY;
      pointerDownTime = Date.now();
    }
  }

  async function handleMouseMove(e: MouseEvent) {
    if (!isPointerDown || hasDragged) return;
    const dist = Math.hypot(e.clientX - pointerDownX, e.clientY - pointerDownY);
    if (dist > 5) {
      hasDragged = true;
      try {
        await getCurrentWindow().startDragging();
      } catch {
        // ignore
      }
    }
  }

  function handleMouseUp(e: MouseEvent) {
    if (e.button === 0) {
      const wasDown = isPointerDown;
      const didDrag = hasDragged;
      isPointerDown = false;
      hasDragged = false;
      const dist = Math.hypot(e.clientX - pointerDownX, e.clientY - pointerDownY);
      const elapsed = Date.now() - pointerDownTime;
      if (wasDown && !didDrag && dist < 8 && elapsed < 500) {
        handlePetInteraction();
      }
    }
  }

  function spriteUrl(id: number, shiny: boolean): string {
    const dir = shiny ? "shiny/" : "";
    return `https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/other/showdown/${dir}${id}.gif`;
  }

  function fallbackStaticSprite(e: Event, id: number, shiny: boolean) {
    const img = e.currentTarget as HTMLImageElement;
    const dir = shiny ? "shiny/" : "";
    img.src = `https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/${dir}${id}.png`;
  }

  onMount(() => {
    refresh();
    const interval = setInterval(refresh, 5_000);
    const unlisten = listen("tray-refresh", refresh);
    return () => {
      clearInterval(interval);
      if (animTimeout) clearTimeout(animTimeout);
      unlisten.then((f) => f());
    };
  });
</script>

<div
  class="pet-container"
  onmousedown={handleMouseDown}
  onmousemove={handleMouseMove}
  onmouseup={handleMouseUp}
  onclick={() => { if (!hasDragged) handlePetInteraction(); }}
  role="button"
  tabindex="0"
  aria-label="Desktop Pet (Click to pet, drag to move)"
  title="Click to Pet & Play / Drag to Move"
>
  {#if snap}
    {@const c = snap.companion}
    {@const u = snap.usage}
    {@const currentProg = Math.max(0, Math.min(1, c.isEgg ? c.eggProgress : c.progress))}
    {@const circ = 326.73}
    {@const strokeDash = circ * (1 - currentProg)}
    {@const hasGoldenAura = c.hasGoldenAura}
    {@const isOverdrive = Boolean(c.isMegaOverdrive || ((u.burnTier === "fast" || u.burnTier === "blazing") && c.megaOverdriveEnabled))}
    {@const overdriveInfo = isOverdrive && c.hasActive && !c.isEgg ? resolveOverdrive(c.currentSpeciesId, c.displayName) : null}
    {@const spriteId = overdriveInfo ? overdriveInfo.spriteId : c.currentSpeciesId}
    {@const petName = overdriveInfo ? overdriveInfo.displayName : c.displayName}

    <div
      class="pet-wrapper"
      class:blazing={u.burnTier === "blazing"}
      class:focus={u.burnTier === "fast"}
      class:sleeping={isSleeping}
      class:golden-aura={hasGoldenAura}
      class:overdrive={isOverdrive}
    >
      <!-- Circular Progress Ring -->
      <svg class="progress-ring" viewBox="0 0 116 116">
        <defs>
          <linearGradient id="ring-grad-default" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stop-color="#39D98A" />
            <stop offset="100%" stop-color="#5B8CFF" />
          </linearGradient>
          <linearGradient id="ring-grad-egg" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stop-color="#E8B84B" />
            <stop offset="100%" stop-color="#FF8A59" />
          </linearGradient>
          <linearGradient id="ring-grad-blazing" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stop-color="#FF8A59" />
            <stop offset="100%" stop-color="#E3372E" />
          </linearGradient>
          <linearGradient id="ring-grad-sleep" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stop-color="#818CF8" />
            <stop offset="100%" stop-color="#C084FC" />
          </linearGradient>
          <linearGradient id="ring-grad-gold" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stop-color="#FCD34D" />
            <stop offset="100%" stop-color="#F59E0B" />
          </linearGradient>
          <linearGradient id="ring-grad-overdrive" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stop-color="#FF007A" />
            <stop offset="35%" stop-color="#FF9900" />
            <stop offset="70%" stop-color="#FFEA00" />
            <stop offset="100%" stop-color="#00E5FF" />
          </linearGradient>
        </defs>

        <!-- Track Ring -->
        <circle cx="58" cy="58" r="52" class="ring-track" />

        <!-- Progress Fill Ring -->
        <circle
          cx="58"
          cy="58"
          r="52"
          class="ring-progress-fill"
          stroke={isOverdrive
            ? "url(#ring-grad-overdrive)"
            : hasGoldenAura
            ? "url(#ring-grad-gold)"
            : isSleeping
            ? "url(#ring-grad-sleep)"
            : c.isEgg
            ? "url(#ring-grad-egg)"
            : u.burnTier === "blazing"
            ? "url(#ring-grad-blazing)"
            : "url(#ring-grad-default)"}
          stroke-dasharray="326.73"
          stroke-dashoffset={strokeDash}
        />
      </svg>

      <!-- Center Sprite Area (Interactive!) -->
      <div class="sprite-stage">
        {#if c.isEgg}
          <div class="egg-sprite" class:hop={animState === "hop"}>🥚</div>
        {:else if c.hasActive && spriteId}
          <img
            class="sprite"
            class:bounce={u.burnTier === "fast" || u.burnTier === "blazing"}
            class:sleeping-mon={isSleeping}
            class:hop={animState === "hop"}
            class:wiggle={animState === "wiggle"}
            class:wake-up={animState === "wake"}
            class:eating={animState === "eating"}
            class:overdrive-sprite={isOverdrive}
            src={spriteUrl(spriteId, c.isShiny)}
            alt={petName}
            draggable="false"
            onerror={(e) => fallbackStaticSprite(e, spriteId ?? 1, c.isShiny)}
          />
        {/if}

        <!-- Overdrive Sparks -->
        {#if isOverdrive}
          <div class="overdrive-sparks-box">
            <span class="pet-od-spark pod-1">⚡</span>
            <span class="pet-od-spark pod-2">🧬</span>
            <span class="pet-od-spark pod-3">💥</span>
          </div>
        {/if}

        <!-- Floating Zzz Sleep Bubbles -->
        {#if isSleeping}
          <div class="zzz-container">
            <span class="zzz zzz-1">z</span>
            <span class="zzz zzz-2">Z</span>
            <span class="zzz zzz-3">Z</span>
            <span class="zzz zzz-4">💤</span>
          </div>
        {/if}

        <!-- Golden Sparkle Trail -->
        {#if hasGoldenAura}
          <div class="golden-sparkles-container">
            <span class="sparkle sp-1">✨</span>
            <span class="sparkle sp-2">⭐</span>
            <span class="sparkle sp-3">✨</span>
          </div>
        {/if}

        <!-- Floating Heart & Joy Particles -->
        {#each hearts as h (h.id)}
          <span
            class="floating-heart"
            style="--target-x: {h.x}px; --target-y: {h.y}px; --scale: {h.scale};"
          >
            {h.emoji}
          </span>
        {/each}
      </div>

      <!-- Burn Pace / Sleep Status Badge (Top-Right) -->
      {#if isOverdrive && overdriveInfo}
        <div class="status-badge overdrive-badge" title="Mega Overdrive (2× Coins Active!)">{overdriveInfo.mode === 'gmax' ? '⚡' : '🧬'}</div>
      {:else if isSleeping}
        <div class="status-badge sleep-badge" title="Sleeping (Click to pet & wake up)">💤</div>
      {:else if hasGoldenAura}
        <div class="status-badge gold-badge" title="Sitrus Sparkle Aura Active! ✨">🌟</div>
      {:else if u.burnTier === "blazing"}
        <div class="status-badge blazing-badge" title="On Fire!">🔥</div>
      {:else if u.burnTier === "fast"}
        <div class="status-badge fast-badge" title="Fast Pace">⚡</div>
      {/if}
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
    background: transparent !important;
    background-color: transparent !important;
    user-select: none;
  }

  .pet-container {
    width: 100vw;
    height: 100vh;
    min-width: 220px;
    min-height: 220px;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: grab;
    background: transparent;
  }

  .pet-container:active {
    cursor: grabbing;
  }

  .pet-wrapper {
    position: relative;
    width: 116px;
    height: 116px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: rgba(14, 16, 22, 0.88);
    backdrop-filter: blur(14px);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.55), inset 0 0 10px rgba(255, 255, 255, 0.04);
    transition: transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1), box-shadow 0.25s ease;
  }

  .pet-wrapper:hover {
    transform: scale(1.06);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.65), inset 0 0 14px rgba(255, 255, 255, 0.1);
  }

  .pet-wrapper.focus {
    box-shadow: 0 0 18px rgba(232, 184, 75, 0.5), inset 0 0 12px rgba(232, 184, 75, 0.15);
  }

  .pet-wrapper.blazing {
    box-shadow: 0 0 20px rgba(227, 55, 45, 0.6), inset 0 0 14px rgba(227, 55, 45, 0.2);
  }

  .pet-wrapper.sleeping {
    box-shadow: 0 0 18px rgba(129, 140, 248, 0.45), inset 0 0 12px rgba(129, 140, 248, 0.15);
    background: rgba(16, 17, 28, 0.9);
  }

  .pet-wrapper.golden-aura {
    box-shadow: 0 0 22px rgba(245, 158, 11, 0.65), inset 0 0 14px rgba(252, 211, 77, 0.25);
    animation: goldPulse 2.5s infinite ease-in-out;
  }

  @keyframes goldPulse {
    0%, 100% { box-shadow: 0 0 20px rgba(245, 158, 11, 0.6), inset 0 0 12px rgba(252, 211, 77, 0.2); }
    50% { box-shadow: 0 0 28px rgba(245, 158, 11, 0.85), inset 0 0 18px rgba(252, 211, 77, 0.4); }
  }

  /* SVG Perimeter Progress Ring */
  .progress-ring {
    position: absolute;
    top: 0;
    left: 0;
    width: 116px;
    height: 116px;
    pointer-events: none;
  }

  .ring-track {
    fill: none;
    stroke: rgba(255, 255, 255, 0.1);
    stroke-width: 3.5;
  }

  .ring-progress-fill {
    fill: none;
    stroke-width: 3.5;
    stroke-linecap: round;
    transform: rotate(-90deg);
    transform-origin: 58px 58px;
    transition: stroke-dashoffset 0.4s ease, stroke 0.3s ease;
  }

  /* Sprite Area */
  .sprite-stage {
    position: relative;
    width: 86px;
    height: 86px;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: visible;
    pointer-events: auto;
    cursor: pointer;
    z-index: 1;
  }

  .sprite {
    max-width: 82px;
    max-height: 82px;
    width: auto;
    height: auto;
    object-fit: contain;
    image-rendering: pixelated;
    filter: drop-shadow(0 4px 10px rgba(0, 0, 0, 0.5));
    transition: transform 0.2s cubic-bezier(0.34, 1.56, 0.64, 1), opacity 0.3s ease;
    transform-origin: center bottom;
  }

  /* Animations */
  @keyframes bounce {
    0%, 100% { transform: translateY(0); }
    50% { transform: translateY(-4px); }
  }

  .bounce {
    animation: bounce 0.8s infinite ease-in-out;
  }

  /* Sleep Breathing */
  @keyframes sleepBreath {
    0%, 100% { transform: translateY(2px) scale(0.96) rotate(-2deg); opacity: 0.85; }
    50% { transform: translateY(0px) scale(1) rotate(1deg); opacity: 0.95; }
  }

  .sleeping-mon {
    animation: sleepBreath 3.2s infinite ease-in-out !important;
  }

  /* Hop Interaction */
  @keyframes petHop {
    0% { transform: scale(1, 1) translateY(0); }
    30% { transform: scale(1.15, 0.85) translateY(0); }
    50% { transform: scale(0.9, 1.15) translateY(-14px); }
    75% { transform: scale(1.05, 0.95) translateY(-2px); }
    100% { transform: scale(1, 1) translateY(0); }
  }

  .sprite.hop, .egg-sprite.hop {
    animation: petHop 0.55s cubic-bezier(0.34, 1.56, 0.64, 1) !important;
  }

  /* Wiggle Interaction */
  @keyframes petWiggle {
    0%, 100% { transform: rotate(0deg) scale(1); }
    20% { transform: rotate(-12deg) scale(1.08); }
    40% { transform: rotate(12deg) scale(1.08); }
    60% { transform: rotate(-8deg) scale(1.05); }
    80% { transform: rotate(8deg) scale(1.05); }
  }

  .sprite.wiggle {
    animation: petWiggle 0.6s ease-in-out !important;
  }

  /* Wake Up Pop */
  @keyframes wakePop {
    0% { transform: scale(0.85) translateY(4px); }
    40% { transform: scale(1.22) translateY(-12px); }
    70% { transform: scale(0.95) translateY(2px); }
    100% { transform: scale(1) translateY(0); }
  }

  .sprite.wake-up {
    animation: wakePop 0.8s cubic-bezier(0.34, 1.56, 0.64, 1) !important;
  }

  /* Eating Animation */
  @keyframes eatingNom {
    0%, 100% { transform: scale(1); }
    25% { transform: scale(1.15, 0.88) translateY(2px); }
    50% { transform: scale(0.92, 1.12) translateY(-4px); }
    75% { transform: scale(1.1, 0.92) translateY(1px); }
  }

  .sprite.eating {
    animation: eatingNom 0.8s ease-in-out !important;
  }

  /* Egg wobble */
  .egg-sprite {
    font-size: 48px;
    line-height: 1;
    filter: drop-shadow(0 4px 10px rgba(0, 0, 0, 0.5));
    animation: eggWobble 2.5s infinite ease-in-out;
  }

  @keyframes eggWobble {
    0%, 100% { transform: rotate(0deg); }
    25% { transform: rotate(-6deg); }
    75% { transform: rotate(6deg); }
  }

  /* Floating Zzz Bubbles */
  .zzz-container {
    position: absolute;
    top: -12px;
    right: 2px;
    display: flex;
    flex-direction: column;
    align-items: center;
    pointer-events: none;
  }

  .zzz {
    position: absolute;
    color: #C084FC;
    font-family: system-ui, sans-serif;
    font-weight: 800;
    text-shadow: 0 2px 6px rgba(0, 0, 0, 0.7);
    opacity: 0;
    animation: floatZzz 3.2s infinite cubic-bezier(0.25, 0.46, 0.45, 0.94);
  }

  .zzz-1 { font-size: 11px; animation-delay: 0s; }
  .zzz-2 { font-size: 14px; animation-delay: 0.8s; }
  .zzz-3 { font-size: 17px; animation-delay: 1.6s; }
  .zzz-4 { font-size: 18px; animation-delay: 2.4s; }

  @keyframes floatZzz {
    0% { transform: translate(0, 0) scale(0.6); opacity: 0; }
    20% { opacity: 0.9; }
    80% { opacity: 0.7; }
    100% { transform: translate(14px, -32px) scale(1.15); opacity: 0; }
  }

  /* Golden Sparkles Trail */
  .golden-sparkles-container {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }

  .sparkle {
    position: absolute;
    font-size: 14px;
    animation: floatSparkle 2s infinite ease-in-out;
    filter: drop-shadow(0 0 6px rgba(245, 158, 11, 0.8));
  }

  .sp-1 { top: 4px; left: 6px; animation-delay: 0s; }
  .sp-2 { top: 12px; right: 8px; animation-delay: 0.7s; font-size: 11px; }
  .sp-3 { bottom: 8px; left: 14px; animation-delay: 1.3s; font-size: 12px; }

  @keyframes floatSparkle {
    0%, 100% { transform: scale(0.7) rotate(0deg); opacity: 0.3; }
    50% { transform: scale(1.2) rotate(45deg); opacity: 1; }
  }

  /* Floating Pet Hearts / Particle FX */
  .floating-heart {
    position: absolute;
    font-size: 18px;
    pointer-events: none;
    animation: heartFly 1.1s cubic-bezier(0.22, 1, 0.36, 1) forwards;
    filter: drop-shadow(0 2px 8px rgba(0, 0, 0, 0.6));
    z-index: 10;
  }

  @keyframes heartFly {
    0% { transform: translate(0, 0) scale(0.5); opacity: 0; }
    25% { opacity: 1; transform: translate(calc(var(--target-x) * 0.4), calc(var(--target-y) * 0.4)) scale(var(--scale)); }
    100% { transform: translate(var(--target-x), var(--target-y)) scale(calc(var(--scale) * 1.25)); opacity: 0; }
  }

  /* Badges */
  .status-badge {
    position: absolute;
    top: -2px;
    right: -2px;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 13px;
    background: #111319;
    border: 1.5px solid rgba(255, 255, 255, 0.15);
    box-shadow: 0 3px 8px rgba(0, 0, 0, 0.5);
    z-index: 3;
  }

  .status-badge.blazing-badge {
    border-color: rgba(227, 55, 45, 0.7);
    box-shadow: 0 0 10px rgba(227, 55, 45, 0.5);
  }

  .status-badge.fast-badge {
    border-color: rgba(232, 184, 75, 0.7);
    box-shadow: 0 0 10px rgba(232, 184, 75, 0.5);
  }

  .status-badge.sleep-badge {
    border-color: rgba(167, 139, 250, 0.7);
    box-shadow: 0 0 10px rgba(167, 139, 250, 0.5);
    background: #16182a;
  }

  .status-badge.gold-badge {
    border-color: rgba(245, 158, 11, 0.8);
    box-shadow: 0 0 12px rgba(245, 158, 11, 0.6);
    background: #241c0e;
  }

  /* Mega Overdrive Styles */
  .pet-wrapper.overdrive {
    box-shadow: 0 0 16px rgba(255, 0, 122, 0.6), 0 0 30px rgba(0, 229, 255, 0.4), inset 0 0 14px rgba(0, 229, 255, 0.25);
    animation: petOverdrivePulse 2s infinite ease-in-out;
  }

  @keyframes petOverdrivePulse {
    0%, 100% {
      box-shadow: 0 0 16px rgba(255, 0, 122, 0.6), 0 0 28px rgba(0, 229, 255, 0.35), inset 0 0 14px rgba(0, 229, 255, 0.25);
    }
    50% {
      box-shadow: 0 0 22px rgba(255, 0, 122, 0.8), 0 0 36px rgba(0, 229, 255, 0.55), inset 0 0 18px rgba(255, 0, 122, 0.35);
    }
  }

  .sprite.overdrive-sprite {
    filter: drop-shadow(0 0 14px rgba(255, 0, 122, 0.8)) drop-shadow(0 0 20px rgba(0, 229, 255, 0.6)) !important;
  }

  .overdrive-sparks-box {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }

  .pet-od-spark {
    position: absolute;
    font-size: 14px;
    animation: sparkFlash 1.2s infinite ease-in-out;
    filter: drop-shadow(0 0 8px #FF007A);
  }
  .pet-od-spark.pod-1 { top: 2px; left: 4px; animation-delay: 0s; }
  .pet-od-spark.pod-2 { top: 6px; right: 4px; animation-delay: 0.4s; font-size: 12px; }
  .pet-od-spark.pod-3 { bottom: 4px; left: 18px; animation-delay: 0.8s; font-size: 13px; }

  .status-badge.overdrive-badge {
    border-color: rgba(255, 0, 122, 0.85);
    box-shadow: 0 0 14px rgba(255, 0, 122, 0.75);
    background: linear-gradient(135deg, #FF007A, #7928CA);
    color: #FFF;
    font-size: 12px;
    animation: pulseMegaBadge 1.8s infinite ease-in-out;
  }

  @keyframes pulseMegaBadge {
    0%, 100% { transform: scale(1); filter: brightness(1); }
    50% { transform: scale(1.08); filter: brightness(1.25); }
  }

  @keyframes sparkFlash {
    0%, 100% { opacity: 0.2; transform: scale(0.7) translateY(0); }
    50% { opacity: 1; transform: scale(1.2) translateY(-4px); }
  }
</style>


