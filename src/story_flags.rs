use std::collections::HashSet;

use bevy::prelude::*;
use bevy::prelude::Messages;

#[derive(Resource, Debug, Default, Clone)]
pub struct StoryFlags(pub HashSet<String>);

impl StoryFlags {
    pub fn is_set(&self, name: &str) -> bool {
        self.0.contains(name)
    }

    pub fn set(&mut self, name: impl Into<String>) -> bool {
        self.0.insert(name.into())
    }

    pub fn clear(&mut self, name: &str) -> bool {
        self.0.remove(name)
    }
}

/// Fired when a story flag transitions. The dialogue effect dispatcher is the
/// canonical mutation point and emits this event; anything that mutates
/// `StoryFlags` directly should also write to the corresponding message buffer
/// so world-rule triggers stay accurate.
#[derive(Event, Message, Debug, Clone)]
pub struct FlagChangedEvent {
    pub name: String,
    pub set: bool,
}

pub struct StoryFlagsPlugin;

impl Plugin for StoryFlagsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StoryFlags>()
            .insert_resource(Messages::<FlagChangedEvent>::default());
    }
}
