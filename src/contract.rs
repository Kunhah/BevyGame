//! The Merchant's Contract — rule enforcement.
//!
//! Each GDD rule that produces an in-game consequence is one function in this
//! file. Functions are Bevy systems triggered by the events that the rest of
//! the codebase already fires (death, attacks, dialogue, hunts), so adding a
//! new rule is: write a function, register it in `ContractPlugin::build`.
//!
//! The actual *punishment payload* (which Contract debuff, what tier) lives in
//! `apply_contract_punishment` so individual rule enforcers stay short.
//!
//! Rules whose effects are mechanical only (Rule I — Return is the resurrection
//! pipeline; Rule V — Coin is the hunt-reward path; Rule VII — Death & Return
//! is the rating system) do not appear here as enforcers; they live in
//! `combat_plugin.rs` and `quests.rs` where their data lives.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::battle::EnemyEncounter;
use crate::combat_plugin::{AttackIntentEvent, Bound, DeathEvent, ResurrectionStanding};
use crate::core::Timestamp;
use crate::quests::{
    DialogueChoicePickedEvent, HuntCompletedEvent, HuntFailedEvent, HuntRegistry,
};
use crate::status_effects::{
    ApplyStatusEvent, ContractDebuffKind, Expiry, RemoveStatusEvent, StatusKind,
};

/// Identifies which rule was broken. Useful for logging, dialogue branching,
/// and the (future) Clemency / Amendment systems that operate over the rule
/// space rather than over specific punishments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractRule {
    /// Rule II — The Veil. Speaking of Contract, Merchant, or laws.
    Secrecy,
    /// Rule III — The Bound Blade. Harm against another bound soul.
    NonAggression,
    /// Rule IV — The Calling. Refusing or neglecting a hunt.
    TaskObligation,
    /// Rule VI — The Sunrises. Completing a hunt past its deadline.
    Sunrises,
    /// Rule VIII — The Shared Path. Hindering another bound's hunt.
    SharedPath,
}

#[derive(Debug, Clone, Message)]
pub struct ContractViolatedEvent {
    pub rule: ContractRule,
    pub by: Entity,
    pub target: Option<Entity>,
    /// Extra context the atonement detectors need to know which specific
    /// situation is owed (which hunt to complete, which bound to help, etc.).
    /// Populated by the rule enforcer that fired this event.
    pub context: ViolationContext,
}

/// Per-rule data carried alongside a violation. The atonement detectors
/// match on this rather than re-deriving the situation from world state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationContext {
    None,
    /// `quest_id` of the hunt that was abandoned past the neglect window.
    /// Atonement: complete that exact hunt (DenyTheContract).
    NeglectedHunt { quest_id: u32 },
    /// Marker for late-completion violations. Atonement: complete the *next*
    /// hunt before its deadline (StolenHours).
    LateHunt,
    /// `affected_bound` is the original assignee whose hunt was sabotaged.
    /// Atonement: that same bound completes a hunt afterward (LoneShadow).
    SabotagedHunt { affected_bound: Entity },
    /// `victim` is the bound character that was attacked. Atonement:
    /// drink tea with them (MirrorWound).
    HarmedBound { victim: Entity },
    /// Atonement: stay silent for 3 days, then confess at a shrine
    /// (TongueOfAsh).
    SecrecyBreach,
}

/// Maps a violated rule to the Contract debuff payload (kind, tier) that
/// `apply_contract_punishment` then writes via `ApplyStatusEvent`.
fn punishment_for_rule(rule: ContractRule) -> (ContractDebuffKind, u8) {
    match rule {
        ContractRule::Secrecy => (ContractDebuffKind::TongueOfAsh, 1),
        ContractRule::NonAggression => (ContractDebuffKind::MirrorWound, 1),
        ContractRule::TaskObligation => (ContractDebuffKind::DenyTheContract, 1),
        ContractRule::Sunrises => (ContractDebuffKind::StolenHours, 1),
        ContractRule::SharedPath => (ContractDebuffKind::LoneShadow, 1),
    }
}

