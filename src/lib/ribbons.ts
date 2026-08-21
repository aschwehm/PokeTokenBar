export interface RibbonDefinition {
  id: string;
  name: string;
  badge: string;
  icon: string;
  title: string;
  description: string;
  category: "journey" | "milestone" | "special" | "bond";
  color: string;
  glow: string;
}

export const ALL_RIBBONS: Record<string, RibbonDefinition> = {
  starter: {
    id: "starter",
    name: "Starter Ribbon",
    badge: "🐣 Starter",
    icon: "🎗️",
    title: "The Beginner's Mark",
    description: "Awarded to Pokémon that hatched from an egg and embarked on your coding journey.",
    category: "journey",
    color: "#39D98A",
    glow: "rgba(57, 217, 138, 0.4)",
  },
  affection: {
    id: "affection",
    name: "Best Buddy Ribbon",
    badge: "💖 Best Buddy",
    icon: "💖",
    title: "The Affectionate Partner",
    description: "Awarded for showing love, petting, and bonding with your companion.",
    category: "bond",
    color: "#FF007A",
    glow: "rgba(255, 0, 122, 0.45)",
  },
  gourmet: {
    id: "gourmet",
    name: "Gourmet Berry Ribbon",
    badge: "🍊 Berry Lover",
    icon: "🍊",
    title: "The Well-Fed Buddy",
    description: "Awarded for feeding Sitrus or Oran berries from the PokéShop.",
    category: "bond",
    color: "#F97316",
    glow: "rgba(249, 115, 22, 0.45)",
  },
  overdrive: {
    id: "overdrive",
    name: "Overdrive Surge Ribbon",
    badge: "⚡ Overdrive",
    icon: "⚡",
    title: "The High-Velocity Spark",
    description: "Awarded for sprinting in Mega Overdrive during intense Fast / Blazing burn tiers.",
    category: "special",
    color: "#00E5FF",
    glow: "rgba(0, 229, 255, 0.45)",
  },
  nightOwl: {
    id: "nightOwl",
    name: "Midnight Coder Ribbon",
    badge: "🌙 Night Owl",
    icon: "🌙",
    title: "The Nocturnal Companion",
    description: "Awarded for burning AI coding tokens late at night between midnight and 5 AM.",
    category: "special",
    color: "#C084FC",
    glow: "rgba(192, 132, 252, 0.45)",
  },
  bronzeBurner: {
    id: "bronzeBurner",
    name: "Bronze Burner Ribbon",
    badge: "🥉 10M Burner",
    icon: "🥉",
    title: "The 10M Token Contributor",
    description: "Awarded for burning over 10 Million AI tokens together in coding sessions.",
    category: "milestone",
    color: "#CD7F32",
    glow: "rgba(205, 127, 50, 0.4)",
  },
  silverBurner: {
    id: "silverBurner",
    name: "Silver Burner Ribbon",
    badge: "🥈 50M Burner",
    icon: "🥈",
    title: "The 50M Token Specialist",
    description: "Awarded for burning over 50 Million AI tokens together in coding sessions.",
    category: "milestone",
    color: "#E2E8F0",
    glow: "rgba(226, 232, 240, 0.45)",
  },
  goldBurner: {
    id: "goldBurner",
    name: "Gold Century Ribbon",
    badge: "🥇 100M Master",
    icon: "🥇",
    title: "The 100M Token Master",
    description: "Awarded for burning over 100 Million AI tokens together in coding sessions.",
    category: "milestone",
    color: "#FCD34D",
    glow: "rgba(252, 211, 77, 0.5)",
  },
  platinumBurner: {
    id: "platinumBurner",
    name: "Titan Burner Ribbon",
    badge: "👑 500M Titan",
    icon: "👑",
    title: "The 500M Token Legend",
    description: "Awarded for burning over 500 Million AI tokens together — a legendary coding companion!",
    category: "milestone",
    color: "#A78BFA",
    glow: "rgba(167, 139, 250, 0.5)",
  },
  graduate: {
    id: "graduate",
    name: "Hall of Fame Ribbon",
    badge: "🎓 Hall of Fame",
    icon: "🎓",
    title: "The Graduated Champion",
    description: "Awarded upon reaching final evolutionary stage and graduating into the Pokédex.",
    category: "journey",
    color: "#60A5FA",
    glow: "rgba(96, 165, 250, 0.45)",
  },
  shiny: {
    id: "shiny",
    name: "Star Sparkle Ribbon",
    badge: "✨ Shiny Star",
    icon: "✨",
    title: "The Radiant Star",
    description: "An ultra-rare Ribbon awarded exclusively to gleaming Shiny Pokémon.",
    category: "special",
    color: "#FBBF24",
    glow: "rgba(251, 191, 36, 0.55)",
  },
};

export const ORDERED_RIBBON_IDS = [
  "starter",
  "affection",
  "gourmet",
  "overdrive",
  "nightOwl",
  "bronzeBurner",
  "silverBurner",
  "goldBurner",
  "platinumBurner",
  "graduate",
  "shiny",
];

export function getRibbon(id: string): RibbonDefinition | undefined {
  return ALL_RIBBONS[id];
}
