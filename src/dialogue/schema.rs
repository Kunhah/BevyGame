use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub type NodeId = String;
pub type SceneId = String;
pub type ItemId = u32;
pub type QuestId = u32;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DialogueScene {
    pub id: SceneId,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub music: Option<String>,
    pub start: NodeId,
    pub nodes: HashMap<NodeId, DialogueNode>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum DialogueNode {
    Line(LineNode),
    Choice(ChoiceNode),
    Scene(SceneNode),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LineNode {
    pub speaker: Speaker,
    pub text: String,
    #[serde(default)]
    pub on_enter: Vec<Effect>,
    #[serde(default)]
    pub condition: Option<Condition>,
    #[serde(default)]
    pub next: Option<NodeId>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChoiceNode {
    #[serde(default)]
    pub prompt: Option<Speaker>,
    #[serde(default)]
    pub prompt_text: Option<String>,
    pub options: Vec<ChoiceOption>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChoiceOption {
    pub text: String,
    #[serde(default)]
    pub condition: Option<Condition>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    #[serde(default)]
    pub next: Option<NodeId>,
    /// Legacy `event: u32` from the pre-schema RON files. Preserved here
    /// because `quests.rs::ObjectiveKind::DialogueChoice` and
    /// `contract.rs::rule_ii_secrecy` still match against it. New scenes
    /// should express progression through `effects` instead; this field
    /// will be removed once those consumers migrate (planned step 7).
    #[serde(default)]
    pub legacy_event_id: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SceneNode {
    pub actions: Vec<SceneAction>,
    #[serde(default)]
    pub next: Option<NodeId>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Speaker {
    pub name: String,
    #[serde(default)]
    pub slot: SpeakerSlot,
    #[serde(default)]
    pub expression: Option<String>,
}

/// 12 evenly-spaced horizontal positions for character portraits during cutscenes.
/// Slot1 is left-most, Slot12 is right-most. Slot6/Slot7 sit nearest the center.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpeakerSlot {
    #[default]
    Slot1,
    Slot2,
    Slot3,
    Slot4,
    Slot5,
    Slot6,
    Slot7,
    Slot8,
    Slot9,
    Slot10,
    Slot11,
    Slot12,
}

#[allow(dead_code)] // wired up by the multi-portrait cutscene UI in a later step
impl SpeakerSlot {
    pub fn index(self) -> u8 {
        match self {
            SpeakerSlot::Slot1 => 0,
            SpeakerSlot::Slot2 => 1,
            SpeakerSlot::Slot3 => 2,
            SpeakerSlot::Slot4 => 3,
            SpeakerSlot::Slot5 => 4,
            SpeakerSlot::Slot6 => 5,
            SpeakerSlot::Slot7 => 6,
            SpeakerSlot::Slot8 => 7,
            SpeakerSlot::Slot9 => 8,
            SpeakerSlot::Slot10 => 9,
            SpeakerSlot::Slot11 => 10,
            SpeakerSlot::Slot12 => 11,
        }
    }

    /// Horizontal screen fraction (0.0 = left, 1.0 = right). Each slot sits at the
    /// center of an equal column when split into 12 columns.
    pub fn x_fraction(self) -> f32 {
        (self.index() as f32 + 0.5) / 12.0
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    Flag(String),
    NotFlag(String),
    HasItem { item: ItemId, qty: u32 },
    QuestStatus { quest: QuestId, status: QuestStatusFilter },
    ReputationAtLeast { target: ReputationTargetRef, min: i32 },
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuestStatusFilter {
    Inactive,
    Active,
    Completed,
    Failed,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ReputationTargetRef {
    LocalGovernor,
    LocalMerchant,
    LocalClan,
    Governor { city_id: u16 },
    Merchant { merchant_id: u16 },
    Clan { name: String },
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    SetFlag(String),
    ClearFlag(String),
    Reputation {
        target: ReputationTargetRef,
        delta: i16,
        reason: String,
    },
    GiveItem { item: ItemId, qty: u32 },
    TakeItem { item: ItemId, qty: u32 },
    GiveCoin(u32),
    TakeCoin(u32),
    StartQuest(QuestId),
    AdvanceObjective { quest: QuestId, objective: u32 },
    AcceptContract,
    PlaySfx(String),
    /// None stops the current music.
    PlayMusic(Option<String>),
    SpawnInteractable { kind: String, x: f32, y: f32 },
    DespawnInteractable { name: String },
    ChangeScene(SceneId),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum SceneAction {
    EnterCharacter {
        name: String,
        slot: SpeakerSlot,
        #[serde(default)]
        expression: Option<String>,
        #[serde(default)]
        transition_secs: f32,
    },
    ExitCharacter {
        name: String,
        #[serde(default)]
        transition_secs: f32,
    },
    SetExpression {
        name: String,
        #[serde(default)]
        expression: Option<String>,
    },
    SetBackground(Option<String>),
    PlayMusic(Option<String>),
    PlaySfx(String),
    Wait(f32),
    FadeOut(f32),
    FadeIn(f32),
    ShakeScreen(f32),
}
