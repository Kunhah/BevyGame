//! Skill trees.
//!
//! Each tree is authored as a RON file under `assets/data/skills/`. Trees come in
//! three groups:
//!
//! - **Magic-source** (`Kiho`, `Onmyodo`, `Yokaijutsu`, `Kamishin`): one
//!   per spell source. Tier-3 capstones unlock the per-school abilities at
//!   the bottom of `assets/data/abilities/AbilitiesExample.ron` (ids 0x1001/0x1101/
//!   0x1201/0x1301).
//! - **Universal non-magic** (`Martial`, `Survival`, `Bound`): cross-character
//!   trees themed around weapon use, exploration, and the Merchant's Contract.
//! - **Per-character class** (`RinaRogue`, `SayakaCleric`, `HoujouSamurai`,
//!   `ToshikoVessel`): signature trees per protagonist. The runtime does not
//!   currently restrict who can learn from these — UI is expected to filter.
//!
//! A character earns [`SkillPoints`] on level-up and spends them on
//! [`SkillNode`]s by firing a [`LearnSkillEvent`]. Learning validates
//! prerequisites, deducts SP, and applies the node's [`SkillEffect`]s
//! permanently — base-stat bonuses, per-school regen bumps, magic-cost
//! discounts, ability unlocks, and one-shot custom triggers.
//!
//! ## Spells vs. non-spell activations
//!
//! [`SkillEffect::UnlockAbility`] grants an `Ability` id from the unified
//! ability data ([`crate::combat_ability::Ability`]). That data type covers
//! every activated combat action — abilities with `magic_cost > 0` behave
//! like spells, abilities with `magic_cost == 0` behave like non-spell
//! activations (e.g. weapon techniques, reload, dodge). A skill tree can
//! point at either kind via the same `UnlockAbility { ability_id }` effect.
//!
//! ## Custom per-skill behavior
//!
//! Two extension points cover the cases the static effect variants can't:
//!
//! - **One-shot, on-learn**: `SkillEffect::Trigger { trigger_id }` emits a
//!   [`SkillTriggerEvent`] when the node is learned. Any system can subscribe
//!   and react with arbitrary logic (set a story flag, unlock a dialogue,
//!   reveal a map area, run a cutscene). The `trigger_id` is a user-defined
//!   namespace shared between the RON file and the listener.
//! - **Ongoing passive**: have the domain system query
//!   `LearnedSkills.has(skill_id)` and modify its own behavior accordingly
//!   (e.g. a damage system reads the component to add bonus damage when a
//!   specific skill is present). No new effect type is needed for these.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat_ability::MagicSchool;
use crate::combat_plugin::{Abilities, CombatStats, GrowthTarget, LevelUpEvent};

/// A single permanent effect applied when a skill node is learned. Effects are
/// flat additions (no level curve), so a node that grants `+5 Lethality` adds
/// exactly 5 to base Lethality at learn time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SkillEffect {
    /// Permanent bump to a base stat. Reuses [`GrowthTarget`] so the node can
    /// hit any base stat the level-up system can hit (Health, Mind, Kiho, …).
    StatBonus { target: GrowthTarget, amount: i32 },
    /// Bump the per-rest-hour regen for one magic school.
    MagicRegenBonus { school: MagicSchool, amount: f32 },
    /// Append `ability_id` to the character's [`Abilities`]. No-op if already known.
    UnlockAbility { ability_id: u16 },
    /// Multiplicative reduction (0.10 = 10% off) to the magic cost of abilities
    /// of `school`. Stored on the character as a [`MagicCostMultipliers`]
    /// factor; combat code reads it at cast time.
    MagicCostReduction { school: MagicSchool, percent: f32 },
    /// Fire a [`SkillTriggerEvent`] when this node is learned. Listeners
    /// dispatch on `trigger_id` to run arbitrary, skill-specific logic that
    /// doesn't fit a stat/ability mold (story flags, scripted reveals,
    /// one-shot world hooks). Fires exactly once, at learn time. For ongoing
    /// passives, query [`LearnedSkills`] from the domain system instead.
    Trigger { trigger_id: u16 },
}

