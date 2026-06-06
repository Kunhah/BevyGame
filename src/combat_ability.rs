use std::cmp::Ordering;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use bevy::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::combat_plugin::{
    ActionCause, ApplyAttunementEvent, ApplyBuffEvent, ApplyPolarityFlipEvent, AttackIntentEvent,
    DamageQueue, DamageTag, DamageType, DrainMoraleEvent, HealEvent, QueuedDamage, Stat, SummonEvent,
};
use crate::gogyo::{Element, Phase};
use crate::status_effects::{ApplyStatusEvent, RemoveStatusEvent, ResourceKind, StatusKind};

/// Which kind of temporary combatant a [`AbilityEffect::Summon`] brings onto
/// the field. The concrete stat block / side / AI profile for each is built in
/// `crate::battle::spawn_summoned_combatant`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SummonKind {
    /// The onmyōji's paper familiar — a fragile, fast ally-side striker that
    /// acts on its own and expires after a few turns.
    Shikigami,
    /// A temporary impassable barrier conjured onto the field. Unlike a
    /// combatant it has no stats and never takes a turn — it is just a
    /// `Collider` (so pathfinding routes around it) that dissolves after a few
    /// *rounds*. Spawned by `crate::battle::spawn_summoned_obstacle`.
    SpiritWard,
    /// Passable hazard terrain: it does not block movement but slows anything
    /// crossing it and bites whoever steps onto it (on-pass damage).
    ThornBramble,
    /// Impassable wall that also burns: a damage aura singes enemies standing
    /// nearby at the end of each round.
    EmberWard,
    /// Passable cloud that does not slow or block, but afflicts enemies lingering
    /// within it with the Slowed bad condition each round (a status aura).
    HexMiasma,
}

