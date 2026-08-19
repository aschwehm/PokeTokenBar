<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  interface CompanionView {
    displayName: string;
    isEgg: boolean;
    hasActive: boolean;
    currentSpeciesId: number | null;
    isShiny: boolean;
    displayState: string;
    progress: number;
    eggProgress: number;
  }

  interface UsageView {
    burnTier: string;
  }

  interface Snapshot {
    companion: CompanionView;
    usage: UsageView;
  }

  let snap = $state<Snapshot | null>(null);

  async function refresh() {
    try {
      snap = await invoke<Snapshot>("snapshot");
    } catch {
      // ignore
    }
  }

  async function startDrag(e: MouseEvent) {
    if (e.button === 0) {
      try {
        await getCurrentWindow().startDragging();
      } catch {
        // ignore
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
      unlisten.then((f) => f());
    };
  });
</script>

<div
  class="pet-container"
  data-tauri-drag-region
  onmousedown={startDrag}
  role="toolbar"
  tabindex="-1"
  aria-label="Desktop Pet"
>
  {#if snap}
    {@const c = snap.companion}
    {@const u = snap.usage}
    {@const currentProg = Math.max(0, Math.min(1, c.isEgg ? c.eggProgress : c.progress))}
    {@const circ = 326.73}
    {@const strokeDash = circ * (1 - currentProg)}

    <div class="pet-wrapper" class:blazing={u.burnTier === "blazing"} class:focus={u.burnTier === "fast"}>
      <!-- Circular Progress Ring (circumference) -->
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
        </defs>

        <!-- Track Ring -->
        <circle
          cx="58"
          cy="58"
          r="52"
          class="ring-track"
        />

        <!-- Progress Fill Ring -->
        <circle
          cx="58"
          cy="58"
          r="52"
          class="ring-progress-fill"
          stroke={c.isEgg ? "url(#ring-grad-egg)" : u.burnTier === "blazing" ? "url(#ring-grad-blazing)" : "url(#ring-grad-default)"}
          stroke-dasharray="326.73"
          stroke-dashoffset={strokeDash}
        />
      </svg>

      <!-- Center Sprite Area (Unobstructed!) -->
      <div class="sprite-stage">
        {#if c.isEgg}
          <div class="egg-sprite">🥚</div>
        {:else if c.hasActive && c.currentSpeciesId}
          <img
            class="sprite"
            class:bounce={u.burnTier === "fast" || u.burnTier === "blazing"}
            src={spriteUrl(c.currentSpeciesId, c.isShiny)}
            alt={c.displayName}
            draggable="false"
            onerror={(e) => fallbackStaticSprite(e, c.currentSpeciesId ?? 1, c.isShiny)}
          />
        {/if}
      </div>

      <!-- Burn Pace Badge (Top-Right) -->
      {#if u.burnTier === "blazing"}
        <div class="flame-badge blazing-badge" title="On Fire!">🔥</div>
      {:else if u.burnTier === "fast"}
        <div class="flame-badge fast-badge" title="Fast Pace">⚡</div>
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
    width: 160px;
    height: 160px;
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
    transition: transform 0.2s ease, box-shadow 0.25s ease;
  }

  .pet-wrapper:hover {
    transform: scale(1.05);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.65), inset 0 0 12px rgba(255, 255, 255, 0.08);
  }

  .pet-wrapper.focus {
    box-shadow: 0 0 16px rgba(232, 184, 75, 0.5), inset 0 0 12px rgba(232, 184, 75, 0.15);
  }

  .pet-wrapper.blazing {
    box-shadow: 0 0 18px rgba(227, 55, 45, 0.6), inset 0 0 14px rgba(227, 55, 45, 0.2);
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
    transition: stroke-dashoffset 0.4s ease;
  }

  /* Sprite Area */
  .sprite-stage {
    width: 86px;
    height: 86px;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: visible;
    pointer-events: none;
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
  }

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

  .flame-badge {
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

  .flame-badge.blazing-badge {
    border-color: rgba(227, 55, 45, 0.7);
    box-shadow: 0 0 10px rgba(227, 55, 45, 0.5);
  }

  .flame-badge.fast-badge {
    border-color: rgba(232, 184, 75, 0.7);
    box-shadow: 0 0 10px rgba(232, 184, 75, 0.5);
  }

  @keyframes bounce {
    0%, 100% { transform: translateY(0); }
    50% { transform: translateY(-4px); }
  }

  .bounce {
    animation: bounce 0.8s infinite ease-in-out;
  }
</style>