/// One node in a skill tree. Authored in RON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillNode {
    pub id: u16,
    /// Which tree this node belongs to. Magic trees use the matching
    /// `MagicSchool`-named variant; non-magic trees use one of the other
    /// variants of [`SkillTreeKind`].
    pub tree: SkillTreeKind,
    pub tier: u8,
    pub name: String,
    pub description: String,
    /// Skill points this node costs to learn.
    pub cost: u32,
    /// Skill ids that must be learned first. Empty for tier-1 entry nodes.
    pub prerequisites: Vec<u16>,
    pub effects: Vec<SkillEffect>,
}

/// Discriminator for which tree a node belongs to. Used as the [`SkillTreeData`]
/// key. The four `Magic*` variants pair with the [`MagicSchool`] of the same
/// name; the other variants are the non-magic trees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkillTreeKind {
    // Magic-source trees (one per spell source).
    Kiho,
    Onmyodo,
    Yokaijutsu,
    Kamishin,
    // Universal non-magic trees.
    Martial,
    Survival,
    Bound,
    // Per-character class trees.
    RinaRogue,
    SayakaCleric,
    HoujouSamurai,
    ToshikoVessel,
    RenjiroMonk,
    MidoriWildkeeper,
    KanzoExorcist,
}

impl SkillTreeKind {
    /// If this tree corresponds to a magic source, return that school.
    pub fn as_magic_school(self) -> Option<MagicSchool> {
        match self {
            SkillTreeKind::Kiho => Some(MagicSchool::Kiho),
            SkillTreeKind::Onmyodo => Some(MagicSchool::Onmyodo),
            SkillTreeKind::Yokaijutsu => Some(MagicSchool::Yokaijutsu),
            SkillTreeKind::Kamishin => Some(MagicSchool::Kamishin),
            _ => None,
        }
    }

    pub fn from_magic_school(school: MagicSchool) -> Self {
        match school {
            MagicSchool::Kiho => SkillTreeKind::Kiho,
            MagicSchool::Onmyodo => SkillTreeKind::Onmyodo,
            MagicSchool::Yokaijutsu => SkillTreeKind::Yokaijutsu,
            MagicSchool::Kamishin => SkillTreeKind::Kamishin,
        }
    }
}

/// All trees, indexed in two ways: by kind (for browsing) and by id
/// (for quick learn-event handling).
#[derive(Resource, Default, Debug)]
pub struct SkillTreeData {
    /// One vec of nodes per tree kind.
    pub trees: HashMap<SkillTreeKind, Vec<SkillNode>>,
    /// Flat lookup. Built from `trees` at load.
    pub by_id: HashMap<u16, SkillNode>,
}

impl SkillTreeData {
    pub fn get(&self, id: u16) -> Option<&SkillNode> {
        self.by_id.get(&id)
    }

    pub fn tree_for(&self, kind: SkillTreeKind) -> &[SkillNode] {
        self.trees.get(&kind).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn tree_for_magic(&self, school: MagicSchool) -> &[SkillNode] {
        self.tree_for(SkillTreeKind::from_magic_school(school))
    }

    fn rebuild_index(&mut self) {
        self.by_id.clear();
        for nodes in self.trees.values() {
            for node in nodes {
                self.by_id.insert(node.id, node.clone());
            }
        }
    }
}

/// Per-character skill point pool. Mirrors [`AttributePointPool`] but for
/// skill nodes rather than growth attributes.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct SkillPoints {
    pub available: u32,
    pub spent: u32,
}

/// Skill ids the character has already learned. Order is learn order.
#[derive(Component, Debug, Default, Clone)]
pub struct LearnedSkills(pub Vec<u16>);

impl LearnedSkills {
    pub fn has(&self, id: u16) -> bool {
        self.0.iter().any(|s| *s == id)
    }
}

