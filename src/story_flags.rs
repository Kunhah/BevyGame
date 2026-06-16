use std::collections::HashMap;

use bevy::prelude::*;
use bevy::prelude::Messages;
use fixedbitset::FixedBitSet;

/// Story flags stored as one bit per flag rather than a set of strings.
///
/// Flag names live in data files (dialogue / quest / world-rule RON), so the
/// universe of flags isn't known at compile time. A runtime registry assigns
/// each name a bit index the first time it's seen; `bits` then holds the on/off
/// state with a single bit each.
///
/// The indices are per-process and never observed outside this struct — the
/// save format stores flag *names* (see [`Self::iter_set_names`] /
/// [`Self::from_names`]) and re-interns them on load. That keeps the in-memory
/// representation compact while letting content add or rename flags freely
/// without ever invalidating an existing save.
#[derive(Resource, Debug, Default, Clone)]
pub struct StoryFlags {
    bits: FixedBitSet,
    index_of: HashMap<String, usize>,
    names: Vec<String>,
}

impl StoryFlags {
    /// Bit index for `name`, assigning the next free one if it's new.
    fn intern(&mut self, name: String) -> usize {
        if let Some(&i) = self.index_of.get(&name) {
            return i;
        }
        let i = self.names.len();
        self.index_of.insert(name.clone(), i);
        self.names.push(name);
        self.bits.grow(i + 1);
        i
    }

    pub fn is_set(&self, name: &str) -> bool {
        // A name we've never interned can't be set — and `is_set` must not
        // mutate the registry, so we look up without interning.
        self.index_of
            .get(name)
            .is_some_and(|&i| self.bits.contains(i))
    }

    /// Set the flag. Returns `true` if it was newly set (preserves the old
    /// `HashSet::insert` contract callers rely on to gate change events).
    pub fn set(&mut self, name: impl Into<String>) -> bool {
        let i = self.intern(name.into());
        if self.bits.contains(i) {
            false
        } else {
            self.bits.insert(i);
            true
        }
    }

    /// Clear the flag. Returns `true` if it had been set (preserves the old
    /// `HashSet::remove` contract).
    pub fn clear(&mut self, name: &str) -> bool {
        match self.index_of.get(name) {
            Some(&i) if self.bits.contains(i) => {
                self.bits.set(i, false);
                true
            }
            _ => false,
        }
    }

    /// Names of all currently-set flags, for serialization. Order is unspecified.
    pub fn iter_set_names(&self) -> impl Iterator<Item = &str> {
        self.bits.ones().map(|i| self.names[i].as_str())
    }

    /// Rebuild a flag set from a list of set-flag names (the save's storage form).
    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut flags = Self::default();
        for name in names {
            flags.set(name);
        }
        flags
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_clear_check_match_old_hashset_semantics() {
        let mut flags = StoryFlags::default();

        // Unknown flag reads as unset and must not register a bit.
        assert!(!flags.is_set("met_elder"));

        // set returns true only the first time (HashSet::insert contract).
        assert!(flags.set("met_elder"));
        assert!(!flags.set("met_elder"));
        assert!(flags.is_set("met_elder"));

        // A second flag is independent.
        assert!(flags.set("saw_omen"));
        assert!(flags.is_set("saw_omen"));
        assert!(flags.is_set("met_elder"));

        // clear returns true only when it was set (HashSet::remove contract).
        assert!(flags.clear("met_elder"));
        assert!(!flags.clear("met_elder"));
        assert!(!flags.is_set("met_elder"));
        // Clearing a never-seen flag is a no-op false.
        assert!(!flags.clear("nonexistent"));
        // The other flag is untouched.
        assert!(flags.is_set("saw_omen"));
    }

    #[test]
    fn name_round_trip_preserves_set_flags() {
        let mut flags = StoryFlags::default();
        flags.set("a");
        flags.set("b");
        flags.set("c");
        flags.clear("b");

        let names: Vec<String> = flags.iter_set_names().map(String::from).collect();
        let restored = StoryFlags::from_names(names);

        assert!(restored.is_set("a"));
        assert!(!restored.is_set("b"));
        assert!(restored.is_set("c"));
        assert_eq!(restored.iter_set_names().count(), 2);
    }

    #[test]
    fn reusing_a_bit_after_clear_does_not_leak_to_other_flags() {
        // Interning is by name, so a cleared flag's bit stays bound to its name.
        let mut flags = StoryFlags::default();
        flags.set("first");
        flags.clear("first");
        flags.set("second");
        assert!(!flags.is_set("first"));
        assert!(flags.is_set("second"));
    }
}
