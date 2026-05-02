// use bevy::prelude::*;
// use rand::Rng;
// use std::collections::VecDeque;

// /// -----------------------------
// /// Components & Types
// /// -----------------------------

// #[derive(Component, Debug)]
// pub struct CharacterId(pub u32);

// #[derive(Component, Debug)]
// pub struct Name(pub String);

// #[derive(Component, Debug)]
// pub struct Class(pub String);

// #[derive(Debug, Clone, Copy)]
// pub enum Stat {
//     Health,
//     HealthRegen,
//     Magic,
//     MagicRegen,
//     Stamina,
//     StaminaRegen,
//     Lethality,
//     Hit,
//     Agility,
//     Mind,
//     Morale,
// }

// #[derive(Debug, Clone, Copy)]
// pub enum DamageType {
//     Physical,
//     Fire,
//     Ice,
//     True,
// }

// /// Current / max / regen components
// #[derive(Component, Debug)]
// pub struct Health {
//     pub current: i32,
//     pub max: i32,
//     pub regen: i32,
// }

// #[derive(Component, Debug)]
// pub struct Magic {
//     pub current: i32,
//     pub max: i32,
//     pub regen: i32,
// }

// #[derive(Component, Debug)]
// pub struct Stamina {
//     pub current: i32,
//     pub max: i32,
//     pub regen: i32,
// }

// /// Base stats that describe the character (unchanged by temporary modifiers)
// #[derive(Component, Debug)]
// pub struct CombatStats {
//     pub base_lethality: i32,
//     pub base_hit: i32,
//     pub base_armor: i32,
//     pub base_agility: i32,
//     pub base_mind: i32,
//     pub base_morale: i32,
//     pub movement: i32,
// }

// /// Abilities placeholder (extend later)
// #[derive(Component, Debug, Default)]
// pub struct Abilities(pub Vec<u16>);

// /// Equipment slots hold Entities referencing equipment items
// #[derive(Component, Debug, Default)]
// pub struct EquipmentSlots {
//     pub weapon: Option<Entity>,
//     pub armor: Option<Entity>,
//     pub accessories: Vec<Entity>,
// }

// /// Tag components for class-specific logic (optional; systems can query these)
// #[derive(Component, Debug)]
// pub struct PaladinBehavior;

// #[derive(Component, Debug)]
// pub struct RogueBehavior;

// /// Equipment entity
// #[derive(Component, Debug)]
// pub struct Equipment {
//     pub id: u32,
//     pub name: String,
//     pub lethality: i32,
//     pub hit: i32,
//     pub armor: i32,
//     pub agility: i32,
//     pub mind: i32,
//     pub morale: i32,
// }

// /// A single equipment-provided effect that can react to events.
// /// This is data only — systems will read it and modify stats or emit events.
// #[derive(Clone, Debug)]
// pub enum EquipHook {
//     /// On BeforeAttack: multiply lethality by multiplier for this attack only
//     BeforeAttackMultiplier { stat: Stat, multiplier: f32, duration_turns: u32 },
//     /// On BeforeHit: add flat damage
//     BeforeHitFlatDamage { flat: i32 },
//     // Add additional hook types as you need
// }

// /// Attach hooks to Equipment via a separate component so equipment is still simple
// #[derive(Component, Debug)]
// pub struct EquipmentHooks(pub Vec<EquipHook>);

// /// Buff entity (applied to a character). Modeled as separate entity so it can store lifetime and effects.
// #[derive(Component, Debug)]
// pub struct Buff {
//     pub stat: Stat,
//     pub multiplier: f32,
//     pub remaining_turns: u32,
//     pub source: Option<Entity>, // which equipment/ability created it (optional)
// }

// /// Temporary stat modifiers applied to a character for a limited duration (e.g., one attack)
// #[derive(Component, Debug)]
// pub struct StatModifiers(pub Vec<StatModifier>);

// #[derive(Clone, Debug)]
// pub struct StatModifier {
//     pub stat: Stat,
//     pub multiplier: f32, // multiplicative (e.g., 1.2 => +20%)
//     pub expires_in_turns: Option<u32>, // None => permanent until explicitly removed
//     pub source: Option<Entity>,
// }

// /// Simple experience / level component (placeholder)
// #[derive(Component, Debug)]
// pub struct Experience(pub u32);

// /// AI parameters (kept as component)
// #[derive(Component, Debug)]
// pub struct AIParameters {
//     pub aggressiveness: u8,
//     pub caution: u8,
//     pub curiosity: u8,
//     pub perception: u8,
//     pub bravery: u8,
//     pub patience: u8,
// }

// impl Default for AIParameters {
//     fn default() -> Self {
//         Self {
//             aggressiveness: 5,
//             caution: 5,
//             curiosity: 5,
//             perception: 5,
//             bravery: 5,
//             patience: 5,
//         }
//     }
// }

// /// -----------------------------
// /// Events (FULL EVENTS model)
// /// -----------------------------

// #[derive(Debug, Clone)]
// pub struct AttackIntentEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub ability_id: Option<u16>, // optional: which ability triggered the attack
// }

// #[derive(Debug, Clone)]
// pub struct BeforeAttackEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub context: AttackContext,
// }

// #[derive(Debug, Clone)]
// pub struct AttackExecuteEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub context: AttackContext,
// }

// #[derive(Debug, Clone)]
// pub struct BeforeHitEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub context: AttackContext,
// }

// #[derive(Debug, Clone)]
// pub struct DamageEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub amount: i32,
//     pub damage_type: DamageType,
// }

// #[derive(Debug, Clone)]
// pub struct AfterHitEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub amount: i32,
//     pub damage_type: DamageType,
// }

// #[derive(Debug, Clone)]
// pub struct AfterAttackEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub context: AttackContext,
// }

// #[derive(Component, Debug)]
// pub struct CharacterId(pub u32);

// #[derive(Component, Debug)]
// pub struct Name(pub String);

// #[derive(Component, Debug)]
// pub struct Class(pub String);

// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
// pub enum Stat {
//     Health,
//     HealthRegen,
//     Magic,
//     MagicRegen,
//     Stamina,
//     StaminaRegen,
//     Lethality,
//     Hit,
//     Agility,
//     Mind,
//     Morale,
// }

// #[derive(Debug, Clone, Copy)]
// pub enum DamageType {
//     Physical,
//     Fire,
//     Ice,
//     True,
// }

// /// Current / max / regen components
// #[derive(Component, Debug)]
// pub struct Health {
//     pub current: i32,
//     pub max: i32,
//     pub regen: i32,
// }

// #[derive(Component, Debug)]
// pub struct Magic {
//     pub current: i32,
//     pub max: i32,
//     pub regen: i32,
// }

// #[derive(Component, Debug)]
// pub struct Stamina {
//     pub current: i32,
//     pub max: i32,
//     pub regen: i32,
// }