/// Apply the Contract's measured-fall response (Rule XI). Listens to
/// `ContractViolatedEvent` and translates into `ApplyStatusEvent` on the
/// offending bound character. Also bumps `contract_violations` on their
/// resurrection standing and records the pending atonement so detectors can
/// match later events (a completed hunt, a confession, a tea-share) against
/// the specific situation owed.
pub fn apply_contract_punishment(
    mut reader: MessageReader<ContractViolatedEvent>,
    mut writer: MessageWriter<ApplyStatusEvent>,
    mut q: Query<&mut ResurrectionStanding>,
    mut pending: ResMut<PendingAtonements>,
    timestamp: Res<Timestamp>,
    mut silence: ResMut<SilenceTrackers>,
) {
    for ev in reader.read() {
        let (kind, tier) = punishment_for_rule(ev.rule);
        writer.write(ApplyStatusEvent {
            target: ev.by,
            kind: StatusKind::Contract(kind),
            tier,
            source: None,
            expiry_override: Some(Expiry::UntilAtonement),
            resource_focus: None,
        });
        pending.0.insert((ev.by, kind), ev.context);
        // TongueOfAsh's silence window starts the moment the debuff lands —
        // anchor the tracker now so a confession 3 days later actually clears.
        if matches!(kind, ContractDebuffKind::TongueOfAsh) {
            silence.last_spoke.insert(ev.by, timestamp.0);
        }
        if let Ok(mut standing) = q.get_mut(ev.by) {
            standing.contract_violations =
                standing.contract_violations.saturating_add(1);
            // Heavy hit to standing — violations matter more than mediocre
            // hunting.
            standing.score = standing.score.saturating_sub(40);
        }
        info!(
            "Contract violation: {:?} broke {:?}; punishment {:?} t{} ctx={:?}",
            ev.by, ev.rule, kind, tier, ev.context
        );
    }
}

// ---------------------------------------------------------------------------
// Atonement
// ---------------------------------------------------------------------------

/// Per-(bound, debuff) record of *what* must be atoned. Detectors index this
/// by `(Entity, ContractDebuffKind)` to know if a given world event satisfies
/// the specific atonement that's owed.
#[derive(Resource, Default, Debug)]
pub struct PendingAtonements(pub HashMap<(Entity, ContractDebuffKind), ViolationContext>);

/// Tracks the last in-game timestamp at which a bound character spoke
/// (picked a dialogue choice). The TongueOfAsh atonement requires three days
/// of silence before a shrine confession will count.
#[derive(Resource, Default, Debug)]
pub struct SilenceTrackers {
    pub last_spoke: HashMap<Entity, u32>,
}

/// Fires when the player completes the in-fiction atonement task for a
/// specific contract debuff. `process_atonement_system` consumes it and
/// removes the corresponding `Contract` status.
#[derive(Debug, Clone, Message)]
pub struct AtonementCompletedEvent {
    pub who: Entity,
    pub debuff: ContractDebuffKind,
}

/// Fired when a bound character confesses at a shrine. World/dialogue code
/// fires this; `atonement_tongue_of_ash_system` checks whether the silence
/// window has been observed and, if so, completes the atonement.
#[derive(Debug, Clone, Message)]
pub struct ConfessAtShrineEvent {
    pub who: Entity,
}

/// Fired when a bound shares tea with another bound (the GDD's literal
/// "drink tea in the wagon with the injured" act). World/dialogue code fires
/// this; `atonement_mirror_wound_system` clears MirrorWound when the partner
/// is the original victim.
#[derive(Debug, Clone, Message)]
pub struct DrinkTeaWithBoundEvent {
    pub who: Entity,
    pub with_bound: Entity,
}

/// Updates the silence tracker every time a bound (or anyone, for now —
/// dialogue choices aren't yet tagged with speaker) picks a dialogue choice.
/// TongueOfAsh detection compares "now" to the most recent value here.
pub fn track_speech_system(
    mut reader: MessageReader<DialogueChoicePickedEvent>,
    player_q: Query<Entity, With<crate::core::Player>>,
    timestamp: Res<Timestamp>,
    mut silence: ResMut<SilenceTrackers>,
) {
    let Ok(speaker) = player_q.single() else {
        return;
    };
    for _ in reader.read() {
        silence.last_spoke.insert(speaker, timestamp.0);
    }
}

