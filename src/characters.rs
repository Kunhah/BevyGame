//! Playable protagonist roster.
//!
//! The party is drawn from a fixed cast of named characters, but only ever four
//! fight at once (see `constants::MAX_OBJECTS`), so the roster being larger than
//! the party is what gives a run its identity: which four you bring decides how
//! the game plays.
//!
//! Each [`CharacterKind`] bundles the data that makes one protagonist
//! mechanically distinct:
//!   * cosmetic identity (display name, class label, placeholder tint),
//!   * a starting combat stat block ([`CombatStats`]) and growth profile
//!     ([`GrowthAttributes`]),
//!   * the skill trees they may learn from ([`SkillTreeAccess`] = their class
//!     tree + the magic schools they have affinity for + the universal trees),
//!   * their starting [`Abilities`], and
//!   * any signature mechanic (Toshiko's Kuro [`ExtraHp`] pool).
//!
//! The overworld tags each party ally with its `CharacterKind`
//! ([`crate::world`]); when a battle starts, [`crate::battle::spawn_ally_combat`]
//! reads the kind back and materialises the matching stat block + access. Allies
//! with no `CharacterKind` fall back to a generic block (ambient/test allies).
//!
//! The numbers for the original four were lifted from the GDD-tuned (but
//! previously unwired) `combat_plugin::spawn_examples`; the three later
//! additions (Renjiro / Suzuka / Kanzo) are tuned to the same scale and pair
//! with the class trees of the same name under `assets/data/skills/`.

use bevy::ecs::system::EntityCommands;
use bevy::prelude::*;

use crate::combat_ability::MagicSchool;
use crate::combat_plugin::{
    Abilities, AccessoryType, ArmorType, CharacterId, CombatStats, EquipmentLoadout,
    EquipmentSlotType, EquipmentType, ExtraHp, GrowthAttributes, GrowthCurve, Inventory,
    MagicDistribution, RogueBehavior, SpiritMediumBehavior, StatPool, WeaponType,
};
use crate::constants::DEFAULT_ACTION_POINTS;
use crate::skill_tree::{
    LearnedSkills, MagicCostMultipliers, SkillPoints, SkillTreeAccess, SkillTreeKind,
};

/// One playable protagonist. Carried as a component on both the overworld ally
/// entity and (after wiring) its in-battle combatant, so HUD/identity systems
/// can read who a unit is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CharacterKind {
    /// Rogue / kunoichi — Kiho shinobi: shinobigatana, bo-shuriken, matchlock
    /// pistol, caltrops, smoke, and the substitution art.
    Rina,
    /// Cleric / Kitsune — Kamishin + Onmyodo support.
    Sayaka,
    /// Samurai — Kiho/Yokaijutsu front-line bruiser.
    Houjou,
    /// Vessel — Yokaijutsu caster with the Kuro extra-HP pact.
    Toshiko,
    /// Monk (yamabushi) — Kiho staff striker, mobile front-line.
    Renjiro,
    /// Onmyoji — Onmyodo controller/summoner: gogyō element seals, binding
    /// ofuda, hexes, and a commanded paper shikigami.
    Suzuka,
    /// Exorcist (biwa hōshi) — Kamishin back-line glass cannon.
    Kanzo,
}

impl CharacterKind {
    /// Every protagonist, in roster order. Handy for menus / roster screens.
    pub const ALL: [CharacterKind; 7] = [
        CharacterKind::Rina,
        CharacterKind::Sayaka,
        CharacterKind::Houjou,
        CharacterKind::Toshiko,
        CharacterKind::Renjiro,
        CharacterKind::Suzuka,
        CharacterKind::Kanzo,
    ];

    /// Stable per-character id (matches the `CharacterId` used in the legacy
    /// example spawn for the original four).
    pub fn character_id(self) -> u32 {
        match self {
            CharacterKind::Rina => 1,
            CharacterKind::Sayaka => 2,
            CharacterKind::Houjou => 3,
            CharacterKind::Toshiko => 4,
            CharacterKind::Renjiro => 5,
            CharacterKind::Suzuka => 6,
            CharacterKind::Kanzo => 7,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            CharacterKind::Rina => "Rina",
            CharacterKind::Sayaka => "Sayaka",
            CharacterKind::Houjou => "Houjou Utaka",
            CharacterKind::Toshiko => "Toshiko",
            CharacterKind::Renjiro => "Renjiro",
            CharacterKind::Suzuka => "Suzuka",
            CharacterKind::Kanzo => "Kanzo",
        }
    }

