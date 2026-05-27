use std::collections::HashMap;
use std::path::Path;

use bevy::prelude::*;

use crate::ui_style::palette;

use super::runtime::DialogueRuntime;
use super::schema::{DialogueNode, SpeakerSlot};

const STAGE_HEIGHT_RESERVED_PX: f32 = 220.0; // dialogue box (200) + small gap
const PORTRAIT_WIDTH: f32 = 128.0;
const PORTRAIT_HEIGHT: f32 = 192.0;
const NAMEPLATE_HEIGHT: f32 = 24.0;

const SPEAKER_SLOTS: [SpeakerSlot; 12] = [
    SpeakerSlot::Slot1,
    SpeakerSlot::Slot2,
    SpeakerSlot::Slot3,
    SpeakerSlot::Slot4,
    SpeakerSlot::Slot5,
    SpeakerSlot::Slot6,
    SpeakerSlot::Slot7,
    SpeakerSlot::Slot8,
    SpeakerSlot::Slot9,
    SpeakerSlot::Slot10,
    SpeakerSlot::Slot11,
    SpeakerSlot::Slot12,
];

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageEntry {
    pub name: String,
    pub expression: Option<String>,
}

#[derive(Resource, Default)]
pub struct StageState {
    pub slots: HashMap<SpeakerSlot, StageEntry>,
    /// Name of the speaker currently delivering a line — used by the visuals
    /// to dim the rest. `None` when no Line node is active (e.g. between
    /// scene actions, or pure narration with empty speaker).
    pub active_speaker: Option<String>,
    /// Optional background image asset path (relative to `assets/`).
    pub background: Option<String>,
}

impl StageState {
    pub fn place(&mut self, slot: SpeakerSlot, entry: StageEntry) {
        self.slots.insert(slot, entry);
    }

    pub fn remove_named(&mut self, name: &str) {
        self.slots.retain(|_, entry| entry.name != name);
    }

    pub fn set_expression(&mut self, name: &str, expression: Option<String>) {
        for entry in self.slots.values_mut() {
            if entry.name == name {
                entry.expression = expression.clone();
            }
        }
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.active_speaker = None;
        self.background = None;
    }
}

// ---------------------------------------------------------------------------
// UI Components
// ---------------------------------------------------------------------------

#[derive(Component)]
pub struct StageRoot;

#[derive(Component)]
pub struct StageSlotMarker;

#[derive(Component)]
pub struct StagePortraitImage(pub SpeakerSlot);

#[derive(Component)]
pub struct StagePortraitName(pub SpeakerSlot);

#[derive(Component)]
pub struct DialogueBackgroundRoot;

#[derive(Component)]
pub struct DialogueBackgroundImage;

#[derive(Component)]
pub struct FadeOverlay;

// ---------------------------------------------------------------------------
// Spawn / despawn
// ---------------------------------------------------------------------------

/// Spawns the background, 12-slot stage, and fade overlay. Called by the
/// dialogue-box spawn flow so all overlay entities live for the same lifetime.
pub fn spawn_stage_overlays(commands: &mut Commands) {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                ..default()
            },
            BackgroundColor(Color::NONE),
            ZIndex(-10),
            DialogueBackgroundRoot,
        ))
        .with_children(|parent| {
            parent.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                ImageNode {
                    image_mode: NodeImageMode::Stretch,
                    ..default()
                },
                Visibility::Hidden,
                DialogueBackgroundImage,
            ));
        });

    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                padding: UiRect {
                    bottom: Val::Px(STAGE_HEIGHT_RESERVED_PX),
                    ..default()
                },
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceAround,
                align_items: AlignItems::FlexEnd,
                ..default()
            },
            BackgroundColor(Color::NONE),
            ZIndex(-1),
            StageRoot,
        ))
        .with_children(|parent| {
            for slot in SPEAKER_SLOTS {
                spawn_slot(parent, slot);
            }
        });

    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
        ZIndex(50),
        FadeOverlay,
    ));
}

fn spawn_slot(parent: &mut ChildSpawnerCommands, slot: SpeakerSlot) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0 / 12.0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                row_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(Color::NONE),
            StageSlotMarker,
        ))
        .with_children(|inner| {
            inner.spawn((
                Node {
                    width: Val::Px(PORTRAIT_WIDTH),
                    height: Val::Px(PORTRAIT_HEIGHT),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                ImageNode {
                    image_mode: NodeImageMode::Stretch,
                    ..default()
                },
                BorderColor::all(palette::BORDER_SUBTLE),
                Visibility::Hidden,
                StagePortraitImage(slot),
            ));
            inner.spawn((
                Node {
                    height: Val::Px(NAMEPLATE_HEIGHT),
                    ..default()
                },
                Text::new(""),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(palette::TEXT_SECONDARY),
                Visibility::Hidden,
                StagePortraitName(slot),
            ));
        });
}