// /// Base stats that describe the character (unchanged by temporary modifiers)
// #[derive(Component, Debug)]
// pub struct CombatStats {
//     pub base_lethality: i32,
//     pub base_hit: i32,
//     pub base_armor: i32,
//     pub base_agility: i32,
//     pub base_mind: i32,
//     pub base_morale: i32,
//     pub movement: i32,
// }

// /// XP / Level
// #[derive(Component, Debug)]
// pub struct Experience(pub u32); // raw XP; level is experience >> 16 as original

// #[derive(Component, Debug)]
// pub struct Level(pub u8);

// /// Accumulated agility for turn system (like your original accumulated_agility)
// #[derive(Component, Debug)]
// pub struct AccumulatedAgility(pub u32);

// /// Abilities placeholder (extend later)
// #[derive(Component, Debug, Default)]
// pub struct Abilities(pub Vec<u16>);

// /// Equipment slots hold Entities referencing equipment items
// #[derive(Component, Debug, Default)]
// pub struct EquipmentSlots {
//     pub weapon: Option<Entity>,
//     pub armor: Option<Entity>,
//     pub accessories: Vec<Entity>,
// }

// /// Tag components for class-specific logic (optional; systems can query these)
// #[derive(Component, Debug)]
// pub struct PaladinBehavior;

// #[derive(Component, Debug)]
// pub struct RogueBehavior;

// /// Equipment entity
// #[derive(Component, Debug)]
// pub struct Equipment {
//     pub id: u32,
//     pub name: String,
//     pub lethality: i32,
//     pub hit: i32,
//     pub armor: i32,
//     pub agility: i32,
//     pub mind: i32,
//     pub morale: i32,
// }

// /// Equipment Hooks & Buffs (same as before)
// #[derive(Clone, Debug)]
// pub enum EquipHook {
//     BeforeAttackMultiplier { stat: Stat, multiplier: f32, duration_turns: u32 },
//     BeforeHitFlatDamage { flat: i32 },
// }

// #[derive(Component, Debug)]
// pub struct EquipmentHooks(pub Vec<EquipHook>);

// #[derive(Component, Debug)]
// pub struct Buff {
//     pub stat: Stat,
//     pub multiplier: f32,
//     pub remaining_turns: u32,
//     pub source: Option<Entity>,
// }

// #[derive(Component, Debug)]
// pub struct StatModifiers(pub Vec<StatModifier>);

// #[derive(Clone, Debug)]
// pub struct StatModifier {
//     pub stat: Stat,
//     pub multiplier: f32,
//     pub expires_in_turns: Option<u32>,
//     pub source: Option<Entity>,
// }

// /// -----------------------------
// /// Events (extended)
// /// -----------------------------

// #[derive(Debug, Clone)]
// pub struct AttackIntentEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub ability_id: Option<u16>,
// }

// #[derive(Debug, Clone)]
// pub struct BeforeAttackEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub context: AttackContext,
// }

// #[derive(Debug, Clone)]
// pub struct AttackExecuteEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub context: AttackContext,
// }

// #[derive(Debug, Clone)]
// pub struct BeforeHitEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub context: AttackContext,
// }

// #[derive(Debug, Clone)]
// pub struct DamageEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub amount: i32,
//     pub damage_type: DamageType,
// }

// #[derive(Debug, Clone)]
// pub struct AfterHitEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub amount: i32,
//     pub damage_type: DamageType,
// }

// #[derive(Debug, Clone)]
// pub struct AfterAttackEvent {
//     pub attacker: Entity,
//     pub target: Entity,
//     pub context: AttackContext,
// }

// /// XP / leveling events
// #[derive(Debug, Clone)]
// pub struct XPGainEvent {
//     pub receiver: Entity,
//     pub amount: u32,
// }

// #[derive(Debug, Clone)]
// pub struct LevelUpEvent {
//     pub who: Entity,
//     pub old_level: u8,
//     pub new_level: u8,
// }

// /// Turn & timeline events
// #[derive(Debug, Clone)]
// pub struct TurnOrderCalculatedEvent; // signals the TurnOrder resource was updated

// #[derive(Debug, Clone)]
// pub struct TurnStartEvent {
//     pub who: Entity,
// }

// #[derive(Debug, Clone)]
// pub struct TurnEndEvent {
//     pub who: Entity,
// }


// /// Context shared along the attack pipeline; systems may mutate `meta` or read values.
// #[derive(Debug, Clone)]
// pub struct AttackContext {
//     pub base_lethality: i32,
//     pub base_hit: i32,
//     pub extra_flat_damage: i32,
//     pub multipliers: Vec<StatModifier>, // trackers for multiplicative modifiers applied during flow
// }

// impl Default for AttackContext {
//     fn default() -> Self {
//         Self {
//             base_lethality: 0,
//             base_hit: 0,
//             extra_flat_damage: 0,
//             multipliers: Vec::new(),
//         }
//     }
// }

// /// -----------------------------
// /// Systems
// /// -----------------------------

// /// Startup: spawns example characters and equipment
// fn spawn_examples(mut commands: Commands) {
//     // Example equipment: Silversteel Blade (weapon)
//     let sword = commands
//         .spawn((
//             Equipment {
//                 id: 5001,
//                 name: "Silversteel Blade".to_string(),
//                 lethality: 10,
//                 hit: 5,
//                 armor: 0,
//                 agility: 2,
//                 mind: 0,
//                 morale: 0,
//             },
//             EquipmentHooks(vec![EquipHook::BeforeAttackMultiplier {
//                 stat: Stat::Lethality,
//                 multiplier: 1.15,
//                 duration_turns: 1,
//             }]),
//         ))
//         .id();

//     // Petrus (Paladin) entity
//     let petrus = commands
//         .spawn((
//             Name("Petrus".to_string()),
//             CharacterId(1),
//             Class("Paladin".to_string()),
//             Health {
//                 current: 180,
//                 max: 180,
//                 regen: 2,
//             },
//             Magic {
//                 current: 60,
//                 max: 60,
//                 regen: 1,
//             },
//             Stamina {
//                 current: 100,
//                 max: 100,
//                 regen: 3,
//             },
//             CombatStats {
//                 base_lethality: 18,
//                 base_hit: 80,
//                 base_armor: 20,
//                 base_agility: 7,
//                 base_mind: 10,
//                 base_morale: 95,
//                 movement: 5,
//             },
//             EquipmentSlots {
//                 weapon: Some(sword),
//                 ..Default::default()
//             },
//             Abilities(vec![]),
//             Experience(0),
//             PaladinBehavior,
//             AIParameters {
//                 aggressiveness: 6,
//                 caution: 4,
//                 curiosity: 4,
//                 perception: 7,
//                 bravery: 9,
//                 patience: 6,
//             },
//             StatModifiers(Vec::new()),
//         ))
//         .id();

