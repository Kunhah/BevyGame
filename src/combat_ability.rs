use std::cmp::Ordering;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use bevy::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::combat_plugin::{
    AttackIntentEvent, ApplyBuffEvent, DamageQueue, DamageTag, DamageType, HealEvent, QueuedDamage,
    Stat,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AbilityEffect {
    Heal { floor: u32, ceiling: u32, scaled_with: Stat },
    Damage {
        floor: u32,
        ceiling: u32,
        damage_type: DamageType,
        scaled_with: Stat,
        defended_with: Stat,
    },
    Buff {
        stat: Stat,
        multiplier: f32,
        effects: Option<Vec<u16>>,
        scaled_with: Stat,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AbilityShape {
    Radius(f32),
    Line { length: f32, thickness: f32 },
    Cone { angle: f32, radius: f32 },
    Select,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ability {
    pub id: u16,
    pub next_id: Option<u16>,
    pub name: String,
    pub health_cost: i32,
    pub magic_cost: i32,
    pub stamina_cost: i32,
    pub cooldown: u8,
    pub description: String,
    pub effects: Vec<AbilityEffect>,
    pub shape: AbilityShape,
    pub duration: u8,
    pub targets: u8,
}

impl Ability {
    pub fn get_level(&self) -> u8 {
        // high byte of the packed id
        ((self.id & 0xFF00) >> 8) as u8
    }
    pub fn get_sub_id(&self) -> u8 {
        // low byte of the packed id
        (self.id & 0x00FF) as u8
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
    mut dq: ResMut<DamageQueue>,
    mut attack_intent_events: MessageWriter<AttackIntentEvent>,
    mut heal_events: MessageWriter<HealEvent>,
    mut buff_events: MessageWriter<ApplyBuffEvent>,
) {
    for &target in affected {
        for effect in &ability.effects {
            match effect {
                AbilityEffect::Heal { floor, ceiling, .. } => {
                    let amount = rand::rng().gen_range(*floor..*ceiling);
                    heal_events.write(HealEvent {
                        healer: caster,
                        target,
                        amount,
                    });
                }
                AbilityEffect::Damage {
                    floor,
                    ceiling,
                    damage_type,
                    scaled_with,
                    defended_with,
                } => {
                    let base = rand::rng().gen_range(*floor..*ceiling) as i32;

                    dq.0.push(QueuedDamage {
                        attacker: caster,
                        target,
                        amount: base,
                        damage_type: *damage_type,
                        scaled_with: vec![(*scaled_with, 1.0)],
                        defended_with: vec![(*defended_with, 1.0)],
                        accuracy_override: None,
                        crit_chance: 0.0,
                        tags: vec![DamageTag::FromAbility(ability.id)],
                    });

                    attack_intent_events.write(AttackIntentEvent {
                        attacker: caster,
                        target,
                        ability: Some(ability.clone()),
                        context: crate::combat_plugin::AttackContext {
                            damage_type: Some(*damage_type),
                            ..Default::default()
                        },
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
                    });
                }
            }
        }
    }
}
