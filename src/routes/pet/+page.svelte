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

    <div class="pet-wrapper" class:blazing={u.burnTier === "blazing"} class:focus={u.burnTier === "fast"}>
      {#if c.isEgg}
        <div class="egg">
          <div class="egg-sprite">🥚</div>
          <div class="ring-progress">
            <div class="ring-fill" style="width: {(c.eggProgress * 100).toFixed(0)}%"></div>
          </div>
        </div>
      {:else if c.hasActive && c.currentSpeciesId}
        <img
          class="sprite"
          class:bounce={u.burnTier === "fast" || u.burnTier === "blazing"}
          src={spriteUrl(c.currentSpeciesId, c.isShiny)}
          alt={c.displayName}
          draggable="false"
          onerror={(e) => fallbackStaticSprite(e, c.currentSpeciesId ?? 1, c.isShiny)}
        />
        <div class="ring-progress">
          <div class="ring-fill" style="width: {(c.progress * 100).toFixed(0)}%"></div>
        </div>
      {/if}

      {#if u.burnTier === "blazing"}
        <div class="flame-badge">🔥</div>
      {:else if u.burnTier === "fast"}
        <div class="flame-badge">⚡</div>
      {/if}
    </div>
  {/if}
</div>

<style>
  :global(html, body) {
    margin: 0;
    padding: 0;
    overflow: hidden;
    background: transparent !important;
    background-color: transparent !important;
    user-select: none;
  }

  .pet-container {
    width: 140px;
    height: 140px;
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
    width: 120px;
    height: 120px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: rgba(20, 24, 33, 0.82);
    backdrop-filter: blur(10px);
    border: 2px solid rgba(255, 255, 255, 0.15);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
    transition: transform 0.2s ease, border-color 0.2s ease;
  }

  .pet-wrapper:hover {
    transform: scale(1.04);
    border-color: rgba(255, 255, 255, 0.35);
  }

  .pet-wrapper.focus {
    border-color: rgba(229, 169, 60, 0.85);
    box-shadow: 0 0 18px rgba(229, 169, 60, 0.45);
  }

  .pet-wrapper.blazing {
    border-color: rgba(255, 93, 93, 0.95);
    box-shadow: 0 0 22px rgba(255, 93, 93, 0.65);
  }

  .sprite {
    width: 96px;
    height: 96px;
    image-rendering: pixelated;
    pointer-events: none;
    margin-top: -6px;
    margin-bottom: -10px;
    filter: drop-shadow(0 4px 8px rgba(0, 0, 0, 0.5));
  }

  .egg {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
  }

  .egg-sprite {
    font-size: 52px;
    line-height: 1;
    margin-bottom: 2px;
    filter: drop-shadow(0 4px 8px rgba(0, 0, 0, 0.5));
  }

  .ring-progress {
    width: 64px;
    height: 5px;
    background: rgba(255, 255, 255, 0.2);
    border-radius: 3px;
    overflow: hidden;
    z-index: 2;
  }

  .ring-fill {
    height: 100%;
    background: linear-gradient(90deg, #49c66c, #e5a93c);
    border-radius: 3px;
    transition: width 0.3s ease;
  }

  .flame-badge {
    position: absolute;
    top: 2px;
    right: 2px;
    font-size: 16px;
    background: rgba(0, 0, 0, 0.7);
    border-radius: 50%;
    padding: 3px;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.4);
  }

  @keyframes bounce {
    0%, 100% {
      transform: translateY(0);
    }
    50% {
      transform: translateY(-4px);
    }
  }

  .bounce {
    animation: bounce 0.8s infinite ease-in-out;
  }
</style>