//     // Rina (Rogue) entity
//     let rina = commands
//         .spawn((
//             Name("Rina".to_string()),
//             CharacterId(2),
//             Class("Rogue".to_string()),
//             Health {
//                 current: 90,
//                 max: 90,
//                 regen: 1,
//             },
//             Magic {
//                 current: 40,
//                 max: 40,
//                 regen: 1,
//             },
//             Stamina {
//                 current: 80,
//                 max: 80,
//                 regen: 2,
//             },
//             CombatStats {
//                 base_lethality: 14,
//                 base_hit: 90,
//                 base_armor: 10,
//                 base_agility: 14,
//                 base_mind: 9,
//                 base_morale: 85,
//                 movement: 7,
//             },
//             EquipmentSlots::default(),
//             Abilities(vec![]),
//             Experience(0),
//             RogueBehavior,
//             AIParameters {
//                 aggressiveness: 5,
//                 caution: 7,
//                 curiosity: 6,
//                 perception: 9,
//                 bravery: 6,
//                 patience: 4,
//             },
//             StatModifiers(Vec::new()),
//         ))
//         .id();

//     // Optional: spawn a buff entity (e.g., Blessing of Courage) applied to Petrus
//     let blessing = commands
//         .spawn((
//             Buff {
//                 stat: Stat::Hit,
//                 multiplier: 1.10, // +10% hit
//                 remaining_turns: 3,
//                 source: None,
//             },
//             // link it to Petrus by adding a marker component or by storing ApplyTo resource. Simpler approach:
//         ))
//         .id();

//     // For demonstration: attach the buff to petrus by inserting a StatModifier directly
//     commands.entity(petrus).insert(StatModifiers(vec![StatModifier {
//         stat: Stat::Hit,
//         multiplier: 1.10,
//         expires_in_turns: Some(3),
//         source: Some(blessing),
//     }]));

//     info!("Spawned Petrus (Entity {:?}) and Rina (Entity {:?}), sword: {:?}", petrus, rina, sword);
// }

// /// Process AttackIntentEvent -> send BeforeAttackEvent
// fn process_attack_intent(
//     mut intents: EventReader<AttackIntentEvent>,
//     mut befores: EventWriter<BeforeAttackEvent>,
//     stats_q: Query<&CombatStats>,
// ) {
//     for intent in intents.iter() {
//         // initialize context from attacker's base stats (if available)
//         if let Ok(stats) = stats_q.get(intent.attacker) {
//             let ctx = AttackContext {
//                 base_lethality: stats.base_lethality,
//                 base_hit: stats.base_hit,
//                 extra_flat_damage: 0,
//                 multipliers: Vec::new(),
//             };
//             befores.send(BeforeAttackEvent {
//                 attacker: intent.attacker,
//                 target: intent.target,
//                 context: ctx,
//             });
//         } else {
//             // fallback: send default context
//             befores.send(BeforeAttackEvent {
//                 attacker: intent.attacker,
//                 target: intent.target,
//                 context: AttackContext::default(),
//             });
//         }
//     }
// }

// /// Generic equipment system: reacts to BeforeAttackEvent and applies stat modifiers when equipment has matching hooks.
// fn equipment_before_attack_listener(
//     mut befores: EventReader<BeforeAttackEvent>,
//     equipment_q: Query<(Entity, &Equipment, &EquipmentHooks)>,
//     slots_q: Query<&EquipmentSlots>,
//     mut commands: Commands,
// ) {
//     for ev in befores.iter() {
//         // find equipment in attacker's slots
//         if let Ok(slots) = slots_q.get(ev.attacker) {
//             // weapon
//             if let Some(weapon_entity) = slots.weapon {
//                 if let Ok((equip_entity, _equip, hooks)) = equipment_q.get(weapon_entity) {
//                     for hook in &hooks.0 {
//                         match hook {
//                             EquipHook::BeforeAttackMultiplier {
//                                 stat,
//                                 multiplier,
//                                 duration_turns,
//                             } => {
//                                 // add a temporary StatModifier to the attacker
//                                 commands.entity(ev.attacker).insert(if let Some(mut sm) = commands
//                                     .get_entity(ev.attacker)
//                                     .and_then(|e| e.get::<StatModifiers>().cloned())
//                                 {
//                                     // This code path is not ideal since Commands does not give easy get() at runtime.
//                                     // Instead we always push a new StatModifiers component with the modifier appended.
//                                     StatModifiers(vec![StatModifier {
//                                         stat: *stat,
//                                         multiplier: *multiplier,
//                                         expires_in_turns: Some(*duration_turns),
//                                         source: Some(equip_entity),
//                                     }])
//                                 } else {
//                                     StatModifiers(vec![StatModifier {
//                                         stat: *stat,
//                                         multiplier: *multiplier,
//                                         expires_in_turns: Some(*duration_turns),
//                                         source: Some(equip_entity),
//                                     }])
//                                 });
//                             }
//                             _ => {}
//                         }
//                     }
//                 }
//             }
//             // TODO: check armor and accessories similarly
//         }
//     }
// }

// /// After all BeforeAttack listeners ran, we push an AttackExecuteEvent so the pipeline continues
// fn before_to_execute(
//     mut befores: EventReader<BeforeAttackEvent>,
//     mut execs: EventWriter<AttackExecuteEvent>,
// ) {
//     for ev in befores.iter() {
//         execs.send(AttackExecuteEvent {
//             attacker: ev.attacker,
//             target: ev.target,
//             context: ev.context.clone(),
//         });
//     }
// }

// /// BeforeHit listeners: weapons or buffs may add flat damage or additional multipliers
// fn before_hit_listeners(
//     mut executes: EventReader<AttackExecuteEvent>,
//     mut before_hits: EventWriter<BeforeHitEvent>,
// ) {
//     for ev in executes.iter() {
//         // For now, forward to BeforeHitEvent; systems can mutate context by listening here (we pass clone)
//         before_hits.send(BeforeHitEvent {
//             attacker: ev.attacker,
//             target: ev.target,
//             context: ev.context.clone(),
//         });
//     }
// }

// /// Execute the hit: compute damage using CombatStats + StatModifiers + context
// fn execute_hit_system(
//     mut before_hits: EventReader<BeforeHitEvent>,
//     mut damage_writer: EventWriter<DamageEvent>,
//     stats_q: Query<&CombatStats>,
//     modifiers_q: Query<&StatModifiers>,
// ) {
//     for ev in before_hits.iter() {
//         // base lethality from context (usually came from attacker's stats)
//         let mut base_leth = ev.context.base_lethality;
//         let base_hit = ev.context.base_hit;
//         let mut flat = ev.context.extra_flat_damage;