    pub fn class_label(self) -> &'static str {
        match self {
            CharacterKind::Rina => "Rogue",
            CharacterKind::Sayaka => "Cleric",
            CharacterKind::Houjou => "Samurai",
            CharacterKind::Toshiko => "Vessel",
            CharacterKind::Renjiro => "Monk",
            CharacterKind::Suzuka => "Onmyoji",
            CharacterKind::Kanzo => "Exorcist",
        }
    }

    /// Placeholder capsule tint until real art exists. Linear sRGB.
    pub fn color(self) -> Color {
        match self {
            CharacterKind::Rina => Color::srgb(0.85, 0.75, 0.25),
            CharacterKind::Sayaka => Color::srgb(0.95, 0.55, 0.65),
            CharacterKind::Houjou => Color::srgb(0.60, 0.20, 0.20),
            CharacterKind::Toshiko => Color::srgb(0.45, 0.30, 0.55),
            CharacterKind::Renjiro => Color::srgb(0.80, 0.50, 0.20),
            CharacterKind::Suzuka => Color::srgb(0.30, 0.32, 0.62),
            CharacterKind::Kanzo => Color::srgb(0.85, 0.85, 0.92),
        }
    }

    /// The character's own class skill tree.
    pub fn class_tree(self) -> SkillTreeKind {
        match self {
            CharacterKind::Rina => SkillTreeKind::RinaRogue,
            CharacterKind::Sayaka => SkillTreeKind::SayakaCleric,
            CharacterKind::Houjou => SkillTreeKind::HoujouSamurai,
            CharacterKind::Toshiko => SkillTreeKind::ToshikoVessel,
            CharacterKind::Renjiro => SkillTreeKind::RenjiroMonk,
            CharacterKind::Suzuka => SkillTreeKind::SuzukaOnmyoji,
            CharacterKind::Kanzo => SkillTreeKind::KanzoExorcist,
        }
    }

    /// Magic schools this character has affinity for (gates their magic trees).
    pub fn magic_affinities(self) -> &'static [MagicSchool] {
        match self {
            CharacterKind::Rina => &[MagicSchool::Kiho],
            CharacterKind::Sayaka => &[MagicSchool::Kamishin, MagicSchool::Onmyodo],
            CharacterKind::Houjou => &[MagicSchool::Kiho, MagicSchool::Yokaijutsu],
            CharacterKind::Toshiko => &[MagicSchool::Yokaijutsu],
            CharacterKind::Renjiro => &[MagicSchool::Kiho],
            CharacterKind::Suzuka => &[MagicSchool::Onmyodo],
            CharacterKind::Kanzo => &[MagicSchool::Kamishin],
        }
    }

    /// Full learn allowlist: universal trees + magic affinities + class tree.
    pub fn skill_access(self) -> SkillTreeAccess {
        SkillTreeAccess::new()
            .with_universal()
            .with_magic(self.magic_affinities().iter().copied())
            .with(self.class_tree())
    }

    /// Starting abilities (ids into `assets/data/abilities/AbilitiesExample.ron`).
    pub fn abilities(self) -> Vec<u16> {
        match self {
            // Core: Shinobigatana, Bo-shuriken, Tanzutsu, Ramrod, Kawarimi,
            // Makibishi, Kemuri-dama, Ansatsu. Extras (0x5008+): Metsubushi,
            // Kusarigama, Shinobi-aruki, Poisoned Blade, Happō Shuriken, Quickstep.
            CharacterKind::Rina => vec![
                20480, 20481, 20482, 20483, 20484, 20485, 20486, 20487,
                20488, 20489, 20490, 20491, 20492, 20493,
            ],
            // Core: Kitsune-bi, Inari's Boon, Fox Glamour, Harae, Foxfire Lanterns.
            // Extras (0x5805+): Dakini's Boon, Ninetail Foxfire, Beguile,
            // Inari's Aegis, Searing Foxflame, Rite of Wards.
            CharacterKind::Sayaka => vec![
                22528, 22529, 22530, 22531, 22532,
                22533, 22534, 22535, 22536, 22537, 22538,
            ],
            // Core: Kesa-giri, Yoko-giri, Iai, Sutemi, Magakiri, Sakazuki, Reibaku.
            // Extras (0x6007+): Tsubame-gaeshi, Munen Musō, Zanshin, Kabuto-wari,
            // Bloodthirst Blade, Oni's Roar.
            CharacterKind::Houjou => vec![
                24576, 24577, 24578, 24579, 24580, 24581, 24582,
                24583, 24584, 24585, 24586, 24587, 24588,
            ],
            // Core: Kuro's Touch/Whisper, Reigan, Kuro's grasp, Tokoyo Veil, Shared
            // pain. Extras (0x6806+): Kuro's Jaws, Maddening Whisper, Lend Me Your
            // Strength, Umbral Step, Curse of Kuro, Two-Souls Surge.
            CharacterKind::Toshiko => vec![
                26624, 26625, 26626, 26627, 26628, 26629,
                26630, 26631, 26632, 26633, 26634, 26635,
            ],
            // Core: Naginata Arc/Thrust, Yamabushi Breath, Hamaya, Kabura-ya,
            // Fudō's Severance. Extras (0x7020+): Ishizuki, Tomoe Guard, Yatate
            // Volley, Heart-Seeker, Goma Flame, Conch Blast.
            CharacterKind::Renjiro => vec![
                28672, 28673, 28674, 28675, 28676, 28677,
                28704, 28705, 28706, 28707, 28708, 28709,
            ],
            // Core: Ofuda Dart, Cinnabar Bolt, Kekkai, Binding Seal, Curse Ofuda,
            // Bind Shikigami. Extras (0x7028+): Gofu Volley, Kuji-kiri, Hitogata
            // Transfer, Reading of Fate, Origami Blades, Greater Shikigami.
            CharacterKind::Suzuka => vec![
                28680, 28681, 28682, 28683, 28684, 28685, 28686, 28687,
                28712, 28713, 28714, 28715, 28716, 28717, 28718, 28719,
            ],
            // Core: Kotodama, Kiyome Flame, Onusa Ward, Heike Dirge, Goryō Seal,
            // Ōharae. Extras (0x7030+): Heart Sutra, Body Sutras, Gohei Sweep,
            // Tama-shizume, Shakujō Toll, Chinkonsai.
            CharacterKind::Kanzo => vec![
                28688, 28689, 28690, 28691, 28692, 28693,
                28720, 28721, 28722, 28723, 28724, 28725,
            ],
        }
    }

    /// Equipment the character may wield — the allowed-types skeleton (no items
    /// pre-equipped). This gates what each protagonist can wear (Rina can't don
    /// heavy armor; casters are staff-and-robe), which is part of their identity.
    pub fn equipment_loadout(self) -> EquipmentLoadout {
        use AccessoryType::*;
        use ArmorType::*;
        use EquipmentType::{Accessory, Armor, Weapon};
        use WeaponType::*;
        let slots: Vec<(EquipmentSlotType, Vec<EquipmentType>)> = match self {
            CharacterKind::Rina => vec![
                (EquipmentSlotType::Weapon, vec![Weapon(Dagger)]),
                (EquipmentSlotType::Armor, vec![Armor(LightArmor)]),
                (EquipmentSlotType::Accessory, vec![Accessory(Ring)]),
                (EquipmentSlotType::Accessory, vec![Accessory(Charm)]),
            ],
            CharacterKind::Sayaka => vec![
                (EquipmentSlotType::Weapon, vec![Weapon(Staff)]),
                (EquipmentSlotType::Armor, vec![Armor(Robe)]),
                (EquipmentSlotType::Accessory, vec![Accessory(Charm)]),
            ],
            CharacterKind::Houjou => vec![
                (EquipmentSlotType::Weapon, vec![Weapon(Sword)]),
                (EquipmentSlotType::Armor, vec![Armor(HeavyArmor)]),
                (EquipmentSlotType::Accessory, vec![Accessory(Charm)]),
            ],
            CharacterKind::Toshiko => vec![
                (EquipmentSlotType::Weapon, vec![Weapon(Staff)]),
                (EquipmentSlotType::Armor, vec![Armor(Robe)]),
                (EquipmentSlotType::Accessory, vec![Accessory(Charm), Accessory(Relic)]),
            ],
            // Renjiro — sōhei/yamabushi: naginata in melee or the longbow at
            // range, monk's robe or light armor.
            CharacterKind::Renjiro => vec![
                (EquipmentSlotType::Weapon, vec![Weapon(Naginata), Weapon(Bow)]),
                (EquipmentSlotType::Armor, vec![Armor(Robe), Armor(LightArmor)]),
                (EquipmentSlotType::Accessory, vec![Accessory(Charm)]),
            ],
            // Suzuka — onmyōji: a ritual shaku (staff), kariginu robe, and an
            // ofuda relic or charm.
            CharacterKind::Suzuka => vec![
                (EquipmentSlotType::Weapon, vec![Weapon(Staff)]),
                (EquipmentSlotType::Armor, vec![Armor(Robe)]),
                (EquipmentSlotType::Accessory, vec![Accessory(Charm), Accessory(Relic)]),
            ],
            // Kanzo — exorcist: priest's staff, robe, and a relic or charm.
            CharacterKind::Kanzo => vec![
                (EquipmentSlotType::Weapon, vec![Weapon(Staff)]),
                (EquipmentSlotType::Armor, vec![Armor(Robe)]),
                (EquipmentSlotType::Accessory, vec![Accessory(Charm), Accessory(Relic)]),
            ],
        };
        EquipmentLoadout::with_allowed_types(slots)
    }

    /// Starting inventory item ids (mirrors the legacy example data for the
    /// original four; the later three start empty).
    pub fn inventory(self) -> Inventory {
        let item_ids = match self {
            CharacterKind::Rina => vec![1001],
            CharacterKind::Sayaka => vec![5001],
            CharacterKind::Toshiko => vec![1002],
            _ => vec![],
        };
        Inventory { item_ids }
    }

    /// Per-character level-up growth curve (stat-gain multipliers). Casters lean
    /// on `spirit_mage_curve`, agile strikers on `rogue_curve`, the rest default.
    pub fn growth_curve(self) -> GrowthCurve {
        match self {
            CharacterKind::Rina | CharacterKind::Renjiro => GrowthCurve::rogue_curve(),
            CharacterKind::Toshiko | CharacterKind::Kanzo | CharacterKind::Suzuka => {
                GrowthCurve::spirit_mage_curve()
            }
            _ => GrowthCurve::default(),
        }
    }

    /// Insert every component that makes this character *itself* onto a freshly
    /// spawned combatant: stats, growth, abilities, skill access + machinery,
    /// equipment, inventory, identity tags, and any signature mechanic. The
    /// caller is responsible for battle scaffolding (BattleParticipant, side,
    /// turn-order, transform, name, etc.).
    pub fn insert_combat_components(self, e: &mut EntityCommands) {
        e.insert(self.combat_stats());
        e.insert(self.growth());
        e.insert(self.growth_curve());
        e.insert(Abilities(self.abilities()));
        e.insert(self.equipment_loadout());
        e.insert(self.inventory());
        e.insert(self.skill_access());
        e.insert(SkillPoints::default());
        e.insert(LearnedSkills::default());
        e.insert(MagicCostMultipliers::default());
        e.insert(CharacterId(self.character_id()));
        e.insert(self); // the CharacterKind tag itself
        if let Some(extra) = self.extra_hp() {
            e.insert(extra);
        }
        // Class behaviour markers — passive on PlayerControlled units (they grant
        // bonuses without taking over turns). SpiritMediumBehavior is what routes
        // damage through Toshiko's Kuro ExtraHp pool, so the two go together.
        match self {
            CharacterKind::Rina => {
                e.insert(RogueBehavior);
            }
            CharacterKind::Toshiko => {
                e.insert(SpiritMediumBehavior);
            }
            _ => {}
        }
    }

    /// Toshiko's Kuro pact grants a separate damage-absorbing HP pool. Other
    /// characters have no extra pool.
    pub fn extra_hp(self) -> Option<ExtraHp> {
        match self {
            CharacterKind::Toshiko => Some(ExtraHp { current: 40, max: 40 }),
            _ => None,
        }
    }

    /// Starting combat stat block. Numbers for the original four are the
    /// GDD-tuned values from the legacy example spawn.
    pub fn combat_stats(self) -> CombatStats {
        // Local shorthand to keep each block readable.
        let s = |base: i32| <StatPool<i32>>::new(base);
        let m = |base: f32| <StatPool<f32>>::new(base);
        match self {
            CharacterKind::Rina => CombatStats {
                health: s(41),
                morale: s(62),
                action_points: s(DEFAULT_ACTION_POINTS + 1), // GDD: extra AP
                movement: s(7),
                kiho: m(4.0),
                onmyodo: m(0.0),
                yokaijutsu: m(0.0),
                kamishin: m(0.0),
                lethality: s(25),
                hit: s(32),
                armor: s(7),
                speed: s(37),
                evasion: s(37),
                mind: s(3),
                health_per_rest_hour: 1,
                morale_per_rest_hour: 4,
                kiho_per_rest_hour: 0.25,
                onmyodo_per_rest_hour: 0.0,
                yokaijutsu_per_rest_hour: 0.0,
                kamishin_per_rest_hour: 0.0,
            },
            CharacterKind::Sayaka => CombatStats {
                health: s(52),
                morale: s(70),
                action_points: s(DEFAULT_ACTION_POINTS),
                movement: s(5),
                kiho: m(0.0),
                onmyodo: m(5.0),
                yokaijutsu: m(0.0),
                kamishin: m(6.0),
                lethality: s(12),
                hit: s(20),
                armor: s(10),
                speed: s(18),
                evasion: s(18),
                mind: s(22),
                health_per_rest_hour: 2,
                morale_per_rest_hour: 5,
                kiho_per_rest_hour: 0.0,
                onmyodo_per_rest_hour: 0.5,
                yokaijutsu_per_rest_hour: 0.0,
                kamishin_per_rest_hour: 0.6,
            },
            CharacterKind::Houjou => CombatStats {
                health: s(68),
                morale: s(55),
                action_points: s(DEFAULT_ACTION_POINTS),
                movement: s(5),
                kiho: m(2.0),
                onmyodo: m(0.0),
                yokaijutsu: m(3.0),
                kamishin: m(0.0),
                lethality: s(34),
                hit: s(28),
                armor: s(18),
                speed: s(22),
                evasion: s(22),
                mind: s(8),
                health_per_rest_hour: 2,
                morale_per_rest_hour: 3,
                kiho_per_rest_hour: 0.15,
                onmyodo_per_rest_hour: 0.0,
                yokaijutsu_per_rest_hour: 0.2,
                kamishin_per_rest_hour: 0.0,
            },
            CharacterKind::Toshiko => CombatStats {
                health: s(44),
                morale: s(48),
                action_points: s(DEFAULT_ACTION_POINTS),
                movement: s(5),
                kiho: m(0.0),
                onmyodo: m(0.0),
                yokaijutsu: m(5.0),
                kamishin: m(0.0),
                lethality: s(16),
                hit: s(18),
                armor: s(6),
                speed: s(20),
                evasion: s(20),
                mind: s(20),
                health_per_rest_hour: 1,
                morale_per_rest_hour: 3,
                kiho_per_rest_hour: 0.0,
                onmyodo_per_rest_hour: 0.0,
                yokaijutsu_per_rest_hour: 0.5,
                kamishin_per_rest_hour: 0.0,
            },
            // Renjiro — mobile Kiho staff striker: durable, fast, high damage,
            // little magic breadth. Sits between Rina's fragility and Houjou's
            // armor.
            CharacterKind::Renjiro => CombatStats {
                health: s(58),
                morale: s(60),
                action_points: s(DEFAULT_ACTION_POINTS),
                movement: s(6),
                kiho: m(5.0),
                onmyodo: m(0.0),
                yokaijutsu: m(0.0),
                kamishin: m(0.0),
                lethality: s(28),
                hit: s(26),
                armor: s(12),
                speed: s(30),
                evasion: s(26),
                mind: s(6),
                health_per_rest_hour: 2,
                morale_per_rest_hour: 4,
                kiho_per_rest_hour: 0.5,
                onmyodo_per_rest_hour: 0.0,
                yokaijutsu_per_rest_hour: 0.0,
                kamishin_per_rest_hour: 0.0,
            },
            // Suzuka — Onmyodo controller/summoner: a fragile back-line caster
            // (deep Onmyodo + high Mind, light armour) who wins through seals,
            // hexes, and a commanded shikigami rather than direct damage.
            CharacterKind::Suzuka => CombatStats {
                health: s(46),
                morale: s(64),
                action_points: s(DEFAULT_ACTION_POINTS),
                movement: s(5),
                kiho: m(0.0),
                onmyodo: m(6.0),
                yokaijutsu: m(0.0),
                kamishin: m(0.0),
                lethality: s(9),
                hit: s(24),
                armor: s(7),
                speed: s(18),
                evasion: s(18),
                mind: s(24),
                health_per_rest_hour: 1,
                morale_per_rest_hour: 5,
                kiho_per_rest_hour: 0.0,
                onmyodo_per_rest_hour: 0.6,
                yokaijutsu_per_rest_hour: 0.0,
                kamishin_per_rest_hour: 0.0,
            },
            // Kanzo — Kamishin back-line glass cannon: fragile, low armor, high
            // Mind/Hit, deep Kamishin pool. The player-side mirror of the Kasha
            // caster.
            CharacterKind::Kanzo => CombatStats {
                health: s(40),
                morale: s(64),
                action_points: s(DEFAULT_ACTION_POINTS),
                movement: s(5),
                kiho: m(0.0),
                onmyodo: m(0.0),
                yokaijutsu: m(0.0),
                kamishin: m(6.0),
                lethality: s(8),
                hit: s(30),
                armor: s(5),
                speed: s(17),
                evasion: s(16),
                mind: s(24),
                health_per_rest_hour: 1,
                morale_per_rest_hour: 5,
                kiho_per_rest_hour: 0.0,
                onmyodo_per_rest_hour: 0.0,
                yokaijutsu_per_rest_hour: 0.0,
                kamishin_per_rest_hour: 0.6,
            },
        }
    }

    /// Per-character growth profile. Drives which stats climb on level-up; the
    /// `magic_distribution` keeps each character's spirit points inside their
    /// own school(s) so levelling reinforces identity. Soft GDD rule: the
    /// distribution sums to `3 * spirit`.
    pub fn growth(self) -> GrowthAttributes {
        let dist = |kiho, onmyodo, yokaijutsu, kamishin| MagicDistribution {
            kiho,
            onmyodo,
            yokaijutsu,
            kamishin,
        };
        match self {
            CharacterKind::Rina => GrowthAttributes {
                vitality: 8, endurance: 4, spirit: 6, power: 11, control: 11,
                celerity: 13, reflex: 13, insight: 6, resolve: 8,
                magic_distribution: dist(18, 0, 0, 0),
            },
            CharacterKind::Sayaka => GrowthAttributes {
                vitality: 9, endurance: 10, spirit: 12, power: 6, control: 8,
                celerity: 8, reflex: 9, insight: 12, resolve: 9,
                magic_distribution: dist(0, 16, 0, 20),
            },
            CharacterKind::Houjou => GrowthAttributes {
                vitality: 12, endurance: 6, spirit: 5, power: 13, control: 10,
                celerity: 7, reflex: 8, insight: 6, resolve: 12,
                magic_distribution: dist(9, 0, 6, 0),
            },
            CharacterKind::Toshiko => GrowthAttributes {
                vitality: 8, endurance: 6, spirit: 12, power: 8, control: 8,
                celerity: 9, reflex: 9, insight: 12, resolve: 8,
                magic_distribution: dist(0, 0, 36, 0),
            },
            CharacterKind::Renjiro => GrowthAttributes {
                vitality: 11, endurance: 6, spirit: 7, power: 11, control: 9,
                celerity: 12, reflex: 11, insight: 6, resolve: 11,
                magic_distribution: dist(21, 0, 0, 0),
            },
            CharacterKind::Suzuka => GrowthAttributes {
                vitality: 8, endurance: 6, spirit: 13, power: 6, control: 11,
                celerity: 8, reflex: 9, insight: 13, resolve: 9,
                magic_distribution: dist(0, 39, 0, 0),
            },
            CharacterKind::Kanzo => GrowthAttributes {
                vitality: 7, endurance: 6, spirit: 13, power: 6, control: 11,
                celerity: 8, reflex: 8, insight: 13, resolve: 9,
                magic_distribution: dist(0, 0, 0, 39),
            },
        }
    }
}