/// TongueOfAsh atonement — confession at a shrine after 3 days of silence.
pub fn atonement_tongue_of_ash_system(
    mut reader: MessageReader<ConfessAtShrineEvent>,
    pending: Res<PendingAtonements>,
    silence: Res<SilenceTrackers>,
    timestamp: Res<Timestamp>,
    mut writer: MessageWriter<AtonementCompletedEvent>,
) {
    const THREE_DAYS_TICKS: u32 = 3 * 24 * crate::constants::TIMESTAMP_TICKS_PER_HOUR;
    for ev in reader.read() {
        let key = (ev.who, ContractDebuffKind::TongueOfAsh);
        if !pending.0.contains_key(&key) {
            continue;
        }
        let last = silence.last_spoke.get(&ev.who).copied().unwrap_or(0);
        if timestamp.0.saturating_sub(last) < THREE_DAYS_TICKS {
            info!(
                "Confession by {:?} rejected: silence not yet 3 days (since={}, now={})",
                ev.who, last, timestamp.0
            );
            continue;
        }
        writer.write(AtonementCompletedEvent {
            who: ev.who,
            debuff: ContractDebuffKind::TongueOfAsh,
        });
    }
}

/// MirrorWound atonement — drink tea with the bound who was harmed.
pub fn atonement_mirror_wound_system(
    mut reader: MessageReader<DrinkTeaWithBoundEvent>,
    pending: Res<PendingAtonements>,
    mut writer: MessageWriter<AtonementCompletedEvent>,
) {
    for ev in reader.read() {
        let key = (ev.who, ContractDebuffKind::MirrorWound);
        let Some(ctx) = pending.0.get(&key) else {
            continue;
        };
        // Strict variant: only the original victim counts.
        if let ViolationContext::HarmedBound { victim } = ctx {
            if *victim != ev.with_bound {
                continue;
            }
        }
        writer.write(AtonementCompletedEvent {
            who: ev.who,
            debuff: ContractDebuffKind::MirrorWound,
        });
    }
}

/// DenyTheContract atonement — finish the *exact* hunt that was refused.
/// Stolen Hours atonement — finish the *next* hunt before its deadline.
/// LoneShadow atonement — the affected bound completes any hunt afterward.
pub fn atonement_hunt_completion_system(
    mut reader: MessageReader<HuntCompletedEvent>,
    pending: Res<PendingAtonements>,
    mut writer: MessageWriter<AtonementCompletedEvent>,
) {
    for ev in reader.read() {
        let Some(completer) = ev.completer else { continue };

        // DenyTheContract — must be this exact quest_id.
        if let Some(ViolationContext::NeglectedHunt { quest_id }) = pending
            .0
            .get(&(completer, ContractDebuffKind::DenyTheContract))
        {
            if *quest_id == ev.quest_id {
                writer.write(AtonementCompletedEvent {
                    who: completer,
                    debuff: ContractDebuffKind::DenyTheContract,
                });
            }
        }

        // StolenHours — any subsequent hunt completed on time.
        if pending
            .0
            .contains_key(&(completer, ContractDebuffKind::StolenHours))
            && !ev.completed_late
        {
            writer.write(AtonementCompletedEvent {
                who: completer,
                debuff: ContractDebuffKind::StolenHours,
            });
        }

        // LoneShadow — registered against the violator, but completed by the
        // affected bound; we walk every pending entry whose context names
        // this completer as the affected bound.
        let satisfied: Vec<Entity> = pending
            .0
            .iter()
            .filter_map(|((violator, kind), ctx)| {
                if !matches!(kind, ContractDebuffKind::LoneShadow) {
                    return None;
                }
                if let ViolationContext::SabotagedHunt { affected_bound } = ctx {
                    if *affected_bound == completer {
                        return Some(*violator);
                    }
                }
                None
            })
            .collect();
        for violator in satisfied {
            writer.write(AtonementCompletedEvent {
                who: violator,
                debuff: ContractDebuffKind::LoneShadow,
            });
        }
    }
}