//         // Apply stat modifiers for attacker, multiplicatively
//         if let Ok(mods) = modifiers_q.get(ev.attacker) {
//             for m in &mods.0 {
//                 if m.stat == Stat::Lethality {
//                     base_leth = ((base_leth as f32) * m.multiplier).round() as i32;
//                 }
//                 if m.stat == Stat::Hit {
//                     // not used here, but you can transform to hit chance later
//                 }
//             }
//         }

//         // Combine with attacker's combat stats if needed
//         if let Ok(att_stats) = stats_q.get(ev.attacker) {
//             base_leth += att_stats.base_lethality - ev.context.base_lethality; // ensure differences considered
//         }

//         // A very simple damage formula: final = base_leth + flat
//         let final_damage = (base_leth + flat).max(0);

//         damage_writer.send(DamageEvent {
//             attacker: ev.attacker,
//             target: ev.target,
//             amount: final_damage,
//             damage_type: DamageType::Physical,
//         });
//     }
// }

// /// Apply damage to target's Health and emit AfterHitEvent
// fn apply_damage_system(
//     mut damages: EventReader<DamageEvent>,
//     mut after_hit_writer: EventWriter<AfterHitEvent>,
//     mut health_q: Query<&mut Health>,
// ) {
//     for ev in damages.iter() {
//         if let Ok(mut health) = health_q.get_mut(ev.target) {
//             let before = health.current;
//             health.current = (health.current - ev.amount).max(0);
//             let applied = before - health.current;
//             after_hit_writer.send(AfterHitEvent {
//                 attacker: ev.attacker,
//                 target: ev.target,
//                 amount: applied,
//                 damage_type: ev.damage_type,
//             });
//         }
//     }
// }

// /// After hit: allow equipment or buffs to react (e.g., lifesteal)
// fn after_hit_listeners(
//     mut after_hits: EventReader<AfterHitEvent>,
//     mut after_attack_writer: EventWriter<AfterAttackEvent>,
// ) {
//     for ev in after_hits.iter() {
//         // Could apply on-hit effects here
//         after_attack_writer.send(AfterAttackEvent {
//             attacker: ev.attacker,
//             target: ev.target,
//             context: AttackContext::default(),
//         });
//     }
// }

// /// Cleanup after attack (final stage)
// fn after_attack_finalizers(mut after_attacks: EventReader<AfterAttackEvent>) {
//     for ev in after_attacks.iter() {
//         info!("AfterAttack: attacker={:?}, target={:?}", ev.attacker, ev.target);
//         // Trigger visual effects, animations, etc. from here
//     }
// }

// /// Buff tick system: decrease durations and remove expired modifiers/buffs
// fn buff_tick_system(
//     mut commands: Commands,
//     mut query_mods: Query<(Entity, &mut StatModifiers)>,
//     mut query_buffs: Query<(Entity, &mut Buff)>,
// ) {
//     // Decrease stat modifiers' expires_in_turns, remove those with 0
//     for (entity, mut mods) in query_mods.iter_mut() {
//         let mut keep = Vec::new();
//         for mut m in mods.0.drain(..) {
//             if let Some(mut turns) = m.expires_in_turns {
//                 if turns > 0 {
//                     turns -= 1;
//                     if turns > 0 {
//                         m.expires_in_turns = Some(turns);
//                         keep.push(m);
//                     } else {
//                         // expired: drop it
//                     }
//                 } else {
//                     // already expired
//                 }
//             } else {
//                 // permanent: keep
//                 keep.push(m);
//             }
//         }
//         // reinsert kept modifiers
//         commands.entity(entity).insert(StatModifiers(keep));
//     }

//     // Decrease buffs
//     for (entity, mut buff) in query_buffs.iter_mut() {
//         if buff.remaining_turns > 0 {
//             buff.remaining_turns -= 1;
//             if buff.remaining_turns == 0 {
//                 // remove the buff entity
//                 commands.entity(entity).despawn();
//             }
//         }
//     }
// }

// /// Simple regeneration system (health/magic/stamina)
// fn regen_system(mut qh: Query<&mut Health>, qm: Query<&mut Magic>, qs: Query<&mut Stamina>) {
//     for mut h in qh.iter_mut() {
//         h.current = (h.current + h.regen).min(h.max);
//     }
//     for mut m in qm.iter_mut() {
//         m.current = (m.current + m.regen).min(m.max);
//     }
//     for mut s in qs.iter_mut() {
//         s.current = (s.current + s.regen).min(s.max);
//     }
// }

// /// Example AI system that makes a simple attack intent for demo
// fn demo_ai_system(
//     mut intents: EventWriter<AttackIntentEvent>,
//     query_chars: Query<Entity, With<CombatStats>>,
// ) {
//     // Very naive: if there are at least two entities, make one attack the next
//     let ents: Vec<Entity> = query_chars.iter().collect();
//     if ents.len() >= 2 {
//         // pick two distinct entities
//         let attacker = ents[0];
//         let target = ents[1];
//         // send intent (this would normally be based on AIParameters)
//         intents.send(AttackIntentEvent {
//             attacker,
//             target,
//             ability_id: None,
//         });
//     }
// }

// /// Debug print of characters status
// fn debug_print_system(
//     q: Query<(
//         &Name,
//         &CharacterId,
//         &Health,
//         &CombatStats,
//         Option<&StatModifiers>,
//         Option<&EquipmentSlots>,
//     )>,
// ) {
//     for (name, id, health, stats, mods, slots) in q.iter() {
//         let mut s = format!(
//             "{}({:?}) HP: {}/{} Leth:{} Hit:{}",
//             name.0, id.0, health.current, health.max, stats.base_lethality, stats.base_hit
//         );
//         if let Some(mods) = mods {
//             if !mods.0.is_empty() {
//                 s.push_str(&format!(" Mods: {:?}", mods.0));
//             }
//         }
//         if let Some(slots) = slots {
//             if slots.weapon.is_some() {
//                 s.push_str(" WeaponEquipped");
//             }
//         }
//         info!("{}", s);
//     }
// }

// /// -----------------------------
// /// Components & Types (extended)
// /// -----------------------------

// /// Context shared along the attack pipeline; systems may mutate `meta` or read values.
// #[derive(Debug, Clone)]
// pub struct AttackContext {
//     pub base_lethality: i32,
//     pub base_hit: i32,
//     pub extra_flat_damage: i32,
//     pub multipliers: Vec<StatModifier>,
// }

// impl Default for AttackContext {
//     fn default() -> Self {
//         Self {
//             base_lethality: 0,
//             base_hit: 0,
//             extra_flat_damage: 0,
//             multipliers: Vec::new(),
//         }
//     }
// }

// /// -----------------------------
// /// Turn Manager resource
// /// -----------------------------

// /// Holds the current computed turn order (queue of entities ready to act).
// #[derive(Resource, Default)]
// pub struct TurnOrder {
//     pub queue: VecDeque<Entity>,
// }