impl SummonKind {
    /// Whether this summon is an inert obstacle (no stat block, no turn) rather
    /// than an autonomous combatant. Obstacles take a different spawn path and
    /// tick their lifetime on round-end instead of on their own turn-end.
    pub fn is_obstacle(self) -> bool {
        matches!(
            self,
            SummonKind::SpiritWard
                | SummonKind::ThornBramble
                | SummonKind::EmberWard
                | SummonKind::HexMiasma
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AbilityEffect {
    Heal { floor: u32, ceiling: u32, scaled_with: Stat },
    Damage {
        floor: u32,
        ceiling: u32,
        damage_type: DamageType,
        scaled_with: Stat,
        defended_with: Stat,
        /// "Sanity pressure" — how much the hit is amplified as the target's
        /// morale (their will/capacity to fight) is depleted. `0.0` (the
        /// default) means no scaling. `1.0` means up to +100% damage against a
        /// target at zero morale, scaling linearly: a target at full morale
        /// takes the unmodified hit, one at half morale takes +`factor/2`, etc.
        /// Toshiko's Kuro abilities use this to punish foes she has unnerved.
        #[serde(default)]
        amplify_low_morale: f32,
    },
    /// Directly siphon a target's **morale** — the mental "capacity to fight"
    /// resource (see [`crate::combat_plugin::CombatStats::morale`]). Unlike
    /// `Damage` (which drains health) this taps the will to fight itself, and
    /// pairs with `Damage { amplify_low_morale }` so softening a target's
    /// resolve makes the follow-up bite harder. `scaled_with` adds half the
    /// caster's stat on top of the rolled `floor..ceiling` base.
    DrainMorale { floor: u32, ceiling: u32, scaled_with: Stat },
    Buff {
        stat: Stat,
        multiplier: f32,
        effects: Option<Vec<u16>>,
        scaled_with: Stat,
    },
    /// Apply a Bad Condition / Debuff / Buff / Contract Debuff to each
    /// target. Default GDD duration is used (the apply system reads it from
    /// `default_expiry`); per-school resource focus is required for
    /// single-resource debuffs (Crippled Spirit, Starved, Focused Spirit,
    /// Overflowing Renewal).
    ApplyStatus {
        kind: StatusKind,
        tier: u8,
        #[serde(default)]
        resource_focus: Option<ResourceKind>,
    },
    /// Strip a specific status off each target (Sayaka's Cleanse, etc.).
    RemoveStatus { kind: StatusKind },
    /// Bring a temporary combatant onto the field beside the caster. Resolved
    /// out-of-band via [`SummonEvent`] (this fn has no `Commands`); the spawn /
    /// turn-order / expiry wiring lives in `crate::battle`. Fired once per cast,
    /// not per target.
    Summon { kind: SummonKind, lifetime_turns: u8 },
    /// 五行 lever — temporarily re-attune each target to `phase` on the Gogyō
    /// wheel for `duration` turns (changes their effective element for matchups
    /// and 生 support; see `crate::gogyo`). Onmyōdō's attunement seals.
    Attune { phase: Phase, duration: u8 },
    /// 五行 lever — temporarily flip each target's In/Yō polarity for `duration`
    /// turns (the Reversal Seal etc.; §3a of the design doc).
    FlipPolarity { duration: u8 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AbilityShape {
    Radius(f32),
    Line { length: f32, thickness: f32 },
    Cone { angle: f32, radius: f32 },
    Select,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default)]
pub enum MagicSchool {
    #[default]
    Kiho,
    Onmyodo,
    Yokaijutsu,
    Kamishin,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ability {
    pub id: u16,
    pub next_id: Option<u16>,
    pub name: String,
    pub health_cost: i32,
    pub magic_cost: f32,
    #[serde(default)]
    pub magic_school: MagicSchool,
    /// 五行 elemental nature of this ability on the Gogyō wheel, orthogonal to
    /// `magic_school` (the *source*) — it decides 剋/生 matchups and which
    /// phase-status it can proc. `None` = elementally neutral (no wheel
    /// interaction). Kiho abilities ignore this and inherit the caster's innate
    /// [`crate::combat_plugin::ElementalAffinity`]. Defaults to `None` so older
    /// ability data deserialises unchanged.
    #[serde(default)]
    pub element: Option<Element>,
    #[serde(alias = "stamina_cost")]
    pub action_point_cost: i32,
    pub cooldown: u8,
    pub description: String,
    pub effects: Vec<AbilityEffect>,
    pub shape: AbilityShape,
    pub duration: u8,
    pub targets: u8,
}

// ---------------------------------------------------------------------------
// Ability id packing
// ---------------------------------------------------------------------------
//
// An ability id is a `u16` split into a level (high bits) and a within-level
// sub-id (low bits): `id = (level << ID_BITS) | sub_id`. The split used to be
// 8/8 (256 levels, 256 ids/level); it is now 5/11 — 32 representable levels
// (capped at [`MAX_LEVEL`] = 30) and 2048 abilities per level. Packing the
// level into the high bits keeps the BST naturally grouped by level, which is
// what [`AbilityTree::find_all_level`] relies on.

/// Bits reserved for the level in a packed ability id.
pub const LEVEL_BITS: u16 = 5;
/// Bits reserved for the within-level sub-id.
pub const ID_BITS: u16 = 11;
/// Hard cap on character / ability levels. 5 level bits can represent 0..=31;
/// game logic clamps to 30.
pub const MAX_LEVEL: u8 = 30;
/// Largest representable sub-id (`2^ID_BITS - 1`).
pub const MAX_SUB_ID: u16 = (1 << ID_BITS) - 1;

const SUB_ID_MASK: u16 = (1 << ID_BITS) - 1;
const LEVEL_MASK: u16 = ((1 << LEVEL_BITS) - 1) << ID_BITS;

/// Pack a `(level, sub_id)` pair into an ability id. The level is clamped to
/// [`MAX_LEVEL`] and the sub-id to [`MAX_SUB_ID`] so the result always round-
/// trips through [`unpack_ability_id`].
pub fn pack_ability_id(level: u8, sub_id: u16) -> u16 {
    let level = (level.min(MAX_LEVEL) as u16) << ID_BITS;
    level | (sub_id & SUB_ID_MASK)
}

/// Split a packed ability id back into `(level, sub_id)`.
pub fn unpack_ability_id(id: u16) -> (u8, u16) {
    (((id & LEVEL_MASK) >> ID_BITS) as u8, id & SUB_ID_MASK)
}

impl Ability {
    pub fn get_level(&self) -> u8 {
        ((self.id & LEVEL_MASK) >> ID_BITS) as u8
    }
    pub fn get_sub_id(&self) -> u16 {
        self.id & SUB_ID_MASK
    }
}

#[derive(Clone)]
pub struct AbilityNode {
    pub ability: Ability,
    pub left: Option<Arc<RwLock<AbilityNode>>>,
    pub right: Option<Arc<RwLock<AbilityNode>>>,
}

impl AbilityNode {
    pub fn new(ability: Ability) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(AbilityNode {
            ability,
            left: None,
            right: None,
        }))
    }
}

#[derive(Resource, Clone)]
pub struct Ability_Tree(pub AbilityTree);

#[derive(Clone)]
pub struct AbilityTree {
    pub root: Option<Arc<RwLock<AbilityNode>>>,
}

impl AbilityTree {
    pub fn new() -> Self {
        AbilityTree { root: None }
    }

    pub fn insert(&mut self, ability: Ability) {
        let node = AbilityNode::new(ability.clone());

        match &self.root {
            None => self.root = Some(node),
            Some(root) => Self::insert_node(root.clone(), node),
        }
    }

    fn insert_node(current: Arc<RwLock<AbilityNode>>, new_node: Arc<RwLock<AbilityNode>>) {
        let Some(new_id) = read_guard(&new_node).map(|n| n.ability.id) else {
            return;
        };
        let Some(current_id) = read_guard(&current).map(|n| n.ability.id) else {
            return;
        };

        match new_id.cmp(&current_id) {
            Ordering::Less => {
                if let Some(left) = &read_guard(&current).and_then(|n| n.left.clone()) {
                    Self::insert_node(left.clone(), new_node);
                } else {
                    if let Some(mut w) = write_guard(&current) {
                        w.left = Some(new_node);
                    }
                }
            }
            Ordering::Greater => {
                if let Some(right) = &read_guard(&current).and_then(|n| n.right.clone()) {
                    Self::insert_node(right.clone(), new_node);
                } else {
                    if let Some(mut w) = write_guard(&current) {
                        w.right = Some(new_node);
                    }
                }
            }
            Ordering::Equal => {
                let Some(new_ability) = read_guard(&new_node).map(|n| n.ability.clone()) else {
                    return;
                };
                if let Some(mut w) = write_guard(&current) {
                    w.ability = new_ability;
                }
            }
        }
    }

    pub fn find(&self, id: u16) -> Option<Ability> {
        Self::find_node(self.root.clone(), id)
    }

    fn find_node(node: Option<Arc<RwLock<AbilityNode>>>, id: u16) -> Option<Ability> {
        if let Some(n) = node {
            let n_borrow = read_guard(&n)?;
            return if id == n_borrow.ability.id {
                Some(n_borrow.ability.clone())
            } else if id < n_borrow.ability.id {
                Self::find_node(n_borrow.left.clone(), id)
            } else {
                Self::find_node(n_borrow.right.clone(), id)
            };
        }
        None
    }

    pub fn find_all_level(&self, level: u8) -> Option<Vec<Ability>> {
        let mut current_node = self.root.clone();

        while let Some(n) = current_node {
            let n_borrow = read_guard(&n)?;
            if n_borrow.ability.get_level() == level {
                let mut results = Vec::new();
                Self::collect_level_abilities(self.root.clone(), level, &mut results);
                return Some(results);
            } else {
                current_node = n_borrow.right.clone();
            }
        }
        None
    }

    fn collect_level_abilities(
        node: Option<Arc<RwLock<AbilityNode>>>,
        level: u8,
        results: &mut Vec<Ability>,
    ) {
        if let Some(n) = node {
            let n_borrow = match read_guard(&n) {
                Some(guard) => guard,
                None => return,
            };
            results.push(n_borrow.ability.clone());

            Self::collect_level_abilities(n_borrow.left.clone(), level, results);
            Self::collect_level_abilities(n_borrow.right.clone(), level, results);
        }
    }

    pub fn traverse_all(&self) -> Vec<Ability> {
        let mut all = Vec::new();
        Self::collect_all(self.root.clone(), &mut all);
        all
    }

    fn collect_all(node: Option<Arc<RwLock<AbilityNode>>>, all: &mut Vec<Ability>) {
        if let Some(n) = node {
            if let Some(n_borrow) = read_guard(&n) {
                all.push(n_borrow.ability.clone());
                Self::collect_all(n_borrow.left.clone(), all);
                Self::collect_all(n_borrow.right.clone(), all);
            }
        }
    }
}

fn read_guard(node: &Arc<RwLock<AbilityNode>>) -> Option<RwLockReadGuard<'_, AbilityNode>> {
    match node.read() {
        Ok(guard) => Some(guard),
        Err(err) => {
            warn!("Ability tree read lock poisoned: {err}");
            None
        }
    }
}

fn write_guard(node: &Arc<RwLock<AbilityNode>>) -> Option<RwLockWriteGuard<'_, AbilityNode>> {
    match node.write() {
        Ok(guard) => Some(guard),
        Err(err) => {
            warn!("Ability tree write lock poisoned: {err}");
            None
        }
    }
}

pub fn handle_ability(
    caster: Entity,
    ability: &Ability,
    affected: &[Entity],
    now: u32,
    dq: &mut DamageQueue,
    attack_intent_events: &mut MessageWriter<AttackIntentEvent>,
    heal_events: &mut MessageWriter<HealEvent>,
    buff_events: &mut MessageWriter<ApplyBuffEvent>,
    apply_status_events: &mut MessageWriter<ApplyStatusEvent>,
    remove_status_events: &mut MessageWriter<RemoveStatusEvent>,
    summon_events: &mut MessageWriter<SummonEvent>,
    attune_events: &mut MessageWriter<ApplyAttunementEvent>,
    flip_events: &mut MessageWriter<ApplyPolarityFlipEvent>,
    drain_morale_events: &mut MessageWriter<DrainMoraleEvent>,
) {
    for (target_index, &target) in affected.iter().enumerate() {
        let cause = ActionCause::Ability { id: ability.id };
        for effect in &ability.effects {
            match effect {
                AbilityEffect::Heal { floor, ceiling, .. } => {
                    let amount = rand::rng().gen_range(*floor..*ceiling);
                    heal_events.write(HealEvent {
                        healer: caster,
                        target,
                        amount,
                        element: ability.element,
                        cause: cause.clone(),
                    });
                }
                AbilityEffect::DrainMorale { floor, ceiling, scaled_with } => {
                    let base = rand::rng().gen_range(*floor..*ceiling) as i32;
                    drain_morale_events.write(DrainMoraleEvent {
                        drainer: caster,
                        target,
                        amount: base,
                        scaled_with: *scaled_with,
                        cause: cause.clone(),
                    });
                }
                AbilityEffect::Damage {
                    floor,
                    ceiling,
                    damage_type,
                    scaled_with,
                    defended_with,
                    amplify_low_morale,
                } => {
                    let base = rand::rng().gen_range(*floor..*ceiling) as i32;

                    let mut tags = vec![DamageTag::FromAbility(ability.id)];
                    if *amplify_low_morale > 0.0 {
                        tags.push(DamageTag::AmplifyLowMorale(*amplify_low_morale));
                    }

                    dq.0.push(QueuedDamage {
                        attacker: caster,
                        target,
                        amount: base,
                        damage_type: *damage_type,
                        element: ability.element,
                        scaled_with: vec![(*scaled_with, 1.0)],
                        defended_with: vec![(*defended_with, 1.0)],
                        accuracy_override: None,
                        crit_multiplier: 1.0,
                        tags,
                        cause: cause.clone(),
                    });

                    attack_intent_events.write(AttackIntentEvent {
                        attacker: caster,
                        target,
                        ability: Some(ability.clone()),
                        context: crate::combat_plugin::AttackContext {
                            damage_type: Some(*damage_type),
                            ..Default::default()
                        },
                        cause: cause.clone(),
                    });
                }
                AbilityEffect::Buff {
                    stat,
                    multiplier,
                    effects,
                    ..
                } => {
                    buff_events.write(ApplyBuffEvent {
                        applier: caster,
                        target,
                        stat: *stat,
                        multiplier: *multiplier,
                        duration_in_ticks: ability.duration as u32,
                        additional_effects: effects.clone(),
                        applied_at: now,
                        element: ability.element,
                        cause: cause.clone(),
                    });
                }
                AbilityEffect::ApplyStatus {
                    kind,
                    tier,
                    resource_focus,
                } => {
                    apply_status_events.write(ApplyStatusEvent {
                        target,
                        kind: *kind,
                        tier: *tier,
                        source: Some(caster),
                        expiry_override: None,
                        resource_focus: *resource_focus,
                    });
                }
                AbilityEffect::RemoveStatus { kind } => {
                    remove_status_events.write(RemoveStatusEvent {
                        target,
                        kind: *kind,
                    });
                }
                AbilityEffect::Summon { kind, lifetime_turns } => {
                    // Caster-centric, not per-target: emit once per cast so a
                    // multi-target ability doesn't conjure a familiar per foe.
                    if target_index == 0 {
                        summon_events.write(SummonEvent {
                            summoner: caster,
                            kind: *kind,
                            lifetime_turns: *lifetime_turns,
                            target: affected.first().copied(),
                        });
                    }
                }
                AbilityEffect::Attune { phase, duration } => {
                    attune_events.write(ApplyAttunementEvent {
                        target,
                        phase: *phase,
                        duration: *duration as u32,
                        source: Some(caster),
                    });
                }
                AbilityEffect::FlipPolarity { duration } => {
                    flip_events.write(ApplyPolarityFlipEvent {
                        target,
                        duration: *duration as u32,
                        source: Some(caster),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_unpack_round_trips() {
        for level in 0..=MAX_LEVEL {
            for sub in [0u16, 1, 7, 255, 1000, MAX_SUB_ID] {
                let id = pack_ability_id(level, sub);
                assert_eq!(unpack_ability_id(id), (level, sub));
            }
        }
    }

    #[test]
    fn level_is_clamped_to_max() {
        // 5 bits can encode 31, but the cap is 30.
        assert_eq!(unpack_ability_id(pack_ability_id(31, 0)).0, MAX_LEVEL);
        assert_eq!(unpack_ability_id(pack_ability_id(200, 5)), (MAX_LEVEL, 5));
    }

    #[test]
    fn sub_id_does_not_bleed_into_level() {
        // Over-large sub-ids are masked, never corrupting the level field.
        let id = pack_ability_id(12, MAX_SUB_ID + 9);
        let (level, sub) = unpack_ability_id(id);
        assert_eq!(level, 12);
        assert!(sub <= MAX_SUB_ID);
    }

    /// The shipped ability data must deserialise and every id must decode to a
    /// level within the cap — guards the 5/11 re-mint against regressions.
    #[test]
    fn shipped_ability_data_parses_and_respects_cap() {
        let text = std::fs::read_to_string("assets/data/abilities/AbilitiesExample.ron")
            .expect("AbilitiesExample.ron exists");
        let abilities: Vec<Ability> =
            ron::de::from_str(&text).expect("AbilitiesExample.ron deserialises into Vec<Ability>");
        assert!(!abilities.is_empty());
        for a in &abilities {
            assert!(
                a.get_level() <= MAX_LEVEL,
                "ability {} ('{}') decodes to level {} > {MAX_LEVEL}",
                a.id,
                a.name,
                a.get_level(),
            );
        }
    }
}