/// Consumes `AtonementCompletedEvent`, removes the corresponding contract
/// status from the bound, and clears the pending-atonement record. Also
/// gives a small standing recovery — atonement is supposed to be redemptive.
pub fn process_atonement_system(
    mut reader: MessageReader<AtonementCompletedEvent>,
    mut pending: ResMut<PendingAtonements>,
    mut remove_writer: MessageWriter<RemoveStatusEvent>,
    mut q: Query<&mut ResurrectionStanding>,
) {
    for ev in reader.read() {
        if pending.0.remove(&(ev.who, ev.debuff)).is_none() {
            // Already cleared (duplicate event from the same hunt) — skip
            // standing bump as well.
            continue;
        }
        remove_writer.write(RemoveStatusEvent {
            target: ev.who,
            kind: StatusKind::Contract(ev.debuff),
        });
        if let Ok(mut standing) = q.get_mut(ev.who) {
            standing.score = standing.score.saturating_add(15);
        }
        info!("Atonement complete: {:?} for {:?}", ev.who, ev.debuff);
    }
}

// ---------------------------------------------------------------------------
// Rule II — The Veil (Secrecy)
// ---------------------------------------------------------------------------

/// Triggered when a bound soul reveals the Contract through a dialogue
/// choice tagged with the secrecy-violation event id. The dialogue catalog
/// owns the tagging; this system just listens for the choice and emits a
/// violation. Once a ContractCanonicalEvents resource is added, the magic
/// number can be replaced with a named constant.
pub fn rule_ii_secrecy(
    mut reader: MessageReader<crate::quests::DialogueChoicePickedEvent>,
    bound_q: Query<&Bound>,
    player_q: Query<Entity, With<crate::core::Player>>,
    mut writer: MessageWriter<ContractViolatedEvent>,
) {
    /// Designated event id used by dialogue choices that reveal the
    /// Contract. Pick something deliberately distinctive.
    const CONTRACT_REVEAL_EVENT: u32 = 0xC0FFEE_01;

    for ev in reader.read() {
        if ev.event_id != CONTRACT_REVEAL_EVENT {
            continue;
        }
        // Without a per-event speaker on the dialogue choice, fall back to
        // the local player as the violator. When dialogue speaker tracking
        // lands, prefer that.
        let Ok(by) = player_q.single() else {
            continue;
        };
        if bound_q.get(by).is_err() {
            continue;
        }
        writer.write(ContractViolatedEvent {
            rule: ContractRule::Secrecy,
            by,
            target: None,
            context: ViolationContext::SecrecyBreach,
        });
    }
}

// ---------------------------------------------------------------------------
// Rule III — The Bound Blade (Non-aggression)
// ---------------------------------------------------------------------------

/// A bound soul raising a hand against another bound soul. Watches attack
/// intents because intent-against-bound is the violation; whether the attack
/// lands is irrelevant per GDD ("Harm through intent is forbidden. The
/// Contract does not judge accidents...").
pub fn rule_iii_bound_blade(
    mut reader: MessageReader<AttackIntentEvent>,
    bound_q: Query<&Bound>,
    mut writer: MessageWriter<ContractViolatedEvent>,
) {
    for ev in reader.read() {
        if ev.attacker == ev.target {
            continue;
        }
        if bound_q.get(ev.attacker).is_err() {
            continue;
        }
        if bound_q.get(ev.target).is_err() {
            continue;
        }
        writer.write(ContractViolatedEvent {
            rule: ContractRule::NonAggression,
            by: ev.attacker,
            target: Some(ev.target),
            context: ViolationContext::HarmedBound { victim: ev.target },
        });
    }
}

// ---------------------------------------------------------------------------
// Rule IV — The Calling (Task Obligation)
// ---------------------------------------------------------------------------

/// A neglected hunt (more than a week past deadline) breaks the Calling. The
/// hunt-deadline system in `quests.rs` decides "neglected" vs. "merely late";
/// this enforcer just translates the neglected case into a Contract
/// violation.
pub fn rule_iv_calling(
    mut reader: MessageReader<HuntFailedEvent>,
    bound_q: Query<&Bound>,
    mut writer: MessageWriter<ContractViolatedEvent>,
) {
    for ev in reader.read() {
        if !ev.neglected {
            continue;
        }
        let Some(by) = ev.assigned_to else { continue };
        if bound_q.get(by).is_err() {
            continue;
        }
        writer.write(ContractViolatedEvent {
            rule: ContractRule::TaskObligation,
            by,
            target: None,
            context: ViolationContext::NeglectedHunt { quest_id: ev.quest_id },
        });
    }
}