// /// Resource that knows which entities should participate in turn calc.
// /// For simplicity we store a Vec<Entity> that is maintained at spawn time.
// #[derive(Resource, Default)]
// pub struct TurnManager {
//     pub participants: Vec<Entity>,
//     pub turn_threshold: u32,
//     pub maximum_value: u32, // random jitter max
// }

// impl TurnManager {
//     pub fn recompute_params(&mut self, stats_q: &Query<&CombatStats>, levels_q: &Query<&Level>) {
//         // compute avg agility and avg level across participants that still exist
//         let mut total_agility: u32 = 0;
//         let mut total_level: u32 = 0;
//         let mut count: u32 = 0;
//         for &e in &self.participants {
//             if let Ok(stats) = stats_q.get(e) {
//                 total_agility += stats.base_agility.max(0) as u32;
//                 if let Ok(level) = levels_q.get(e) {
//                     total_level += level.0 as u32;
//                 } else {
//                     total_level += 1; // default level if missing
//                 }
//                 count += 1;
//             }
//         }
//         if count == 0 {
//             self.turn_threshold = 100; // fallback
//             self.maximum_value = 10;
//             return;
//         }
//         let avg_agility = (total_agility / count).max(1);
//         self.turn_threshold = avg_agility * 2; // original used <<1
//         let avg_level = (total_level / count).max(1);
//         self.maximum_value = avg_level << 3; // original used <<3
//     }

//     /// Calculate a precise turn order based on accumulated agility.
//     /// For each participant:
//     ///   accumulated += base_agility + rand(0..maximum_value)
//     ///   while accumulated >= turn_threshold: push to order and subtract threshold
//     pub fn calculate_turn_order(
//         &mut self,
//         mut acc_q: &mut Query<&mut AccumulatedAgility>,
//         stats_q: &Query<&CombatStats>,
//     ) -> Vec<Entity> {
//         let mut rng = rand::thread_rng();
//         let mut order: Vec<Entity> = Vec::new();

//         // iterate participants in stable order
//         for &entity in &self.participants {
//             // get accumulated agility, if missing insert default (0)
//             if let Ok(mut acc) = acc_q.get_mut(entity) {
//                 let agility = stats_q
//                     .get(entity)
//                     .map(|s| s.base_agility.max(0) as u32)
//                     .unwrap_or(0);
//                 let jitter: u32 = if self.maximum_value > 0 {
//                     rng.gen_range(0..self.maximum_value)
//                 } else {
//                     0
//                 };

//                 let mut current = acc.0;
//                 // add base agility + random jitter
//                 current = current.saturating_add(agility).saturating_add(jitter);
//                 // while enough to take a turn
//                 while current >= self.turn_threshold && self.turn_threshold > 0 {
//                     current = current.saturating_sub(self.turn_threshold);
//                     order.push(entity);
//                 }
//                 acc.0 = current;
//             } else {
//                 // entity missing AccumulatedAgility -> skip or insert?
//                 // We skip; spawn-time code should ensure AccumulatedAgility exists for participants.
//             }
//         }
//         order
//     }
// }

// /// -----------------------------
// /// Systems: XP / Leveling
// /// -----------------------------

// /// Calculate XP awarded given enemy_experience and receiver_experience (from original formula).
// /// This mirrors your original approach but with safe floating arithmetic and guards.
// fn calculate_xp_award(receiver_experience: u32, enemy_experience: u32) -> u32 {
//     // original used ratio = enemy_experience / self.experience
//     // guard: if receiver_experience == 0, treat ratio as 1.0 to avoid div-by-zero
//     let ratio: f32 = if receiver_experience == 0 {
//         // if receiver has 0 xp, award something small proportional to enemy XP
//         (enemy_experience as f32) / 1.0f32
//     } else {
//         (enemy_experience as f32) / (receiver_experience as f32)
//     };

//     // thresholds from original: shift left 14 (= *16384)
//     let multiplier_base = 16384.0_f32;

//     // clamp ratio to avoid NaNs
//     let ratio = ratio.max(0.0);

//     let amount_f: f32 = if ratio > 0.946 {
//         // ((ratio - 0.2).ln() / 1.25.ln() + 1.5) << 14  converted to *16384
//         let inner = (ratio - 0.2).max(0.0001);
//         let value = ((inner.ln() / 1.25f32.ln()) + 1.5) * multiplier_base;
//         // clamp to non-negative
//         value.max(0.0)
//     } else {
//         // ratio.powf(30.2) << 14
//         ratio.powf(30.2) * multiplier_base
//     };

//     // avoid huge values; clamp to u32::MAX-1
//     if amount_f.is_nan() || amount_f <= 0.0 {
//         0
//     } else if amount_f >= (u32::MAX as f32) {
//         u32::MAX - 1
//     } else {
//         amount_f.round() as u32
//     }
// }

// /// Handle XPGainEvent: add to Experience component and emit LevelUpEvent if level increases
// fn xp_gain_system(
//     mut xp_events: EventReader<XPGainEvent>,
//     mut xp_writer: EventWriter<LevelUpEvent>,
//     mut qxp: Query<(&mut Experience, &mut Level)>,
// ) {
//     for ev in xp_events.iter() {
//         if let Ok((mut xp, mut lvl)) = qxp.get_mut(ev.receiver) {
//             let old_level = lvl.0;
//             // add XP
//             xp.0 = xp.0.saturating_add(ev.amount);
//             // recompute level as experience >> 16 (original approach)
//             let new_level = (xp.0 >> 16) as u8;
//             if new_level > old_level {
//                 lvl.0 = new_level;
//                 xp_writer.send(LevelUpEvent {
//                     who: ev.receiver,
//                     old_level,
//                     new_level,
//                 });
//             }
//         }
//     }
// }

// /// Level up handler: apply stat increases using functions derived from original file.
// /// The original used strange formulas; here we approximate the same behavior while keeping types safe.
// /// Each function will increase appropriate components.
// fn level_up_system(
//     mut lvl_events: EventReader<LevelUpEvent>,
//     mut q_stats: Query<(
//         &mut CombatStats,
//         Option<&mut Health>,
//         Option<&mut Stamina>,
//         Option<&mut Magic>,
//     )>,
// ) {
//     for ev in lvl_events.iter() {
//         if let Ok((mut stats, h_opt, s_opt, m_opt)) = q_stats.get_mut(ev.who) {
//             // number of levels gained
//             let level_gained = (ev.new_level as i32) - (ev.old_level as i32);
//             if level_gained <= 0 {
//                 continue;
//             }