/// Per-character allowlist: which trees this entity is permitted to learn
/// from. Enforced by [`learn_skill_system`] — a `LearnSkillEvent` for a tree
/// not in this list is rejected with an info log. Magic trees are typically
/// gated by which sources the character has affinity for; class trees are
/// gated to the matching protagonist; universal trees (Martial, Survival,
/// Bound) are open to anyone the player wants to grant them to.
///
/// If the component is missing from an entity, every learn attempt is
/// rejected — the access list must be set explicitly at spawn time.
#[derive(Component, Debug, Default, Clone)]
pub struct SkillTreeAccess {
    pub allowed: Vec<SkillTreeKind>,
}

impl SkillTreeAccess {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a single tree to the allowlist (no-op if already present).
    pub fn with(mut self, kind: SkillTreeKind) -> Self {
        if !self.allowed.contains(&kind) {
            self.allowed.push(kind);
        }
        self
    }

    pub fn with_many(mut self, kinds: impl IntoIterator<Item = SkillTreeKind>) -> Self {
        for k in kinds {
            self = self.with(k);
        }
        self
    }

    /// Convenience: take a list of magic schools and add the matching
    /// `SkillTreeKind::Kiho`/`Onmyodo`/etc. variants.
    pub fn with_magic(mut self, schools: impl IntoIterator<Item = MagicSchool>) -> Self {
        for s in schools {
            self = self.with(SkillTreeKind::from_magic_school(s));
        }
        self
    }

    /// Convenience: the three universal non-magic trees.
    pub fn with_universal(self) -> Self {
        self.with_many([
            SkillTreeKind::Martial,
            SkillTreeKind::Survival,
            SkillTreeKind::Bound,
        ])
    }

    pub fn allows(&self, kind: SkillTreeKind) -> bool {
        self.allowed.contains(&kind)
    }
}

/// Per-character magic-cost multipliers, one per school. 1.0 = no discount,
/// 0.85 = 15% off. [`SkillEffect::MagicCostReduction`] composes by multiplying
/// the existing factor by `(1.0 - percent)`.
#[derive(Component, Debug, Clone, Copy)]
pub struct MagicCostMultipliers {
    pub kiho: f32,
    pub onmyodo: f32,
    pub yokaijutsu: f32,
    pub kamishin: f32,
}

impl Default for MagicCostMultipliers {
    fn default() -> Self {
        Self { kiho: 1.0, onmyodo: 1.0, yokaijutsu: 1.0, kamishin: 1.0 }
    }
}

impl MagicCostMultipliers {
    pub fn for_school(&self, school: MagicSchool) -> f32 {
        match school {
            MagicSchool::Kiho => self.kiho,
            MagicSchool::Onmyodo => self.onmyodo,
            MagicSchool::Yokaijutsu => self.yokaijutsu,
            MagicSchool::Kamishin => self.kamishin,
        }
    }

    fn reduce(&mut self, school: MagicSchool, percent: f32) {
        let factor = (1.0 - percent.clamp(0.0, 1.0)).max(0.0);
        let slot = match school {
            MagicSchool::Kiho => &mut self.kiho,
            MagicSchool::Onmyodo => &mut self.onmyodo,
            MagicSchool::Yokaijutsu => &mut self.yokaijutsu,
            MagicSchool::Kamishin => &mut self.kamishin,
        };
        *slot *= factor;
    }
}

/// Fire to attempt to learn `skill_id` for `who`. The handler validates
/// prerequisites and SP, then applies the node's effects exactly once.
#[derive(Debug, Clone, Message)]
pub struct LearnSkillEvent {
    pub who: Entity,
    pub skill_id: u16,
}

/// Emitted when a [`SkillNode`] containing a [`SkillEffect::Trigger`] is
/// learned. Listeners dispatch on `trigger_id` (a user-defined namespace
/// shared between authors of the RON file and the listener system) to run
/// arbitrary skill-specific logic. `skill_id` is included so a listener
/// keyed on `trigger_id` can still distinguish which skill produced the
/// event when multiple skills share a trigger.
#[derive(Debug, Clone, Message)]
pub struct SkillTriggerEvent {
    pub who: Entity,
    pub skill_id: u16,
    pub trigger_id: u16,
}

/// SP awarded per character level gained.
const SKILL_POINTS_PER_LEVEL: u32 = 1;