/// The party the player has chosen for this run. Element `0` is the leader (the
/// overworld [`crate::core::Player`] avatar); the rest are spawned as
/// [`crate::battle::WorldAlly`] companions. Capped to the party size
/// ([`crate::constants::MAX_OBJECTS`]) when consumed. Defaults to a balanced
/// skirmisher/support/tank/caster four so the game is playable even if the
/// selection screen is skipped (e.g. loading a save).
#[derive(Resource, Debug, Clone)]
pub struct SelectedParty(pub Vec<CharacterKind>);

impl Default for SelectedParty {
    fn default() -> Self {
        SelectedParty(vec![
            CharacterKind::Rina,
            CharacterKind::Sayaka,
            CharacterKind::Houjou,
            CharacterKind::Toshiko,
        ])
    }
}

impl SelectedParty {
    /// The leader (overworld avatar), or `None` if the party is empty.
    pub fn leader(&self) -> Option<CharacterKind> {
        self.0.first().copied()
    }

    /// The non-leader companions, capped to the remaining party slots.
    pub fn companions(&self) -> Vec<CharacterKind> {
        self.0
            .iter()
            .skip(1)
            .take(crate::constants::MAX_OBJECTS.saturating_sub(1))
            .copied()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every protagonist must be able to learn from their own class tree and
    /// from the universal trees, and must declare a non-empty stat block.
    #[test]
    fn every_character_has_coherent_access_and_stats() {
        for kind in CharacterKind::ALL {
            let access = kind.skill_access();
            assert!(
                access.allows(kind.class_tree()),
                "{:?} cannot learn its own class tree",
                kind
            );
            assert!(
                access.allows(SkillTreeKind::Martial),
                "{:?} missing universal Martial tree",
                kind
            );
            for school in kind.magic_affinities() {
                assert!(
                    access.allows(SkillTreeKind::from_magic_school(*school)),
                    "{:?} missing affinity tree for {:?}",
                    kind,
                    school
                );
            }
            let stats = kind.combat_stats();
            assert!(stats.health.base > 0, "{:?} has no health", kind);
        }
    }

    /// Every protagonist now ships with a starting ability set, and each id must
    /// decode to a level within the engine cap (5-bit level field, ≤ 30).
    #[test]
    fn every_character_has_abilities_within_level_cap() {
        for kind in CharacterKind::ALL {
            let abilities = kind.abilities();
            assert!(!abilities.is_empty(), "{:?} has no starting abilities", kind);
            for id in abilities {
                let level = id >> 11; // top 5 bits encode the ability's level
                assert!(level <= 30, "{:?} ability {id} decodes to level {level} > 30", kind);
            }
        }
    }

    /// The leader is element 0 and companions are capped to the remaining party
    /// slots, never exceeding `MAX_OBJECTS - 1`.
    #[test]
    fn selected_party_leader_and_companion_cap() {
        let party = SelectedParty(vec![
            CharacterKind::Kanzo,
            CharacterKind::Rina,
            CharacterKind::Suzuka,
            CharacterKind::Houjou,
            CharacterKind::Sayaka, // one beyond a 4-slot party
        ]);
        assert_eq!(party.leader(), Some(CharacterKind::Kanzo));
        assert_eq!(
            party.companions().len(),
            crate::constants::MAX_OBJECTS - 1,
            "companions must be capped to the non-leader slots",
        );
        assert!(!party.companions().contains(&CharacterKind::Kanzo), "leader must not also be a companion");

        let empty = SelectedParty(vec![]);
        assert_eq!(empty.leader(), None);
        assert!(empty.companions().is_empty());
    }

    /// The soft GDD rule: spirit yields 3 distribution points per point, and a
    /// character's allocation should not exceed that.
    #[test]
    fn magic_distribution_within_spirit_budget() {
        for kind in CharacterKind::ALL {
            let g = kind.growth();
            let allocated = g.magic_distribution.kiho as u32
                + g.magic_distribution.onmyodo as u32
                + g.magic_distribution.yokaijutsu as u32
                + g.magic_distribution.kamishin as u32;
            assert!(
                allocated <= 3 * g.spirit as u32,
                "{:?} over-allocates magic distribution ({} > 3*{})",
                kind,
                allocated,
                g.spirit
            );
        }
    }
}