//             // For each gained level, apply growth functions.
//             for _ in 0..level_gained {
//                 // health growth
//                 if let Some(mut h) = h_opt {
//                     // This mirrors your "level_health" intent: add diminishing returns
//                     // We'll add a value proportional to 50..150 using a soft cap approach:
//                     let add_hp = (500.0 - (h.max as f32).powf(1.955) / 524288.0).max(1.0);
//                     let add_hp_i = add_hp.round() as i32;
//                     h.max = h.max.saturating_add(add_hp_i);
//                     h.current = h.current.saturating_add(add_hp_i);
//                     // increase health regen slightly
//                     h.regen = (h.regen + (50i32 - ((h.regen as f32).powf(2.21) / 524288.0).round() as i32)).max(0);
//                 }

//                 // stamina growth
//                 if let Some(mut s) = s_opt {
//                     let add_s = (500.0 - (s.max as f32).powf(1.955) / 524288.0).max(1.0);
//                     let add_s_i = add_s.round() as i32;
//                     s.max = s.max.saturating_add(add_s_i);
//                     s.current = s.current.saturating_add(add_s_i);
//                     s.regen = (s.regen + (50i32 - ((s.regen as f32).powf(2.05) / 524288.0).round() as i32)).max(0);
//                 }

//                 // magic growth
//                 if let Some(mut m) = m_opt {
//                     let add_m = (500.0 - (m.max as f32).powf(1.955) / 524288.0).max(1.0);
//                     let add_m_i = add_m.round() as i32;
//                     m.max = m.max.saturating_add(add_m_i);
//                     m.current = m.current.saturating_add(add_m_i);
//                     m.regen = (m.regen + (50i32 - ((m.regen as f32).powf(2.21) / 524288.0).round() as i32)).max(0);
//                 }

//                 // stats bump: lethality, hit, agility, mind, morale etc.
//                 stats.base_lethality = stats.base_lethality.saturating_add( (500.0 - (stats.base_lethality as f32).powf(1.955) / 524288.0).round() as i32 );
//                 stats.base_hit = stats.base_hit.saturating_add( (500.0 - (stats.base_hit as f32).powf(1.955) / 524288.0).round() as i32 );
//                 stats.base_agility = stats.base_agility.saturating_add( (500.0 - (stats.base_agility as f32).powf(1.955) / 524288.0).round() as i32 );
//                 stats.base_mind = stats.base_mind.saturating_add( (500.0 - (stats.base_mind as f32).powf(1.955) / 524288.0).round() as i32 );
//                 stats.base_morale = stats.base_morale.saturating_add( (500.0 - (stats.base_morale as f32).powf(1.955) / 524288.0).round() as i32 );
//             }

//             info!("Level up applied to {:?}: {} -> {}", ev.who, ev.old_level, ev.new_level);
//         }
//     }
// }

// /// -----------------------------
// /// Systems: Turn manager & Turn order calculation
// /// -----------------------------

// /// A system that ensures TurnManager participants are kept in sync with spawned characters.
// /// Call this whenever you spawn or despawn participants.
// fn register_participants_system(
//     mut tm: ResMut<TurnManager>,
//     query_chars: Query<Entity, With<CombatStats>>,
// ) {
//     // simple strategy: replace participants with all entities that have CombatStats
//     tm.participants = query_chars.iter().collect();
// }

// /// Calculate turn order each "tick" (you may call this on a schedule or when you want a fresh order)
// fn compute_turn_order_system(
//     mut tm: ResMut<TurnManager>,
//     mut turn_order: ResMut<TurnOrder>,
//     mut acc_q: Query<&mut AccumulatedAgility>,
//     stats_q: Query<&CombatStats>,
//     levels_q: Query<&Level>,
//     mut ev_writer: EventWriter<TurnOrderCalculatedEvent>,
// ) {
//     // recompute threshold / max jitter based on participants
//     tm.recompute_params(&stats_q, &levels_q);

//     // Important: make acc_q mutable borrow optional; pass as &mut Query below
//     // But in bevy we cannot pass &mut Query into resource functions; we call method and use acc_q directly
//     // We'll call calculate_turn_order in-place:
//     let mut order_vec: Vec<Entity> = Vec::new();
//     // Create a temporary mutable reference to acc_q by using the Query directly
//     // call tm.calculate_turn_order(mut acc_q, &stats_q)
//     // Unfortunately we cannot pass Query into a method expecting &mut Query, so inline behavior here:

//     let mut rng = rand::thread_rng();
//     for &entity in &tm.participants {
//         if let Ok(mut acc) = acc_q.get_mut(entity) {
//             let agility = stats_q.get(entity).map(|s| s.base_agility.max(0) as u32).unwrap_or(0);
//             let jitter: u32 = if tm.maximum_value > 0 { rng.gen_range(0..tm.maximum_value) } else { 0 };
//             let mut current = acc.0;
//             current = current.saturating_add(agility).saturating_add(jitter);
//             while current >= tm.turn_threshold && tm.turn_threshold > 0 {
//                 current = current.saturating_sub(tm.turn_threshold);
//                 order_vec.push(entity);
//             }
//             acc.0 = current;
//         }
//     }

//     // place order_vec into TurnOrder queue
//     turn_order.queue.clear();
//     for e in order_vec {
//         turn_order.queue.push_back(e);
//     }

//     ev_writer.send(TurnOrderCalculatedEvent);
// }

// /// Splits out the next entity from TurnOrder and emits a TurnStartEvent
// fn advance_turn_system(mut turn_order: ResMut<TurnOrder>, mut ev_writer: EventWriter<TurnStartEvent>) {
//     if let Some(next) = turn_order.queue.pop_front() {
//         ev_writer.send(TurnStartEvent { who: next });
//     }
// }

// /// Example: when a turn starts for an entity, we allow AI or player to emit AttackIntentEvent.
// /// For simplicity demo AI will fire an intent against any other participant.
// fn on_turn_start_system(
//     mut ev_reader: EventReader<TurnStartEvent>,
//     q_participants: Query<Entity, With<CombatStats>>,
//     mut intent_writer: EventWriter<AttackIntentEvent>,
// ) {
//     for ev in ev_reader.iter() {
//         // simple demo: find first entity different from ev.who and issue attack
//         let mut target_opt: Option<Entity> = None;
//         for e in q_participants.iter() {
//             if e != ev.who {
//                 target_opt = Some(e);
//                 break;
//             }
//         }
//         if let Some(target) = target_opt {
//             intent_writer.send(AttackIntentEvent {
//                 attacker: ev.who,
//                 target,
//                 ability_id: None,
//             });
//         }
//     }
// }

// /// At the end of a turn, we emit TurnEndEvent to allow cleanup and buff ticks if you prefer to tie buff durations to turns.
// fn on_turn_end_system(mut ev_reader: EventReader<TurnEndEvent>, mut _commands: Commands) {
//     for ev in ev_reader.iter() {
//         info!("Turn ended for {:?}", ev.who);
//         // You can do per-turn cleanup here if necessary
//     }
// }