// ---------------------------------------------------------------------------
// Rule VI — The Sunrises (Time)
// ---------------------------------------------------------------------------

/// Completing a hunt after its measure of time still earns coin (Rule VI is
/// not failure), but the Contract levies Stolen Hours as a measured response
/// to the delay.
pub fn rule_vi_sunrises(
    mut reader: MessageReader<HuntCompletedEvent>,
    bound_q: Query<&Bound>,
    mut writer: MessageWriter<ContractViolatedEvent>,
) {
    for ev in reader.read() {
        if !ev.completed_late {
            continue;
        }
        let Some(by) = ev.completer else { continue };
        if bound_q.get(by).is_err() {
            continue;
        }
        writer.write(ContractViolatedEvent {
            rule: ContractRule::Sunrises,
            by,
            target: None,
            context: ViolationContext::LateHunt,
        });
    }
}

// ---------------------------------------------------------------------------
// Rule VIII — The Shared Path (Inter-bound interference)
// ---------------------------------------------------------------------------

/// Sabotaging another bound soul's hunt — defined as "delivering the killing
/// blow on a target that a *different* bound soul was assigned to". Joins
/// `DeathEvent` against the `HuntRegistry` via `EnemyEncounter.id` to find
/// the hunt this kill belongs to, then fires a violation if the hunt's
/// assignee is a *different* bound from the killer.
///
/// Future obstruction modes (blocking the assignee from reaching the target,
/// destroying questgiver dialogue paths, etc.) can be added as additional
/// triggers writing the same `SabotagedHunt` violation.
pub fn rule_viii_shared_path(
    mut reader: MessageReader<DeathEvent>,
    bound_q: Query<&Bound>,
    enemy_q: Query<&EnemyEncounter>,
    hunts: Res<HuntRegistry>,
    mut writer: MessageWriter<ContractViolatedEvent>,
) {
    for ev in reader.read() {
        let Some(killer) = ev.killer else { continue };
        if bound_q.get(killer).is_err() {
            continue;
        }
        let Ok(encounter) = enemy_q.get(ev.entity) else {
            continue;
        };
        // Find an active hunt whose target enemy id matches this kill.
        let Some((_, hunt)) = hunts
            .0
            .iter()
            .find(|(_, h)| h.target_enemy_id == encounter.id)
        else {
            continue;
        };
        let Some(affected_bound) = hunt.assigned_to else {
            continue;
        };
        if affected_bound == killer {
            // Their own hunt — not a violation.
            continue;
        }
        if bound_q.get(affected_bound).is_err() {
            continue;
        }
        writer.write(ContractViolatedEvent {
            rule: ContractRule::SharedPath,
            by: killer,
            target: Some(affected_bound),
            context: ViolationContext::SabotagedHunt { affected_bound },
        });
    }
}

// ---------------------------------------------------------------------------
// Favors (Merchant services bought with Merchant Coins)
// ---------------------------------------------------------------------------

/// The catalog of favors a bound character can ask the Merchant for. Each
/// variant has a fixed coin cost; favors that affect a specific character
/// take the entity in the event payload, not in the variant.
///
/// Per GDD Rule V: "Each coin represents a favor, a currency with the
/// merchant itself." Per Rule XIII: the Merchant may forgive a punishment
/// "for exceptional service" — that's a separate, free path the Merchant
/// chooses unilaterally and is not part of this menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Favor {
    /// Cuts the resurrection delay to "instant + 1 minute" regardless of the
    /// character's standing. GDD-suggested cost is one coin per
    /// post-Forfeited resurrection.
    InstantResurrection,
    /// Cleanses a single Bad Condition (any tier).
    CleanseBadCondition,
    /// Cleanses a single Debuff (Minor or Severe).
    CleanseDebuff,
    /// Removes a single Contract debuff. Per GDD Rule XIII this is the
    /// Merchant's clemency made transactional — coins instead of an
    /// exceptional-service decision.
    RemoveContractDebuff,
}

