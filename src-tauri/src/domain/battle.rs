//! Pure battle domain models and calculation engine for PokeTokenBar v0.4.0.
//!
//! Provides type effectiveness calculations, signature 4-move set generation,
//! combat damage formulas, status stage mechanics, opponent matchmaking,
//! and turn-based battle resolution.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveCategory {
    Physical,
    Special,
    Status,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BattleMove {
    pub id: String,
    pub name: String,
    pub element: String,
    pub category: MoveCategory,
    pub power: u32,
    pub accuracy: u32,
    pub current_pp: u32,
    pub max_pp: u32,
    pub description: String,
    pub effect: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BattleFighter {
    pub species_id: i64,
    pub name: String,
    pub is_shiny: bool,
    pub level: u32,
    pub stage: u32,
    pub element_types: Vec<String>,
    pub max_hp: u32,
    pub current_hp: u32,
    pub attack: u32,
    pub defense: u32,
    pub sp_attack: u32,
    pub sp_defense: u32,
    pub speed: u32,
    pub ribbon_count: u32,
    pub is_overdrive: bool,
    pub atk_stage: i32,
    pub def_stage: i32,
    pub moves: Vec<BattleMove>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BattleLogEntry {
    pub id: String,
    pub text: String,
    pub actor: String, // "player" | "opponent" | "system"
    pub damage: Option<u32>,
    pub is_crit: bool,
    pub effectiveness: String, // "super" | "not_very" | "immune" | "normal"
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveBattleState {
    pub battle_id: String,
    pub turn_count: u32,
    pub player: BattleFighter,
    pub opponent: BattleFighter,
    pub is_player_turn: bool,
    pub battle_phase: String, // "selecting" | "resolving" | "won" | "lost" | "fled"
    pub battle_log: Vec<BattleLogEntry>,
    pub reward_bp: u32,
    pub reward_coins: u64,
    pub won: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BattleStatsRecord {
    pub wins: u32,
    pub losses: u32,
    pub win_streak: u32,
    pub best_streak: u32,
    pub total_battles: u32,
    pub total_bp_earned: u32,
}

impl BattleFighter {
    pub fn is_fainted(&self) -> bool {
        self.current_hp == 0
    }

    pub fn hp_percentage(&self) -> f32 {
        if self.max_hp == 0 {
            0.0
        } else {
            (self.current_hp as f32 / self.max_hp as f32) * 100.0
        }
    }
}

/// Calculate type advantage multiplier (0.0x, 0.5x, 1.0x, 2.0x).
pub fn type_multiplier(attacker_type: &str, defender_type: &str) -> f32 {
    let atk = attacker_type.trim().to_lowercase();
    let def = defender_type.trim().to_lowercase();

    match (atk.as_str(), def.as_str()) {
        // Normal
        ("normal", "rock") | ("normal", "steel") => 0.5,
        ("normal", "ghost") => 0.0,

        // Fire
        ("fire", "grass") | ("fire", "ice") | ("fire", "bug") | ("fire", "steel") => 2.0,
        ("fire", "fire") | ("fire", "water") | ("fire", "rock") | ("fire", "dragon") => 0.5,

        // Water
        ("water", "fire") | ("water", "ground") | ("water", "rock") => 2.0,
        ("water", "water") | ("water", "grass") | ("water", "dragon") => 0.5,

        // Grass
        ("grass", "water") | ("grass", "ground") | ("grass", "rock") => 2.0,
        ("grass", "fire")
        | ("grass", "grass")
        | ("grass", "poison")
        | ("grass", "flying")
        | ("grass", "bug")
        | ("grass", "dragon")
        | ("grass", "steel") => 0.5,

        // Electric
        ("electric", "water") | ("electric", "flying") => 2.0,
        ("electric", "electric") | ("electric", "grass") | ("electric", "dragon") => 0.5,
        ("electric", "ground") => 0.0,

        // Ice
        ("ice", "grass") | ("ice", "ground") | ("ice", "flying") | ("ice", "dragon") => 2.0,
        ("ice", "fire") | ("ice", "water") | ("ice", "ice") | ("ice", "steel") => 0.5,

        // Fighting
        ("fighting", "normal")
        | ("fighting", "ice")
        | ("fighting", "rock")
        | ("fighting", "dark")
        | ("fighting", "steel") => 2.0,
        ("fighting", "poison")
        | ("fighting", "flying")
        | ("fighting", "psychic")
        | ("fighting", "bug")
        | ("fighting", "fairy") => 0.5,
        ("fighting", "ghost") => 0.0,

        // Poison
        ("poison", "grass") | ("poison", "fairy") => 2.0,
        ("poison", "poison") | ("poison", "ground") | ("poison", "rock") | ("poison", "ghost") => {
            0.5
        }
        ("poison", "steel") => 0.0,

        // Ground
        ("ground", "fire")
        | ("ground", "electric")
        | ("ground", "poison")
        | ("ground", "rock")
        | ("ground", "steel") => 2.0,
        ("ground", "grass") | ("ground", "bug") => 0.5,
        ("ground", "flying") => 0.0,

        // Flying
        ("flying", "grass") | ("flying", "fighting") | ("flying", "bug") => 2.0,
        ("flying", "electric") | ("flying", "rock") | ("flying", "steel") => 0.5,

        // Psychic
        ("psychic", "fighting") | ("psychic", "poison") => 2.0,
        ("psychic", "psychic") | ("psychic", "steel") => 0.5,
        ("psychic", "dark") => 0.0,

        // Bug
        ("bug", "grass") | ("bug", "psychic") | ("bug", "dark") => 2.0,
        ("bug", "fire")
        | ("bug", "fighting")
        | ("bug", "poison")
        | ("bug", "flying")
        | ("bug", "ghost")
        | ("bug", "steel")
        | ("bug", "fairy") => 0.5,

        // Rock
        ("rock", "fire") | ("rock", "ice") | ("rock", "flying") | ("rock", "bug") => 2.0,
        ("rock", "fighting") | ("rock", "ground") | ("rock", "steel") => 0.5,

        // Ghost
        ("ghost", "psychic") | ("ghost", "ghost") => 2.0,
        ("ghost", "dark") => 0.5,
        ("ghost", "normal") => 0.0,

        // Dragon
        ("dragon", "dragon") => 2.0,
        ("dragon", "steel") => 0.5,
        ("dragon", "fairy") => 0.0,

        // Steel
        ("steel", "ice") | ("steel", "rock") | ("steel", "fairy") => 2.0,
        ("steel", "fire") | ("steel", "water") | ("steel", "electric") | ("steel", "steel") => 0.5,

        // Dark
        ("dark", "psychic") | ("dark", "ghost") => 2.0,
        ("dark", "fighting") | ("dark", "dark") | ("dark", "fairy") => 0.5,

        // Fairy
        ("fairy", "fighting") | ("fairy", "dragon") | ("fairy", "dark") => 2.0,
        ("fairy", "fire") | ("fairy", "poison") | ("fairy", "steel") => 0.5,

        _ => 1.0,
    }
}

/// Calculate overall effectiveness against single or dual-typed defenders.
pub fn total_type_effectiveness(attacker_type: &str, defender_types: &[String]) -> f32 {
    if defender_types.is_empty() {
        return 1.0;
    }
    let mut mult = 1.0;
    for def in defender_types {
        mult *= type_multiplier(attacker_type, def);
    }
    mult
}

/// Resolve primary & secondary types for a known species ID.
pub fn resolve_species_types(species_id: i64) -> Vec<String> {
    match species_id {
        1 | 2 | 3 => vec!["Grass".to_string(), "Poison".to_string()], // Bulbasaur line
        4 | 5 => vec!["Fire".to_string()],                            // Charmander line
        6 => vec!["Fire".to_string(), "Flying".to_string()],          // Charizard
        7 | 8 | 9 => vec!["Water".to_string()],                       // Squirtle line
        25 | 26 => vec!["Electric".to_string()],                      // Pikachu line
        133 => vec!["Normal".to_string()],                            // Eevee
        134 => vec!["Water".to_string()],                             // Vaporeon
        135 => vec!["Electric".to_string()],                          // Jolteon
        136 => vec!["Fire".to_string()],                              // Flareon
        150 | 151 => vec!["Psychic".to_string()],                     // Mewtwo / Mew
        220 | 221 | 473 => vec!["Ice".to_string(), "Ground".to_string()], // Swinub line
        92 | 93 | 94 => vec!["Ghost".to_string(), "Poison".to_string()], // Gengar line
        147 | 148 => vec!["Dragon".to_string()],                      // Dratini / Dragonair
        149 => vec!["Dragon".to_string(), "Flying".to_string()],      // Dragonite
        443 | 444 | 445 => vec!["Dragon".to_string(), "Ground".to_string()], // Garchomp line
        447 | 448 => vec!["Fighting".to_string(), "Steel".to_string()], // Lucario line
        246 | 247 | 248 => vec!["Rock".to_string(), "Dark".to_string()], // Tyranitar line
        280 | 281 | 282 => vec!["Psychic".to_string(), "Fairy".to_string()], // Gardevoir line
        143 => vec!["Normal".to_string()],                            // Snorlax
        130 => vec!["Water".to_string(), "Flying".to_string()],       // Gyarados
        63 | 64 | 65 => vec!["Psychic".to_string()],                  // Alakazam line
        212 => vec!["Bug".to_string(), "Steel".to_string()],          // Scizor
        58 | 59 => vec!["Fire".to_string()],                          // Arcanine
        371 | 372 | 373 => vec!["Dragon".to_string(), "Flying".to_string()], // Salamence line
        374 | 375 | 376 => vec!["Steel".to_string(), "Psychic".to_string()], // Metagross line
        255 | 256 | 257 => vec!["Fire".to_string(), "Fighting".to_string()], // Blaziken line
        656 | 657 | 658 => vec!["Water".to_string(), "Dark".to_string()], // Greninja line
        131 => vec!["Water".to_string(), "Ice".to_string()],          // Lapras
        66 | 67 | 68 => vec!["Fighting".to_string()],                 // Machamp line
        144 => vec!["Ice".to_string(), "Flying".to_string()],         // Articuno
        145 => vec!["Electric".to_string(), "Flying".to_string()],    // Zapdos
        146 => vec!["Fire".to_string(), "Flying".to_string()],        // Moltres
        _ => vec!["Normal".to_string()],
    }
}

/// Generate 4 signature moves based on elemental types.
pub fn generate_moveset_for_types(types: &[String]) -> Vec<BattleMove> {
    let primary = types.first().map(|s| s.as_str()).unwrap_or("Normal");
    let secondary = types.get(1).map(|s| s.as_str());

    let mut moves = Vec::new();

    // 1. Primary Powerful STAB Attack
    moves.push(match primary {
        "Fire" => BattleMove {
            id: "flamethrower".into(),
            name: "Flamethrower".into(),
            element: "Fire".into(),
            category: MoveCategory::Special,
            power: 90,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Scorches the target with an intense blast of fire.".into(),
            effect: None,
        },
        "Water" => BattleMove {
            id: "hydro_pump".into(),
            name: "Hydro Pump".into(),
            element: "Water".into(),
            category: MoveCategory::Special,
            power: 110,
            accuracy: 85,
            current_pp: 10,
            max_pp: 10,
            description: "Blasts a tremendous volume of pressurized water.".into(),
            effect: None,
        },
        "Grass" => BattleMove {
            id: "solar_beam".into(),
            name: "Solar Beam".into(),
            element: "Grass".into(),
            category: MoveCategory::Special,
            power: 110,
            accuracy: 100,
            current_pp: 10,
            max_pp: 10,
            description: "Gathers solar light and fires an intense beam.".into(),
            effect: None,
        },
        "Electric" => BattleMove {
            id: "thunderbolt".into(),
            name: "Thunderbolt".into(),
            element: "Electric".into(),
            category: MoveCategory::Special,
            power: 90,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Unleashes a powerful thunderbolt shock on the foe.".into(),
            effect: None,
        },
        "Ice" => BattleMove {
            id: "ice_beam".into(),
            name: "Ice Beam".into(),
            element: "Ice".into(),
            category: MoveCategory::Special,
            power: 90,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Fires an icy-cold beam of energy to freeze the target.".into(),
            effect: None,
        },
        "Psychic" => BattleMove {
            id: "psychic".into(),
            name: "Psychic".into(),
            element: "Psychic".into(),
            category: MoveCategory::Special,
            power: 90,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Telekinetic power crushes the foe with strong psychic force.".into(),
            effect: None,
        },
        "Ghost" => BattleMove {
            id: "shadow_ball".into(),
            name: "Shadow Ball".into(),
            element: "Ghost".into(),
            category: MoveCategory::Special,
            power: 80,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Hurls a shadowy blob at the target.".into(),
            effect: None,
        },
        "Dragon" => BattleMove {
            id: "dragon_pulse".into(),
            name: "Dragon Pulse".into(),
            element: "Dragon".into(),
            category: MoveCategory::Special,
            power: 85,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Attacks with a shock wave generated by a dragon's maw.".into(),
            effect: None,
        },
        "Fighting" => BattleMove {
            id: "close_combat".into(),
            name: "Close Combat".into(),
            element: "Fighting".into(),
            category: MoveCategory::Physical,
            power: 120,
            accuracy: 100,
            current_pp: 10,
            max_pp: 10,
            description: "Fights up close without guarding itself.".into(),
            effect: None,
        },
        "Dark" => BattleMove {
            id: "dark_pulse".into(),
            name: "Dark Pulse".into(),
            element: "Dark".into(),
            category: MoveCategory::Special,
            power: 80,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Releases a horrible aura imbued with dark thoughts.".into(),
            effect: None,
        },
        "Steel" => BattleMove {
            id: "flash_cannon".into(),
            name: "Flash Cannon".into(),
            element: "Steel".into(),
            category: MoveCategory::Special,
            power: 80,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Gathers light energy and unleashes it at once.".into(),
            effect: None,
        },
        "Fairy" => BattleMove {
            id: "moonblast".into(),
            name: "Moonblast".into(),
            element: "Fairy".into(),
            category: MoveCategory::Special,
            power: 95,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Borrows the power of the moon to blast the foe.".into(),
            effect: None,
        },
        "Ground" => BattleMove {
            id: "earthquake".into(),
            name: "Earthquake".into(),
            element: "Ground".into(),
            category: MoveCategory::Physical,
            power: 100,
            accuracy: 100,
            current_pp: 10,
            max_pp: 10,
            description: "Sets off an earthquake that strikes everything around.".into(),
            effect: None,
        },
        "Rock" => BattleMove {
            id: "stone_edge".into(),
            name: "Stone Edge".into(),
            element: "Rock".into(),
            category: MoveCategory::Physical,
            power: 100,
            accuracy: 80,
            current_pp: 10,
            max_pp: 10,
            description: "Stabs the target from below with sharpened stones.".into(),
            effect: None,
        },
        _ => BattleMove {
            id: "hyper_beam".into(),
            name: "Hyper Beam".into(),
            element: "Normal".into(),
            category: MoveCategory::Special,
            power: 120,
            accuracy: 90,
            current_pp: 5,
            max_pp: 5,
            description: "Fires a destructive concentrated beam of pure energy.".into(),
            effect: None,
        },
    });

    // 2. Secondary STAB or Coverage Heavy Attack
    let sec_type = secondary.unwrap_or(match primary {
        "Fire" => "Ground",
        "Water" => "Ice",
        "Grass" => "Poison",
        "Electric" => "Steel",
        "Ice" => "Water",
        "Psychic" => "Fairy",
        "Ghost" => "Dark",
        "Dragon" => "Fire",
        "Fighting" => "Rock",
        _ => "Fighting",
    });

    moves.push(match sec_type {
        "Fire" => BattleMove {
            id: "fire_blast".into(),
            name: "Fire Blast".into(),
            element: "Fire".into(),
            category: MoveCategory::Special,
            power: 110,
            accuracy: 85,
            current_pp: 10,
            max_pp: 10,
            description: "Incinerates with an intense star-shaped blast of fire.".into(),
            effect: None,
        },
        "Water" => BattleMove {
            id: "surf".into(),
            name: "Surf".into(),
            element: "Water".into(),
            category: MoveCategory::Special,
            power: 90,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Swamps the area around with a giant crashing wave.".into(),
            effect: None,
        },
        "Ice" => BattleMove {
            id: "blizzard".into(),
            name: "Blizzard".into(),
            element: "Ice".into(),
            category: MoveCategory::Special,
            power: 110,
            accuracy: 80,
            current_pp: 10,
            max_pp: 10,
            description: "Summons a howling blizzard that strikes with freezing cold.".into(),
            effect: None,
        },
        "Ground" => BattleMove {
            id: "earth_power".into(),
            name: "Earth Power".into(),
            element: "Ground".into(),
            category: MoveCategory::Special,
            power: 90,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Makes the ground under the target erupt with power.".into(),
            effect: None,
        },
        "Flying" => BattleMove {
            id: "air_slash".into(),
            name: "Air Slash".into(),
            element: "Flying".into(),
            category: MoveCategory::Special,
            power: 75,
            accuracy: 95,
            current_pp: 15,
            max_pp: 15,
            description: "Attacks with a blade of air that slices through the sky.".into(),
            effect: None,
        },
        "Poison" => BattleMove {
            id: "sludge_bomb".into(),
            name: "Sludge Bomb".into(),
            element: "Poison".into(),
            category: MoveCategory::Special,
            power: 90,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Unsanitary sludge is hurled at the target to inflict damage.".into(),
            effect: None,
        },
        "Fighting" => BattleMove {
            id: "brick_break".into(),
            name: "Brick Break".into(),
            element: "Fighting".into(),
            category: MoveCategory::Physical,
            power: 75,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Swift hard chop that shatters defenses.".into(),
            effect: None,
        },
        "Dark" => BattleMove {
            id: "crunch".into(),
            name: "Crunch".into(),
            element: "Dark".into(),
            category: MoveCategory::Physical,
            power: 80,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Crunches with vicious fangs and dark malice.".into(),
            effect: None,
        },
        "Steel" => BattleMove {
            id: "iron_head".into(),
            name: "Iron Head".into(),
            element: "Steel".into(),
            category: MoveCategory::Physical,
            power: 80,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Slams the target with a steel-hard head.".into(),
            effect: None,
        },
        "Fairy" => BattleMove {
            id: "dazzling_gleam".into(),
            name: "Dazzling Gleam".into(),
            element: "Fairy".into(),
            category: MoveCategory::Special,
            power: 80,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Emits a powerful flash of fairy light to dazzle the foe.".into(),
            effect: None,
        },
        _ => BattleMove {
            id: "body_slam".into(),
            name: "Body Slam".into(),
            element: "Normal".into(),
            category: MoveCategory::Physical,
            power: 85,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Drops its full body weight down on the target.".into(),
            effect: None,
        },
    });

    // 3. Quick Priority / Tactical Attack
    moves.push(BattleMove {
        id: "quick_attack".into(),
        name: "Quick Attack".into(),
        element: "Normal".into(),
        category: MoveCategory::Physical,
        power: 45,
        accuracy: 100,
        current_pp: 30,
        max_pp: 30,
        description: "Lunges at the target at blinding speed to strike first.".into(),
        effect: Some("priority".into()),
    });

    // 4. Tactical Buff / Recovery
    moves.push(match primary {
        "Grass" | "Fairy" => BattleMove {
            id: "synthesis".into(),
            name: "Synthesis".into(),
            element: "Grass".into(),
            category: MoveCategory::Status,
            power: 0,
            accuracy: 100,
            current_pp: 10,
            max_pp: 10,
            description: "Restores HP using synthesized natural vitality.".into(),
            effect: Some("heal_40".into()),
        },
        "Water" | "Ice" | "Normal" => BattleMove {
            id: "recover".into(),
            name: "Recover".into(),
            element: "Normal".into(),
            category: MoveCategory::Status,
            power: 0,
            accuracy: 100,
            current_pp: 10,
            max_pp: 10,
            description: "Restores up to half of max HP with cellular regeneration.".into(),
            effect: Some("heal_40".into()),
        },
        "Fighting" | "Dragon" | "Steel" => BattleMove {
            id: "swords_dance".into(),
            name: "Swords Dance".into(),
            element: "Normal".into(),
            category: MoveCategory::Status,
            power: 0,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "A frenetic war dance that sharply boosts Attack (+2).".into(),
            effect: Some("buff_atk_2".into()),
        },
        _ => BattleMove {
            id: "calm_mind".into(),
            name: "Calm Mind".into(),
            element: "Psychic".into(),
            category: MoveCategory::Status,
            power: 0,
            accuracy: 100,
            current_pp: 15,
            max_pp: 15,
            description: "Focuses the mind to sharply boost Special Attack & Defense.".into(),
            effect: Some("buff_atk_1".into()),
        },
    });

    moves
}

/// Generate a full `BattleFighter` given a species, name, stage, ribbons, and overdrive state.
pub fn build_fighter(
    species_id: i64,
    name: &str,
    is_shiny: bool,
    stage: u32,
    ribbon_count: u32,
    is_overdrive: bool,
) -> BattleFighter {
    let types = resolve_species_types(species_id);
    let moves = generate_moveset_for_types(&types);

    let level = match stage {
        1 => 25,
        2 => 50,
        _ => 75,
    };

    // Base stat foundation scaled by stage
    let base_hp = 80 + stage * 30;
    let base_atk = 70 + stage * 25;
    let base_def = 65 + stage * 20;
    let base_spa = 75 + stage * 25;
    let base_spd = 65 + stage * 20;
    let base_spe = 70 + stage * 20;

    // +5% bonus per ribbon unlocked
    let ribbon_multiplier = 1.0 + (ribbon_count as f32 * 0.05);

    // Overdrive aura boost: +20% HP and attack
    let overdrive_mult = if is_overdrive { 1.20 } else { 1.0 };

    let final_hp = ((base_hp as f32 * ribbon_multiplier * overdrive_mult) as u32).max(50);
    let final_atk = ((base_atk as f32 * ribbon_multiplier * overdrive_mult) as u32).max(40);
    let final_def = ((base_def as f32 * ribbon_multiplier) as u32).max(40);
    let final_spa = ((base_spa as f32 * ribbon_multiplier * overdrive_mult) as u32).max(40);
    let final_spd = ((base_spd as f32 * ribbon_multiplier) as u32).max(40);
    let final_spe = ((base_spe as f32 * ribbon_multiplier) as u32).max(40);

    BattleFighter {
        species_id,
        name: name.to_string(),
        is_shiny,
        level,
        stage,
        element_types: types,
        max_hp: final_hp,
        current_hp: final_hp,
        attack: final_atk,
        defense: final_def,
        sp_attack: final_spa,
        sp_defense: final_spd,
        speed: final_spe,
        ribbon_count,
        is_overdrive,
        atk_stage: 0,
        def_stage: 0,
        moves,
    }
}

/// Opponent roster entries for random Arena matchmaking.
pub const OPPONENT_ROSTER: &[(i64, &str, u32)] = &[
    (6, "Charizard", 3),
    (9, "Blastoise", 3),
    (3, "Venusaur", 3),
    (94, "Gengar", 3),
    (149, "Dragonite", 3),
    (248, "Tyranitar", 3),
    (445, "Garchomp", 3),
    (448, "Lucario", 2),
    (282, "Gardevoir", 3),
    (143, "Snorlax", 2),
    (130, "Gyarados", 2),
    (65, "Alakazam", 3),
    (212, "Scizor", 2),
    (59, "Arcanine", 2),
    (373, "Salamence", 3),
    (376, "Metagross", 3),
    (257, "Blaziken", 3),
    (658, "Greninja", 3),
    (131, "Lapras", 2),
    (150, "Mewtwo", 3),
];

/// Create a balanced random opponent matching the player's approximate tier.
pub fn generate_random_opponent(player_stage: u32) -> BattleFighter {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(42);

    let idx = (seed as usize) % OPPONENT_ROSTER.len();
    let (species_id, name, base_stage) = OPPONENT_ROSTER[idx];

    let shiny_roll = (seed % 100) < 5; // 5% shiny chance for opponent
    let opp_stage = player_stage.max(1).min(base_stage);

    build_fighter(species_id, name, shiny_roll, opp_stage, 1, false)
}

/// Damage resolution output.
pub struct AttackResult {
    pub damage: u32,
    pub is_crit: bool,
    pub effectiveness: f32,
    pub heal_amount: u32,
    pub buff_text: Option<String>,
}

/// Calculate attack damage & effects from attacker to defender.
pub fn calculate_attack(
    attacker: &BattleFighter,
    defender: &BattleFighter,
    mv: &BattleMove,
    is_crit_forced: bool,
) -> AttackResult {
    if mv.category == MoveCategory::Status {
        if let Some(ref eff) = mv.effect {
            if eff.starts_with("heal_") {
                let pct: u32 = eff.trim_start_matches("heal_").parse().unwrap_or(40);
                let heal = (attacker.max_hp * pct) / 100;
                return AttackResult {
                    damage: 0,
                    is_crit: false,
                    effectiveness: 1.0,
                    heal_amount: heal,
                    buff_text: Some(format!("Restored {} HP!", heal)),
                };
            } else if eff.starts_with("buff_atk_") {
                let stages: i32 = eff.trim_start_matches("buff_atk_").parse().unwrap_or(1);
                return AttackResult {
                    damage: 0,
                    is_crit: false,
                    effectiveness: 1.0,
                    heal_amount: 0,
                    buff_text: Some(format!("Attack rose by {} stage(s)!", stages)),
                };
            }
        }
        return AttackResult {
            damage: 0,
            is_crit: false,
            effectiveness: 1.0,
            heal_amount: 0,
            buff_text: Some("Prepared for battle!".into()),
        };
    }

    // Type effectiveness
    let eff = total_type_effectiveness(&mv.element, &defender.element_types);
    if eff == 0.0 {
        return AttackResult {
            damage: 0,
            is_crit: false,
            effectiveness: 0.0,
            heal_amount: 0,
            buff_text: None,
        };
    }

    // STAB (Same-Type Attack Bonus) = 1.5x
    let is_stab = attacker
        .element_types
        .iter()
        .any(|t| t.eq_ignore_ascii_case(&mv.element));
    let stab_mult = if is_stab { 1.5 } else { 1.0 };

    // Critical Hit Roll (6.25% default, 12.5% if Overdrive)
    let crit_threshold = if attacker.is_overdrive { 125 } else { 62 };
    let is_crit = is_crit_forced || ((attacker.speed + mv.power) % 1000 < crit_threshold);
    let crit_mult = if is_crit { 1.5 } else { 1.0 };

    // Stat stages
    let atk_stat = match mv.category {
        MoveCategory::Physical => {
            let mult = if attacker.atk_stage >= 0 {
                (2 + attacker.atk_stage) as f32 / 2.0
            } else {
                2.0 / (2 - attacker.atk_stage) as f32
            };
            (attacker.attack as f32 * mult) as u32
        }
        MoveCategory::Special => attacker.sp_attack,
        MoveCategory::Status => attacker.attack,
    };

    let def_stat = match mv.category {
        MoveCategory::Physical => {
            let mult = if defender.def_stage >= 0 {
                (2 + defender.def_stage) as f32 / 2.0
            } else {
                2.0 / (2 - defender.def_stage) as f32
            };
            (defender.defense as f32 * mult) as u32
        }
        MoveCategory::Special => defender.sp_defense,
        MoveCategory::Status => defender.defense,
    };

    let level = attacker.level as f32;
    let power = mv.power as f32;
    let ratio = atk_stat.max(10) as f32 / def_stat.max(10) as f32;

    // Classic Pokémon Damage Formula
    let base = (((2.0 * level / 5.0 + 2.0) * power * ratio) / 50.0) + 2.0;
    let raw_damage = base * stab_mult * eff * crit_mult;
    let final_dmg = (raw_damage as u32).max(1);

    AttackResult {
        damage: final_dmg,
        is_crit,
        effectiveness: eff,
        heal_amount: 0,
        buff_text: None,
    }
}

/// Choose an intelligent move for the opponent AI.
pub fn select_ai_move(ai: &BattleFighter, player: &BattleFighter) -> usize {
    if ai.moves.is_empty() {
        return 0;
    }

    // If low HP (< 30%), look for a heal move
    if ai.hp_percentage() < 30.0 {
        if let Some(pos) = ai.moves.iter().position(|m| {
            m.category == MoveCategory::Status
                && m.effect.as_deref().unwrap_or("").starts_with("heal_")
        }) {
            return pos;
        }
    }

    // Find move with highest effective damage
    let mut best_idx = 0;
    let mut best_score = 0.0;

    for (idx, m) in ai.moves.iter().enumerate() {
        if m.current_pp == 0 {
            continue;
        }
        let eff = total_type_effectiveness(&m.element, &player.element_types);
        let stab = if ai
            .element_types
            .iter()
            .any(|t| t.eq_ignore_ascii_case(&m.element))
        {
            1.5
        } else {
            1.0
        };
        let score = (m.power as f32) * eff * stab;
        if score > best_score {
            best_score = score;
            best_idx = idx;
        }
    }

    best_idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_effectiveness_multipliers() {
        assert_eq!(type_multiplier("Water", "Fire"), 2.0);
        assert_eq!(type_multiplier("Fire", "Water"), 0.5);
        assert_eq!(type_multiplier("Electric", "Ground"), 0.0);
        assert_eq!(type_multiplier("Normal", "Ghost"), 0.0);
        assert_eq!(type_multiplier("Ice", "Dragon"), 2.0);

        // Dual type test (e.g. Swinub: Ice/Ground vs Fire -> Fire beats Ice 2x, Ground neutral 1x = 2.0x)
        let swinub_types = vec!["Ice".to_string(), "Ground".to_string()];
        assert_eq!(total_type_effectiveness("Fire", &swinub_types), 2.0);

        // Water vs Swinub (Ice neutral 1x, Ground weak 2x = 2.0x)
        assert_eq!(total_type_effectiveness("Water", &swinub_types), 2.0);
    }

    #[test]
    fn test_moveset_and_fighter_building() {
        let pikachu = build_fighter(25, "Pikachu", false, 1, 3, false);
        assert_eq!(pikachu.name, "Pikachu");
        assert_eq!(pikachu.moves.len(), 4);
        assert_eq!(pikachu.moves[0].element, "Electric");
        assert!(pikachu.max_hp >= 50);

        let charizard = build_fighter(6, "Charizard", true, 3, 5, true);
        assert_eq!(charizard.level, 75);
        assert!(charizard.is_shiny);
        assert!(charizard.is_overdrive);
        assert!(charizard.max_hp > pikachu.max_hp);
    }

    #[test]
    fn test_damage_calculation() {
        let pikachu = build_fighter(25, "Pikachu", false, 1, 0, false);
        let squirtle = build_fighter(7, "Squirtle", false, 1, 0, false);

        let thunderbolt = &pikachu.moves[0];
        let result = calculate_attack(&pikachu, &squirtle, thunderbolt, false);

        assert!(result.damage > 0);
        assert_eq!(result.effectiveness, 2.0); // Electric is 2x vs Water
    }

    #[test]
    fn test_ai_move_selection() {
        let ai_charizard = build_fighter(6, "Charizard", false, 3, 0, false);
        let player_venusaur = build_fighter(3, "Venusaur", false, 3, 0, false);

        let best_move_idx = select_ai_move(&ai_charizard, &player_venusaur);
        let chosen_move = &ai_charizard.moves[best_move_idx];
        assert_eq!(chosen_move.element, "Fire"); // Fire is super effective against Grass
    }
}