// /// A helper system that consumes TurnOrderCalculatedEvent and then advances the turn automatically.
// /// (Optional: you may want to call advance once per frame or per game tick)
// fn auto_advance_after_order(
//     mut ev_reader: EventReader<TurnOrderCalculatedEvent>,
//     mut turn_order: ResMut<TurnOrder>,
//     mut ev_writer: EventWriter<TurnStartEvent>,
// ) {
//     for _ in ev_reader.iter() {
//         if let Some(next) = turn_order.queue.pop_front() {
//             ev_writer.send(TurnStartEvent { who: next });
//         }
//     }
// }

// /// Buff tick per turn: when a TurnStartEvent occurs for a character, decrement their buff durations (so durations map to turns).
// fn buff_tick_on_turn_start_system(
//     mut ev_reader: EventReader<TurnStartEvent>,
//     mut query_buffs: Query<(Entity, &mut Buff)>,
//     mut commands: Commands,
//     mut modifiers_q: Query<(Entity, &mut StatModifiers)>,
// ) {
//     for ev in ev_reader.iter() {
//         // Decrement global Buff entities that have source == ev.who (optional design)
//         for (entity, mut buff) in query_buffs.iter_mut() {
//             if let Some(src) = buff.source {
//                 if src == ev.who {
//                     if buff.remaining_turns > 0 {
//                         buff.remaining_turns -= 1;
//                         if buff.remaining_turns == 0 {
//                             commands.entity(entity).despawn();
//                         }
//                     }
//                 }
//             }
//         }

//         // Also decrement StatModifiers on the actor
//         if let Ok((entity, mut mods)) = modifiers_q.get_mut(ev.who) {
//             let mut keep: Vec<StatModifier> = Vec::new();
//             for mut m in mods.0.drain(..) {
//                 if let Some(turns) = m.expires_in_turns {
//                     if turns > 1 {
//                         m.expires_in_turns = Some(turns - 1);
//                         keep.push(m);
//                     } else {
//                         // expires now -> drop
//                     }
//                 } else {
//                     // permanent -> keep
//                     keep.push(m);
//                 }
//             }
//             // reinsert updated modifiers
//             commands.entity(entity).insert(StatModifiers(keep));
//         }
//     }
// }

// /// -----------------------------
// /// Minimal Combat pipeline (unchanged core) — only key systems are included here,
// /// refer to earlier code for full pipeline. We keep the key entry point systems.
// /// -----------------------------

// fn process_attack_intent(
//     mut intents: EventReader<AttackIntentEvent>,
//     mut befores: EventWriter<BeforeAttackEvent>,
//     stats_q: Query<&CombatStats>,
// ) {
//     for intent in intents.iter() {
//         if let Ok(stats) = stats_q.get(intent.attacker) {
//             let ctx = AttackContext {
//                 base_lethality: stats.base_lethality,
//                 base_hit: stats.base_hit,
//                 extra_flat_damage: 0,
//                 multipliers: Vec::new(),
//             };
//             befores.send(BeforeAttackEvent {
//                 attacker: intent.attacker,
//                 target: intent.target,
//                 context: ctx,
//             });
//         } else {
//             befores.send(BeforeAttackEvent {
//                 attacker: intent.attacker,
//                 target: intent.target,
//                 context: AttackContext::default(),
//             });
//         }
//     }
// }

// fn before_to_execute(
//     mut befores: EventReader<BeforeAttackEvent>,
//     mut execs: EventWriter<AttackExecuteEvent>,
// ) {
//     for ev in befores.iter() {
//         execs.send(AttackExecuteEvent {
//             attacker: ev.attacker,
//             target: ev.target,
//             context: ev.context.clone(),
//         });
//     }
// }

// fn before_hit_listeners(
//     mut executes: EventReader<AttackExecuteEvent>,
//     mut before_hits: EventWriter<BeforeHitEvent>,
// ) {
//     for ev in executes.iter() {
//         before_hits.send(BeforeHitEvent {
//             attacker: ev.attacker,
//             target: ev.target,
//             context: ev.context.clone(),
//         });
//     }
// }

// fn execute_hit_system(
//     mut before_hits: EventReader<BeforeHitEvent>,
//     mut damage_writer: EventWriter<DamageEvent>,
//     stats_q: Query<&CombatStats>,
//     modifiers_q: Query<&StatModifiers>,
// ) {
//     for ev in before_hits.iter() {
//         let mut base_leth = ev.context.base_lethality;
//         let _base_hit = ev.context.base_hit;
//         let mut flat = ev.context.extra_flat_damage;

//         if let Ok(mods) = modifiers_q.get(ev.attacker) {
//             for m in &mods.0 {
//                 if m.stat == Stat::Lethality {
//                     base_leth = ((base_leth as f32) * m.multiplier).round() as i32;
//                 }
//             }
//         }

//         if let Ok(att_stats) = stats_q.get(ev.attacker) {
//             base_leth += att_stats.base_lethality - ev.context.base_lethality;
//         }

//         let final_damage = (base_leth + flat).max(0);
//         damage_writer.send(DamageEvent {
//             attacker: ev.attacker,
//             target: ev.target,
//             amount: final_damage,
//             damage_type: DamageType::Physical,
//         });
//     }
// }

// fn apply_damage_system(
//     mut damages: EventReader<DamageEvent>,
//     mut after_hit_writer: EventWriter<AfterHitEvent>,
//     mut health_q: Query<&mut Health>,
// ) {
//     for ev in damages.iter() {
//         if let Ok(mut health) = health_q.get_mut(ev.target) {
//             let before = health.current;
//             health.current = (health.current - ev.amount).max(0);
//             let applied = before - health.current;
//             after_hit_writer.send(AfterHitEvent {
//                 attacker: ev.attacker,
//                 target: ev.target,
//                 amount: applied,
//                 damage_type: ev.damage_type,
//             });
//         }
//     }
// }

// fn after_hit_listeners(
//     mut after_hits: EventReader<AfterHitEvent>,
//     mut after_attack_writer: EventWriter<AfterAttackEvent>,
// ) {
//     for ev in after_hits.iter() {
//         after_attack_writer.send(AfterAttackEvent {
//             attacker: ev.attacker,
//             target: ev.target,
//             context: AttackContext::default(),
//         });
//     }
// }

// fn after_attack_finalizers(mut after_attacks: EventReader<AfterAttackEvent>) {
//     for ev in after_attacks.iter() {
//         info!("AfterAttack: attacker={:?}, target={:?}", ev.attacker, ev.target);
//     }
// }

// /// -----------------------------
// /// Supporting systems
// /// -----------------------------