impl Favor {
    /// Cost in Merchant Coins. Tunable; current values reflect the GDD's
    /// "removing a debuff must always cost more than a fresh resurrection
    /// with full resources" rule.
    pub fn coin_cost(self) -> u32 {
        match self {
            Favor::InstantResurrection => 1,
            Favor::CleanseBadCondition => 2,
            Favor::CleanseDebuff => 3,
            Favor::RemoveContractDebuff => 5,
        }
    }
}

/// Request to spend coins on a favor. `target` is the character the favor
/// affects; `kind_to_remove` carries the specific status for cleanse-style
/// favors (ignored for InstantResurrection).
#[derive(Debug, Clone, Message)]
pub struct AskFavorEvent {
    pub favor: Favor,
    pub target: Entity,
    pub kind_to_remove: Option<crate::status_effects::StatusKind>,
}

#[derive(Debug, Clone, Message)]
pub struct FavorGrantedEvent {
    pub favor: Favor,
    pub target: Entity,
    pub paid_coins: u32,
}

/// Translates `AskFavorEvent` into the right downstream effect:
/// - `InstantResurrection` shortens the awaiting-resurrection timer to "now"
///   so `process_resurrection_queue_system` resurrects on the next frame.
/// - `Cleanse*` / `RemoveContractDebuff` write a `RemoveStatusEvent` for the
///   `kind_to_remove` (no-op if missing).
///
/// All paths short-circuit if the player can't afford the favor's coin cost.
pub fn handle_ask_favor_system(
    mut reader: MessageReader<AskFavorEvent>,
    mut coins: ResMut<crate::quests::MerchantCoins>,
    mut awaiting_q: Query<&mut crate::combat_plugin::AwaitingResurrection>,
    timestamp: Res<crate::core::Timestamp>,
    mut remove_writer: MessageWriter<crate::status_effects::RemoveStatusEvent>,
    mut granted_writer: MessageWriter<FavorGrantedEvent>,
) {
    for ev in reader.read() {
        let cost = ev.favor.coin_cost();
        if coins.0 < cost {
            info!(
                "Favor {:?} denied: needs {} coins, player has {}",
                ev.favor, cost, coins.0
            );
            continue;
        }
        let mut succeeded = false;
        match ev.favor {
            Favor::InstantResurrection => {
                if let Ok(mut awaiting) = awaiting_q.get_mut(ev.target) {
                    awaiting.ready_at_timestamp = timestamp.0;
                    succeeded = true;
                }
            }
            Favor::CleanseBadCondition
            | Favor::CleanseDebuff
            | Favor::RemoveContractDebuff => {
                if let Some(kind) = ev.kind_to_remove {
                    remove_writer.write(crate::status_effects::RemoveStatusEvent {
                        target: ev.target,
                        kind,
                    });
                    succeeded = true;
                }
            }
        }
        if succeeded {
            coins.0 -= cost;
            granted_writer.write(FavorGrantedEvent {
                favor: ev.favor,
                target: ev.target,
                paid_coins: cost,
            });
            info!(
                "Favor {:?} granted to {:?} for {} coins",
                ev.favor, ev.target, cost
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct ContractPlugin;

impl Plugin for ContractPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingAtonements>()
            .init_resource::<SilenceTrackers>()
            .add_message::<ContractViolatedEvent>()
            .add_message::<AskFavorEvent>()
            .add_message::<FavorGrantedEvent>()
            .add_message::<AtonementCompletedEvent>()
            .add_message::<ConfessAtShrineEvent>()
            .add_message::<DrinkTeaWithBoundEvent>()
            .add_systems(
                Update,
                (
                    rule_ii_secrecy,
                    rule_iii_bound_blade,
                    rule_iv_calling,
                    rule_vi_sunrises,
                    rule_viii_shared_path,
                    apply_contract_punishment,
                    handle_ask_favor_system,
                    track_speech_system,
                    atonement_tongue_of_ash_system.after(track_speech_system),
                    atonement_mirror_wound_system,
                    atonement_hunt_completion_system,
                    process_atonement_system
                        .after(atonement_tongue_of_ash_system)
                        .after(atonement_mirror_wound_system)
                        .after(atonement_hunt_completion_system),
                ),
            );
    }
}
