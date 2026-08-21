export interface MegaFormInfo {
  megaId: number;
  displayName: string;
  mode: "mega" | "gmax" | "primal";
}

// Map base or final species ID to their Mega/G-Max form ID in PokéAPI:
export const MEGA_FORMS: Record<number, MegaFormInfo> = {
  // Gen 1
  3: { megaId: 10033, displayName: "Mega Venusaur", mode: "mega" },
  6: { megaId: 10034, displayName: "Mega Charizard X", mode: "mega" },
  9: { megaId: 10036, displayName: "Mega Blastoise", mode: "mega" },
  12: { megaId: 10198, displayName: "G-Max Butterfree", mode: "gmax" },
  15: { megaId: 10090, displayName: "Mega Beedrill", mode: "mega" },
  18: { megaId: 10073, displayName: "Mega Pidgeot", mode: "mega" },
  25: { megaId: 10199, displayName: "G-Max Pikachu", mode: "gmax" },
  52: { megaId: 10200, displayName: "G-Max Meowth", mode: "gmax" },
  65: { megaId: 10037, displayName: "Mega Alakazam", mode: "mega" },
  68: { megaId: 10197, displayName: "G-Max Machamp", mode: "gmax" },
  94: { megaId: 10038, displayName: "Mega Gengar", mode: "mega" },
  99: { megaId: 10201, displayName: "G-Max Kingler", mode: "gmax" },
  115: { megaId: 10039, displayName: "Mega Kangaskhan", mode: "mega" },
  127: { megaId: 10040, displayName: "Mega Pinsir", mode: "mega" },
  130: { megaId: 10041, displayName: "Mega Gyarados", mode: "mega" },
  131: { megaId: 10206, displayName: "G-Max Lapras", mode: "gmax" },
  133: { megaId: 10205, displayName: "G-Max Eevee", mode: "gmax" },
  142: { megaId: 10042, displayName: "Mega Aerodactyl", mode: "mega" },
  143: { megaId: 10207, displayName: "G-Max Snorlax", mode: "gmax" },
  150: { megaId: 10044, displayName: "Mega Mewtwo Y", mode: "mega" },

  // Gen 2
  181: { megaId: 10045, displayName: "Mega Ampharos", mode: "mega" },
  208: { megaId: 10072, displayName: "Mega Steelix", mode: "mega" },
  212: { megaId: 10046, displayName: "Mega Scizor", mode: "mega" },
  214: { megaId: 10047, displayName: "Mega Heracross", mode: "mega" },
  229: { megaId: 10048, displayName: "Mega Houndoom", mode: "mega" },
  248: { megaId: 10049, displayName: "Mega Tyranitar", mode: "mega" },

  // Gen 3
  254: { megaId: 10065, displayName: "Mega Sceptile", mode: "mega" },
  257: { megaId: 10050, displayName: "Mega Blaziken", mode: "mega" },
  260: { megaId: 10064, displayName: "Mega Swampert", mode: "mega" },
  282: { megaId: 10051, displayName: "Mega Gardevoir", mode: "mega" },
  302: { megaId: 10066, displayName: "Mega Sableye", mode: "mega" },
  303: { megaId: 10052, displayName: "Mega Mawile", mode: "mega" },
  304: { megaId: 10053, displayName: "Mega Aggron", mode: "mega" },
  306: { megaId: 10053, displayName: "Mega Aggron", mode: "mega" },
  308: { megaId: 10054, displayName: "Mega Medicham", mode: "mega" },
  310: { megaId: 10055, displayName: "Mega Manectric", mode: "mega" },
  319: { megaId: 10070, displayName: "Mega Sharpedo", mode: "mega" },
  323: { megaId: 10087, displayName: "Mega Camerupt", mode: "mega" },
  334: { megaId: 10067, displayName: "Mega Altaria", mode: "mega" },
  354: { megaId: 10056, displayName: "Mega Banette", mode: "mega" },
  359: { megaId: 10057, displayName: "Mega Absol", mode: "mega" },
  362: { megaId: 10074, displayName: "Mega Glalie", mode: "mega" },
  373: { megaId: 10089, displayName: "Mega Salamence", mode: "mega" },
  376: { megaId: 10076, displayName: "Mega Metagross", mode: "mega" },
  380: { megaId: 10062, displayName: "Mega Latias", mode: "mega" },
  381: { megaId: 10063, displayName: "Mega Latios", mode: "mega" },
  382: { megaId: 10077, displayName: "Primal Kyogre", mode: "primal" },
  383: { megaId: 10078, displayName: "Primal Groudon", mode: "primal" },
  384: { megaId: 10079, displayName: "Mega Rayquaza", mode: "mega" },

  // Gen 4
  428: { megaId: 10088, displayName: "Mega Lopunny", mode: "mega" },
  445: { megaId: 10058, displayName: "Mega Garchomp", mode: "mega" },
  448: { megaId: 10059, displayName: "Mega Lucario", mode: "mega" },
  460: { megaId: 10060, displayName: "Mega Abomasnow", mode: "mega" },
  475: { megaId: 10068, displayName: "Mega Gallade", mode: "mega" },

  // Gen 5 & 6
  531: { megaId: 10069, displayName: "Mega Audino", mode: "mega" },
  719: { megaId: 10075, displayName: "Mega Diancie", mode: "mega" },
};

export interface ResolvedOverdrive {
  spriteId: number;
  displayName: string;
  badge: string;
  isCustomMega: boolean;
  mode: "mega" | "gmax" | "primal" | "surge";
}

export function resolveOverdrive(speciesId: number | null, baseName: string): ResolvedOverdrive {
  if (!speciesId) {
    return {
      spriteId: 1,
      displayName: baseName,
      badge: "⚡ OVERDRIVE",
      isCustomMega: false,
      mode: "surge",
    };
  }

  const found = MEGA_FORMS[speciesId];
  if (found) {
    return {
      spriteId: found.megaId,
      displayName: found.displayName,
      badge: found.mode === "gmax" ? "⚡ G-MAX" : found.mode === "primal" ? "🌊 PRIMAL" : "🧬 MEGA",
      isCustomMega: true,
      mode: found.mode,
    };
  }

  return {
    spriteId: speciesId,
    displayName: `${baseName} (Overdrive ⚡)`,
    badge: "⚡ OVERDRIVE",
    isCustomMega: false,
    mode: "surge",
  };
}