// fn regen_system(mut qh: Query<&mut Health>, mut qm: Query<&mut Magic>, mut qs: Query<&mut Stamina>) {
//     for mut h in qh.iter_mut() {
//         h.current = (h.current + h.regen).min(h.max);
//     }
//     for mut m in qm.iter_mut() {
//         m.current = (m.current + m.regen).min(m.max);
//     }
//     for mut s in qs.iter_mut() {
//         s.current = (s.current + s.regen).min(s.max);
//     }
// }

// /// Debug print of characters status
// fn debug_print_system(
//     q: Query<(
//         &Name,
//         &CharacterId,
//         &Health,
//         &CombatStats,
//         Option<&StatModifiers>,
//         Option<&EquipmentSlots>,
//         Option<&Level>,
//         Option<&Experience>,
//         Option<&AccumulatedAgility>,
//     )>,
// ) {
//     for (name, id, health, stats, mods, slots, lvl, xp, acc) in q.iter() {
//         let level = lvl.map(|l| l.0).unwrap_or(1);
//         let xp_val = xp.map(|x| x.0).unwrap_or(0);
//         let acc_text = acc.map(|a| a.0.to_string()).unwrap_or_else(|| "N/A".into());
//         let mut s = format!(
//             "{}({:?}) L{} XP:{} HP: {}/{} Leth:{} Hit:{} Acc:{}",
//             name.0, id.0, level, xp_val, health.current, health.max, stats.base_lethality, stats.base_hit, acc_text
//         );
//         if let Some(mods) = mods {
//             if !mods.0.is_empty() {
//                 s.push_str(&format!(" Mods: {:?}", mods.0));
//             }
//         }
//         if let Some(slots) = slots {
//             if slots.weapon.is_some() {
//                 s.push_str(" WeaponEquipped");
//             }
//         }
//         info!("{}", s);
//     }
// }

// /// -----------------------------
// /// Startup spawn examples (with XP, Level, AccumulatedAgility)
// /// -----------------------------
// fn spawn_examples(mut commands: Commands, mut tm: ResMut<TurnManager>) {
//     // spawn sword
//     let sword = commands
//         .spawn((
//             Equipment {
//                 id: 5001,
//                 name: "Silversteel Blade".to_string(),
//                 lethality: 10,
//                 hit: 5,
//                 armor: 0,
//                 agility: 2,
//                 mind: 0,
//                 morale: 0,
//             },
//             EquipmentHooks(vec![EquipHook::BeforeAttackMultiplier {
//                 stat: Stat::Lethality,
//                 multiplier: 1.15,
//                 duration_turns: 1,
//             }]),
//         ))
//         .id();

//     // Petrus
//     let petrus = commands
//         .spawn((
//             Name("Petrus".to_string()),
//             CharacterId(1),
//             Class("Paladin".to_string()),
//             Health {
//                 current: 180,
//                 max: 180,
//                 regen: 2,
//             },
//             Magic {
//                 current: 60,
//                 max: 60,
//                 regen: 1,
//             },
//             Stamina {
//                 current: 100,
//                 max: 100,
//                 regen: 3,
//             },
//             CombatStats {
//                 base_lethality: 18,
//                 base_hit: 80,
//                 base_armor: 20,
//                 base_agility: 7,
//                 base_mind: 10,
//                 base_morale: 95,
//                 movement: 5,
//             },
//             EquipmentSlots {
//                 weapon: Some(sword),
//                 ..Default::default()
//             },
//             Abilities(vec![]),
//             Experience(0),
//             Level(1),
//             AccumulatedAgility(0),
//             PaladinBehavior,
//             StatModifiers(Vec::new()),
//         ))
//         .id();

//     // Rina
//     let rina = commands
//         .spawn((
//             Name("Rina".to_string()),
//             CharacterId(2),
//             Class("Rogue".to_string()),
//             Health {
//                 current: 90,
//                 max: 90,
//                 regen: 1,
//             },
//             Magic {
//                 current: 40,
//                 max: 40,
//                 regen: 1,
//             },
//             Stamina {
//                 current: 80,
//                 max: 80,
//                 regen: 2,
//             },
//             CombatStats {
//                 base_lethality: 14,
//                 base_hit: 90,
//                 base_armor: 10,
//                 base_agility: 14,
//                 base_mind: 9,
//                 base_morale: 85,
//                 movement: 7,
//             },
//             EquipmentSlots::default(),
//             Abilities(vec![]),
//             Experience(0),
//             Level(1),
//             AccumulatedAgility(0),
//             RogueBehavior,
//             StatModifiers(Vec::new()),
//         ))
//         .id();

//     // register participants in turn manager
//     tm.participants.push(petrus);
//     tm.participants.push(rina);

//     info!("Spawned Petrus {:?} and Rina {:?} (sword {:?})", petrus, rina, sword);
// }

// /// -----------------------------
// /// App Setup
// /// -----------------------------
// pub struct CombatPlugin;

// impl Plugin for CombatPlugin {
//     fn build(&self, app: &mut App) {
//         app.insert_resource(TurnOrder::default())
//             .insert_resource(TurnManager::default())
//             // events
//             .add_event::<AttackIntentEvent>()
//             .add_event::<BeforeAttackEvent>()
//             .add_event::<AttackExecuteEvent>()
//             .add_event::<BeforeHitEvent>()
//             .add_event::<DamageEvent>()
//             .add_event::<AfterHitEvent>()
//             .add_event::<AfterAttackEvent>()
//             .add_event::<XPGainEvent>()
//             .add_event::<LevelUpEvent>()
//             .add_event::<TurnOrderCalculatedEvent>()
//             .add_event::<TurnStartEvent>()
//             .add_event::<TurnEndEvent>()
//             // startup
//             .add_startup_system(spawn_examples)
//             // xp / leveling systems
//             .add_system(xp_gain_system)
//             .add_system(level_up_system.after(xp_gain_system))
//             // turn systems
//             .add_system(register_participants_system)
//             .add_system(compute_turn_order_system.after(register_participants_system))
//             .add_system(auto_advance_after_order.after(compute_turn_order_system))
//             .add_system(on_turn_start_system.after(auto_advance_after_order))
//             .add_system(buff_tick_on_turn_start_system.after(on_turn_start_system))
//             .add_system(advance_turn_system.after(compute_turn_order_system))
//             // combat pipeline (core)
//             .add_system(process_attack_intent)
//             .add_system(before_to_execute.after(process_attack_intent))
//             .add_system(before_hit_listeners.after(before_to_execute))
//             .add_system(execute_hit_system.after(before_hit_listeners))
//             .add_system(apply_damage_system.after(execute_hit_system))
//             .add_system(after_hit_listeners.after(apply_damage_system))
//             .add_system(after_attack_finalizers.after(after_hit_listeners))
//             // supporting
//             .add_system(regen_system)
//             .add_system(debug_print_system);
//     }
// }

// // fn main() {
// //     App::new()
// //         .add_plugins(DefaultPlugins)
// //         .add_plugin(CombatPlugin)
// //         .run();
// // }

