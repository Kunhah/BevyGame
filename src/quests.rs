use std::collections::HashMap;

use bevy::ecs::message::MessageReader;
use bevy::prelude::*;

use crate::combat_plugin::DeathEvent;
use crate::core::Player;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectiveState {
    Pending,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestStatus {
    Active,
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct QuestObjective {
    pub id: u32,
    pub description: String,
    pub state: ObjectiveState,
}

#[derive(Debug, Clone)]
pub struct Quest {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub objectives: Vec<QuestObjective>,
    pub status: QuestStatus,
}

impl Quest {
    fn recalc_status(&mut self) {
        if self
            .objectives
            .iter()
            .any(|objective| objective.state == ObjectiveState::Failed)
        {
            self.status = QuestStatus::Failed;
            return;
        }

        if self
            .objectives
            .iter()
            .all(|objective| objective.state == ObjectiveState::Completed)
        {
            self.status = QuestStatus::Completed;
            return;
        }

        self.status = QuestStatus::Active;
    }
}

#[derive(Debug, Clone)]
pub struct ObjectiveDefinition {
    pub id: u32,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct QuestDefinition {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub objectives: Vec<ObjectiveDefinition>,
}

#[derive(Debug, Clone, Copy)]
pub struct QuestUpdateAction {
    pub quest_id: u32,
    pub objective_id: u32,
    pub new_state: ObjectiveState,
}

#[derive(Resource, Default)]
pub struct QuestLog {
    pub quests: HashMap<u32, Quest>,
}

impl QuestLog {
    pub fn add_quest(&mut self, definition: QuestDefinition) -> bool {
        if self.quests.contains_key(&definition.id) {
            return false;
        }

        let objectives = definition
            .objectives
            .into_iter()
            .map(|objective| QuestObjective {
                id: objective.id,
                description: objective.description,
                state: ObjectiveState::Pending,
            })
            .collect();

        let quest = Quest {
            id: definition.id,
            title: definition.title,
            description: definition.description,
            objectives,
            status: QuestStatus::Active,
        };

        self.quests.insert(quest.id, quest);
        true
    }

    pub fn add_objective(
        &mut self,
        quest_id: u32,
        objective: ObjectiveDefinition,
    ) -> Result<(), QuestUpdateError> {
        let quest = self
            .quests
            .get_mut(&quest_id)
            .ok_or(QuestUpdateError::MissingQuest(quest_id))?;

        if quest
            .objectives
            .iter()
            .any(|existing| existing.id == objective.id)
        {
            return Err(QuestUpdateError::DuplicateObjective {
                quest_id,
                objective_id: objective.id,
            });
        }

        quest.objectives.push(QuestObjective {
            id: objective.id,
            description: objective.description,
            state: ObjectiveState::Pending,
        });
        quest.recalc_status();
        Ok(())
    }

    pub fn update_objective_state(
        &mut self,
        quest_id: u32,
        objective_id: u32,
        new_state: ObjectiveState,
    ) -> Result<Option<QuestStatus>, QuestUpdateError> {
        let quest = self
            .quests
            .get_mut(&quest_id)
            .ok_or(QuestUpdateError::MissingQuest(quest_id))?;

        let Some(objective) = quest
            .objectives
            .iter_mut()
            .find(|objective| objective.id == objective_id)
        else {
            return Err(QuestUpdateError::MissingObjective {
                quest_id,
                objective_id,
            });
        };

        objective.state = new_state;

        let previous_status = quest.status;
        quest.recalc_status();

        if quest.status != previous_status {
            Ok(Some(quest.status))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug)]
pub enum QuestUpdateError {
    MissingQuest(u32),
    MissingObjective { quest_id: u32, objective_id: u32 },
    DuplicateObjective { quest_id: u32, objective_id: u32 },
}

#[derive(Event, Message, Debug, Clone)]
pub struct AddQuestEvent {
    pub quest: QuestDefinition,
}

#[derive(Event, Message, Debug, Clone)]
pub struct AddObjectiveEvent {
    pub quest_id: u32,
    pub objective: ObjectiveDefinition,
}

#[derive(Event, Message, Debug, Clone, Copy)]
pub struct UpdateObjectiveEvent {
    pub quest_id: u32,
    pub objective_id: u32,
    pub new_state: ObjectiveState,
}

#[derive(Event, Message, Debug, Clone, Copy)]
pub struct QuestStatusChangedEvent {
    pub quest_id: u32,
    pub status: QuestStatus,
}

#[derive(Event, Message, Debug, Clone, Copy)]
pub struct ItemPickupEvent {
    pub entity: Entity,
}

/// Optional quest hooks attached to gameplay entities.
#[derive(Component, Default)]
pub struct OnItemPickup(pub Option<QuestUpdateAction>);

#[derive(Component, Default)]
pub struct OnDeath(pub Option<QuestUpdateAction>);

#[derive(Component)]
pub struct OnReach {
    pub radius: f32,
    pub action: Option<QuestUpdateAction>,
    pub fired: bool,
}

impl Default for OnReach {
    fn default() -> Self {
        Self {
            radius: 48.0,
            action: None,
            fired: false,
        }
    }
}

fn ingest_new_quests(mut quest_log: ResMut<QuestLog>, mut events: ResMut<Messages<AddQuestEvent>>) {
    for event in events.drain() {
        let quest_id = event.quest.id;

        if !quest_log.add_quest(event.quest) {
            warn!("Quest with id {quest_id} already exists; ignoring add request.");
        }
    }
}

fn ingest_new_objectives(
    mut quest_log: ResMut<QuestLog>,
    mut events: ResMut<Messages<AddObjectiveEvent>>,
) {
    for event in events.drain() {
        if let Err(err) = quest_log.add_objective(event.quest_id, event.objective.clone()) {
            match err {
                QuestUpdateError::MissingQuest(quest_id) => {
                    warn!(
                        "Received AddObjectiveEvent for missing quest {quest_id} (objective {}).",
                        event.objective.id
                    );
                }
                QuestUpdateError::DuplicateObjective {
                    quest_id,
                    objective_id,
                } => {
                    warn!(
                        "Quest {quest_id} already has objective {objective_id}; skipping add."
                    );
                }
                _ => {}
            }
        }
    }
}

fn apply_objective_updates(
    mut quest_log: ResMut<QuestLog>,
    mut update_events: ResMut<Messages<UpdateObjectiveEvent>>,
    mut status_changed: ResMut<Messages<QuestStatusChangedEvent>>,
) {
    for event in update_events.drain() {
        match quest_log.update_objective_state(
            event.quest_id,
            event.objective_id,
            event.new_state,
        ) {
            Ok(Some(status)) => {
                let _ = status_changed.send(QuestStatusChangedEvent {
                    quest_id: event.quest_id,
                    status,
                });
            }
            Ok(None) => {}
            Err(QuestUpdateError::MissingQuest(quest_id)) => {
                warn!(
                    "Received objective update for unknown quest {quest_id} (objective {}).",
                    event.objective_id
                );
            }
            Err(QuestUpdateError::MissingObjective {
                quest_id,
                objective_id,
            }) => {
                warn!("Quest {quest_id} has no objective {objective_id} to update.");
            }
            Err(err) => {
                warn!(
                    "Failed to update objective {} for quest {}: {:?}",
                    event.objective_id, event.quest_id, err
                );
            }
        }
    }
}

/// Trigger quest updates for items when they are picked up.
fn trigger_on_item_pickup(
    mut pickup_events: ResMut<Messages<ItemPickupEvent>>,
    hooks: Query<&OnItemPickup>,
    mut updates: ResMut<Messages<UpdateObjectiveEvent>>,
) {
    for event in pickup_events.drain() {
        if let Ok(OnItemPickup(Some(action))) = hooks.get(event.entity) {
            updates.send(UpdateObjectiveEvent {
                quest_id: action.quest_id,
                objective_id: action.objective_id,
                new_state: action.new_state,
            });
        }
    }
}

/// Trigger quest updates when entities with `OnDeath` die in combat.
fn trigger_on_death(
    mut death_events: MessageReader<DeathEvent>,
    hooks: Query<&OnDeath>,
    mut updates: ResMut<Messages<UpdateObjectiveEvent>>,
) {
    for event in death_events.read() {
        if let Ok(OnDeath(Some(action))) = hooks.get(event.entity) {
            updates.send(UpdateObjectiveEvent {
                quest_id: action.quest_id,
                objective_id: action.objective_id,
                new_state: action.new_state,
            });
        }
    }
}

/// Trigger quest updates when the player comes within `radius` of an entity.
fn trigger_on_reach(
    player: Query<&Transform, With<Player>>,
    mut hooks: Query<(&Transform, &mut OnReach)>,
    mut updates: ResMut<Messages<UpdateObjectiveEvent>>,
) {
    let Ok(player_tf) = player.single() else {
        return;
    };
    let player_pos = player_tf.translation.truncate();

    for (tf, mut hook) in hooks.iter_mut() {
        if hook.fired {
            continue;
        }

        if let Some(action) = hook.action {
            let pos = tf.translation.truncate();
            if player_pos.distance(pos) <= hook.radius {
                updates.send(UpdateObjectiveEvent {
                    quest_id: action.quest_id,
                    objective_id: action.objective_id,
                    new_state: action.new_state,
                });
                hook.fired = true;
            }
        }
    }
}

pub struct QuestPlugin;

impl Plugin for QuestPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(QuestLog::default())
            .insert_resource(Messages::<AddQuestEvent>::default())
            .insert_resource(Messages::<AddObjectiveEvent>::default())
            .insert_resource(Messages::<UpdateObjectiveEvent>::default())
            .insert_resource(Messages::<QuestStatusChangedEvent>::default())
            .insert_resource(Messages::<ItemPickupEvent>::default())
            .add_systems(
                Update,
                (
                    ingest_new_quests,
                    ingest_new_objectives,
                    apply_objective_updates,
                    trigger_on_item_pickup,
                    trigger_on_death,
                    trigger_on_reach,
                ),
            );
    }
}