const SKILL_RON_FILES: &[(SkillTreeKind, &str)] = &[
    // Magic-source trees.
    (SkillTreeKind::Kiho, "assets/data/skills/kiho.ron"),
    (SkillTreeKind::Onmyodo, "assets/data/skills/onmyodo.ron"),
    (SkillTreeKind::Yokaijutsu, "assets/data/skills/yokaijutsu.ron"),
    (SkillTreeKind::Kamishin, "assets/data/skills/kamishin.ron"),
    // Universal non-magic trees.
    (SkillTreeKind::Martial, "assets/data/skills/martial.ron"),
    (SkillTreeKind::Survival, "assets/data/skills/survival.ron"),
    (SkillTreeKind::Bound, "assets/data/skills/bound.ron"),
    // Per-character class trees.
    (SkillTreeKind::RinaRogue, "assets/data/skills/rina_rogue.ron"),
    (SkillTreeKind::SayakaCleric, "assets/data/skills/sayaka_cleric.ron"),
    (SkillTreeKind::HoujouSamurai, "assets/data/skills/houjou_samurai.ron"),
    (SkillTreeKind::ToshikoVessel, "assets/data/skills/toshiko_vessel.ron"),
    (SkillTreeKind::RenjiroMonk, "assets/data/skills/renjiro_monk.ron"),
    (SkillTreeKind::MidoriWildkeeper, "assets/data/skills/midori_wildkeeper.ron"),
    (SkillTreeKind::KanzoExorcist, "assets/data/skills/kanzo_exorcist.ron"),
];

fn load_skill_trees_system(mut tree: ResMut<SkillTreeData>) {
    tree.trees.clear();
    for (kind, path) in SKILL_RON_FILES {
        match std::fs::read_to_string(path) {
            Ok(contents) => match ron::de::from_str::<Vec<SkillNode>>(&contents) {
                Ok(nodes) => {
                    tree.trees.insert(*kind, nodes);
                }
                Err(err) => warn!("Failed to parse skill tree {path}: {err}"),
            },
            Err(err) => warn!("Unable to read skill tree {path}: {err}"),
        }
    }
    tree.rebuild_index();
    info!(
        "Loaded {} skill trees ({} nodes total).",
        tree.trees.len(),
        tree.by_id.len()
    );
}

fn award_skill_points_on_levelup_system(
    mut events: MessageReader<LevelUpEvent>,
    mut q: Query<&mut SkillPoints>,
) {
    for ev in events.read() {
        if ev.new_level <= ev.old_level {
            continue;
        }
        let gained = (ev.new_level as u32 - ev.old_level as u32) * SKILL_POINTS_PER_LEVEL;
        if let Ok(mut sp) = q.get_mut(ev.who) {
            sp.available = sp.available.saturating_add(gained);
        }
    }
}

fn learn_skill_system(
    mut events: MessageReader<LearnSkillEvent>,
    tree: Res<SkillTreeData>,
    mut triggers: MessageWriter<SkillTriggerEvent>,
    mut q: Query<(
        &mut SkillPoints,
        &mut LearnedSkills,
        &mut CombatStats,
        &mut Abilities,
        &mut MagicCostMultipliers,
        &SkillTreeAccess,
    )>,
) {
    for ev in events.read() {
        let Some(node) = tree.get(ev.skill_id).cloned() else {
            warn!("LearnSkillEvent: unknown skill id {}", ev.skill_id);
            continue;
        };
        let Ok((mut sp, mut learned, mut stats, mut abilities, mut mults, access)) =
            q.get_mut(ev.who)
        else {
            // Either entity doesn't exist or it's missing one of the
            // skill-system components (including SkillTreeAccess). The
            // access component is required — silently rejecting matches
            // the GDD intent that magic-source / class trees be gated.
            continue;
        };
        if !access.allows(node.tree) {
            info!(
                "Cannot learn `{}`: tree {:?} not accessible to {:?}.",
                node.name, node.tree, ev.who
            );
            continue;
        }
        if learned.has(node.id) {
            continue;
        }
        if !node.prerequisites.iter().all(|p| learned.has(*p)) {
            info!(
                "Cannot learn `{}`: prerequisites not met for {:?}.",
                node.name, ev.who
            );
            continue;
        }
        if sp.available < node.cost {
            info!(
                "Cannot learn `{}`: needs {} SP, have {}.",
                node.name, node.cost, sp.available
            );
            continue;
        }

        sp.available -= node.cost;
        sp.spent = sp.spent.saturating_add(node.cost);
        learned.0.push(node.id);

        for effect in &node.effects {
            apply_skill_effect(
                effect,
                ev.who,
                node.id,
                &mut stats,
                &mut abilities,
                &mut mults,
                &mut triggers,
            );
        }

        info!(
            "{:?} learned `{}` ({:?} tier {}).",
            ev.who, node.name, node.tree, node.tier
        );
    }
}