pub fn despawn_stage_when_inactive(
    mut commands: Commands,
    runtime: Res<DialogueRuntime>,
    mut stage_state: ResMut<StageState>,
    stage_q: Query<Entity, With<StageRoot>>,
    bg_q: Query<Entity, With<DialogueBackgroundRoot>>,
    fade_q: Query<Entity, With<FadeOverlay>>,
) {
    if runtime.active {
        return;
    }
    for e in stage_q.iter().chain(bg_q.iter()).chain(fade_q.iter()) {
        commands.entity(e).despawn();
    }
    stage_state.clear();
}

// ---------------------------------------------------------------------------
// Render: keep portrait images / nameplates / dim level in sync with
// `StageState` and the active speaker.
// ---------------------------------------------------------------------------

pub fn refresh_stage_visuals(
    stage: Res<StageState>,
    asset_server: Res<AssetServer>,
    mut img_q: Query<
        (&StagePortraitImage, &mut ImageNode, &mut Visibility),
        Without<StagePortraitName>,
    >,
    mut name_q: Query<
        (&StagePortraitName, &mut Text, &mut Visibility, &mut TextColor),
        Without<StagePortraitImage>,
    >,
    mut bg_q: Query<(&mut ImageNode, &mut Visibility), With<DialogueBackgroundImage>>,
) {
    for (StagePortraitImage(slot), mut image_node, mut vis) in img_q.iter_mut() {
        match stage.slots.get(slot) {
            Some(entry) => {
                if let Some(path) = portrait_asset_path(&entry.name, entry.expression.as_deref()) {
                    image_node.image = asset_server.load(path);
                    image_node.image_mode = NodeImageMode::Stretch;
                    image_node.color = active_color(&stage.active_speaker, &entry.name);
                    *vis = Visibility::Visible;
                } else {
                    *vis = Visibility::Hidden;
                }
            }
            None => *vis = Visibility::Hidden,
        }
    }

    for (StagePortraitName(slot), mut text, mut vis, mut color) in name_q.iter_mut() {
        match stage.slots.get(slot) {
            Some(entry) => {
                *text = Text::new(entry.name.clone());
                let active =
                    matches!(&stage.active_speaker, Some(name) if name == &entry.name);
                color.0 = if active {
                    palette::TEXT_HEADING
                } else {
                    palette::TEXT_DIM
                };
                *vis = Visibility::Visible;
            }
            None => *vis = Visibility::Hidden,
        }
    }

    if let Ok((mut image_node, mut vis)) = bg_q.single_mut() {
        match &stage.background {
            Some(path) => {
                image_node.image = asset_server.load(path);
                image_node.image_mode = NodeImageMode::Stretch;
                *vis = Visibility::Visible;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

fn active_color(active_speaker: &Option<String>, name: &str) -> Color {
    let dimmed = matches!(active_speaker, Some(active) if active != name);
    if dimmed {
        Color::srgba(0.55, 0.55, 0.6, 1.0)
    } else {
        Color::WHITE
    }
}

// ---------------------------------------------------------------------------
// Auto-placement: when entering a Line node, ensure the speaker is on stage.
// ---------------------------------------------------------------------------

pub fn auto_place_line_speaker(
    runtime: Res<DialogueRuntime>,
    catalog: Res<super::runtime::DialogueCatalog>,
    mut stage: ResMut<StageState>,
) {
    if !runtime.active {
        return;
    }
    let Some(node) = runtime.current_node(&catalog) else {
        return;
    };
    let DialogueNode::Line(line) = node else {
        // Choice and Scene nodes manage stage state via their own paths
        // (Choice uses the prompt's speaker; Scene uses explicit actions).
        return;
    };
    let name = line.speaker.name.trim();
    if name.is_empty() {
        stage.active_speaker = None;
        return;
    }
    let slot = line.speaker.slot;
    let expression = line.speaker.expression.clone();

    let needs_update = stage
        .slots
        .get(&slot)
        .map(|entry| entry.name != name || entry.expression != expression)
        .unwrap_or(true);
    if needs_update {
        stage.place(
            slot,
            StageEntry {
                name: name.to_string(),
                expression,
            },
        );
    }
    stage.active_speaker = Some(name.to_string());
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn portrait_asset_path(speaker: &str, expression: Option<&str>) -> Option<String> {
    let speaker = speaker.trim();
    if speaker.is_empty() {
        return None;
    }
    let base = Path::new("assets").join("portraits");
    if let Some(expr) = expression {
        let expr = expr.trim();
        if !expr.is_empty() {
            for ext in ["png", "jpg", "jpeg", "webp"] {
                let file_name = format!("{speaker}_{expr}.{ext}");
                if base.join(&file_name).is_file() {
                    return Some(format!("portraits/{file_name}"));
                }
            }
        }
    }
    for ext in ["png", "jpg", "jpeg", "webp"] {
        let file_name = format!("{speaker}.{ext}");
        if base.join(&file_name).is_file() {
            return Some(format!("portraits/{file_name}"));
        }
    }
    None
}