fn apply_skill_effect(
    effect: &SkillEffect,
    who: Entity,
    skill_id: u16,
    stats: &mut CombatStats,
    abilities: &mut Abilities,
    mults: &mut MagicCostMultipliers,
    triggers: &mut MessageWriter<SkillTriggerEvent>,
) {
    match effect {
        SkillEffect::StatBonus { target, amount } => {
            apply_stat_bonus(stats, *target, *amount);
        }
        SkillEffect::MagicRegenBonus { school, amount } => match school {
            MagicSchool::Kiho => stats.kiho_per_rest_hour += amount,
            MagicSchool::Onmyodo => stats.onmyodo_per_rest_hour += amount,
            MagicSchool::Yokaijutsu => stats.yokaijutsu_per_rest_hour += amount,
            MagicSchool::Kamishin => stats.kamishin_per_rest_hour += amount,
        },
        SkillEffect::UnlockAbility { ability_id } => {
            if !abilities.0.contains(ability_id) {
                abilities.0.push(*ability_id);
            }
        }
        SkillEffect::MagicCostReduction { school, percent } => {
            mults.reduce(*school, *percent);
        }
        SkillEffect::Trigger { trigger_id } => {
            triggers.write(SkillTriggerEvent {
                who,
                skill_id,
                trigger_id: *trigger_id,
            });
        }
    }
}

/// Mirror of `combat_plugin::apply_growth` but for flat skill-tree bonuses.
/// Kept local so the combat module doesn't need to expose its internal helper.
fn apply_stat_bonus(stats: &mut CombatStats, target: GrowthTarget, amount: i32) {
    let amount_f = amount as f32;
    match target {
        GrowthTarget::Health => stats.health.add_to_base(amount),
        GrowthTarget::HealthRegen => {
            stats.health_per_rest_hour = stats.health_per_rest_hour.saturating_add(amount);
        }
        GrowthTarget::Morale => stats.morale.add_to_base(amount),
        GrowthTarget::MoraleRegen => {
            stats.morale_per_rest_hour = stats.morale_per_rest_hour.saturating_add(amount);
        }
        GrowthTarget::Lethality => stats.lethality.add_to_base(amount),
        GrowthTarget::Hit => stats.hit.add_to_base(amount),
        GrowthTarget::Armor => stats.armor.add_to_base(amount),
        GrowthTarget::Speed => stats.speed.add_to_base(amount),
        GrowthTarget::Evasion => stats.evasion.add_to_base(amount),
        GrowthTarget::Mind => stats.mind.add_to_base(amount),
        GrowthTarget::Movement => stats.movement.add_to_base(amount),
        GrowthTarget::Kiho => stats.kiho.add_to_base(amount_f),
        GrowthTarget::Onmyodo => stats.onmyodo.add_to_base(amount_f),
        GrowthTarget::Yokaijutsu => stats.yokaijutsu.add_to_base(amount_f),
        GrowthTarget::Kamishin => stats.kamishin.add_to_base(amount_f),
    }
}

pub struct SkillTreePlugin;

impl Plugin for SkillTreePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkillTreeData>()
            .add_message::<LearnSkillEvent>()
            .add_message::<SkillTriggerEvent>()
            .add_systems(Startup, load_skill_trees_system)
            .add_systems(Update, award_skill_points_on_levelup_system)
            .add_systems(Update, learn_skill_system);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse every shipped skill tree to catch RON syntax errors before
    /// they fall over silently at runtime (the loader logs a warning and
    /// keeps going, which makes a typo too easy to miss).
    #[test]
    fn all_shipped_trees_parse() {
        let mut total = 0;
        for (kind, path) in SKILL_RON_FILES {
            let contents = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("missing skill tree file {path}: {e}"));
            let nodes: Vec<SkillNode> = ron::de::from_str(&contents)
                .unwrap_or_else(|e| panic!("failed to parse {path}: {e}"));
            assert!(
                !nodes.is_empty(),
                "{path} parsed but has no nodes; did the file get truncated?"
            );
            for node in &nodes {
                assert_eq!(
                    node.tree, *kind,
                    "{path}: node {} declares tree {:?} but the file is for {:?}",
                    node.id, node.tree, kind
                );
                for prereq in &node.prerequisites {
                    assert!(
                        nodes.iter().any(|n| n.id == *prereq),
                        "{path}: node {} requires {} which isn't in the same tree",
                        node.id,
                        prereq
                    );
                }
            }
            total += nodes.len();
        }
        assert!(total >= 11 * 6, "expected ~6 nodes per tree, got {total}");
    }

    #[test]
    fn access_allows_only_listed_trees() {
        let access = SkillTreeAccess::new()
            .with_universal()
            .with_magic([MagicSchool::Kiho]);

        assert!(access.allows(SkillTreeKind::Kiho));
        assert!(access.allows(SkillTreeKind::Martial));
        assert!(access.allows(SkillTreeKind::Survival));
        assert!(access.allows(SkillTreeKind::Bound));

        // Other magic sources rejected unless explicitly added.
        assert!(!access.allows(SkillTreeKind::Yokaijutsu));
        assert!(!access.allows(SkillTreeKind::Kamishin));
        assert!(!access.allows(SkillTreeKind::Onmyodo));

        // Class trees require the matching protagonist.
        assert!(!access.allows(SkillTreeKind::RinaRogue));
        assert!(!access.allows(SkillTreeKind::ToshikoVessel));
    }

    /// Every `UnlockAbility { ability_id }` in the shipped skill files must
    /// point at an Ability id that actually exists in
    /// `AbilitiesExample.ron`. Otherwise learning the skill silently records
    /// an unusable id on the character.
    #[test]
    fn unlocked_ability_ids_exist_in_abilities_file() {
        use crate::combat_ability::Ability;
        let ability_text = std::fs::read_to_string("assets/data/abilities/AbilitiesExample.ron")
            .expect("missing AbilitiesExample.ron");
        let abilities: Vec<Ability> = ron::de::from_str(&ability_text)
            .expect("failed to parse AbilitiesExample.ron");
        let known_ability_ids: std::collections::HashSet<u16> =
            abilities.iter().map(|a| a.id).collect();

        for (_, path) in SKILL_RON_FILES {
            let contents = std::fs::read_to_string(path).unwrap();
            let nodes: Vec<SkillNode> = ron::de::from_str(&contents).unwrap();
            for node in &nodes {
                for effect in &node.effects {
                    if let SkillEffect::UnlockAbility { ability_id } = effect {
                        assert!(
                            known_ability_ids.contains(ability_id),
                            "{path}: skill {} ({}) unlocks ability id {} which is not in AbilitiesExample.ron",
                            node.id,
                            node.name,
                            ability_id
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn protagonist_class_trees_are_exclusive() {
        // Each class tree should only be granted by adding it explicitly —
        // `with_universal` and `with_magic` must never include any class tree.
        let baseline = SkillTreeAccess::new().with_universal().with_magic([
            MagicSchool::Kiho,
            MagicSchool::Onmyodo,
            MagicSchool::Yokaijutsu,
            MagicSchool::Kamishin,
        ]);
        for class_tree in [
            SkillTreeKind::RinaRogue,
            SkillTreeKind::SayakaCleric,
            SkillTreeKind::HoujouSamurai,
            SkillTreeKind::ToshikoVessel,
        ] {
            assert!(
                !baseline.allows(class_tree),
                "baseline (universal + all magic) leaked class tree {:?}",
                class_tree
            );
        }
    }
}
