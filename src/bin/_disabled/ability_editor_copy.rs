// ability_editor.rs
// A standalone Ability Editor as a separate binary
// Place as: src/bin/ability_editor.rs

// Cargo.toml dependencies:
// [dependencies]
// bevy = { version = "0.11" }
// bevy_egui = "0.22"
// serde = { version = "1.0", features = ["derive"] }
// serde_json = "1.0"

use bevy::prelude::*;
use bevy::window::{Window, WindowPlugin};
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

const DEFAULT_ABILITY_PATH: &str = "src/abilities/AbilitiesExample.json";

// ---------------- Data models ----------------
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Stat {
    Mind,
    Agility,
    Strength,
    Morale,
    Lethality,
    // add more as needed
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DamageType {
    Physical,
    Fire,
    Ice,
    Lightning,
    // add more as needed
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AbilityEffect {
    Heal { floor: u32, ceiling: u32, scaled_with: Stat },
    Damage { floor: u32, ceiling: u32, damage_type: DamageType, scaled_with: Stat, defended_with: Stat },
    Buff { stat: Stat, multiplier: f32, effects: Option<Vec<u16>>, scaled_with: Stat },
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

impl Default for Ability {
    fn default() -> Self {
        Self {
            id: 0,
            next_id: None,
            name: "New Ability".into(),
            health_cost: 0,
            magic_cost: 0,
            stamina_cost: 0,
            cooldown: 0,
            description: String::new(),
            effects: Vec::new(),
            shape: AbilityShape::Select,
            duration: 0,
            targets: 1,
        }
    }
}

#[derive(Resource)]
struct AbilitiesResource {
    abilities: Vec<Ability>,
    file_path: PathBuf,
    dirty: bool,
    validation_messages: Vec<String>,
    selected: Option<usize>,
}

impl Default for AbilitiesResource {
    fn default() -> Self {
        let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        path.push(DEFAULT_ABILITY_PATH);
        Self {
            abilities: Vec::new(),
            file_path: path,
            dirty: false,
            validation_messages: Vec::new(),
            selected: None,
        }
    }
}

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Ability Editor".into(),
                        resolution: (1100, 720).into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(EguiPlugin::default())
        .init_resource::<AbilitiesResource>()
        .add_systems(Startup, setup_system)
        .add_systems(Update, ui_system)
        .run();
}

fn setup_system(mut r: ResMut<AbilitiesResource>) {
    if r.abilities.is_empty() {
        if let Ok(content) = fs::read_to_string(&r.file_path) {
            if let Ok(v) = serde_json::from_str::<Vec<Ability>>(&content) {
                r.abilities = v;
                r.dirty = false;
                return;
            }
        }

        // add an example if nothing could be loaded
        r.abilities.push(Ability {
            id: 0,
            next_id: None,
            name: "Minor Heal".into(),
            health_cost: 0,
            magic_cost: 8,
            stamina_cost: 0,
            cooldown: 1,
            description: "Restore a small amount of HP to a single ally.".into(),
            effects: vec![AbilityEffect::Heal { floor: 12, ceiling: 18, scaled_with: Stat::Mind }],
            shape: AbilityShape::Select,
            duration: 0,
            targets: 1,
        });
        r.dirty = true;
    }
}

// ---------------- Helpers ----------------
fn next_free_id(abilities: &Vec<Ability>, level: u8) -> u16 { // I COMPLETELY FORGOT THAT FIRST 8 BITS MUST BE THE LEVEL
    let used: HashSet<u8> = abilities
        .iter()
        .filter(|a| (a.id >> 8) as u8 == level)
        .map(|a| (a.id >> 8) as u8)  // This will always equal `level`
        .collect();
    for i in 0u8..=u8::MAX {
        let candidate = i;
        if !used.contains(&candidate) {
            return ((level as u16) << 8) | (candidate as u16);
        }
    }
    0
}

fn save_abilities_to_path(abilities: &Vec<Ability>, path: &PathBuf) -> Result<(), String> {
    match serde_json::to_string_pretty(abilities) {
        Ok(json) => match fs::write(path, json) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to write file: {}", e)),
        },
        Err(e) => Err(format!("Failed to serialize: {}", e)),
    }
}

fn validate_abilities(abilities: &Vec<Ability>) -> Vec<String> {
    let mut msgs = Vec::new();
    let mut id_set = HashSet::new();
    for a in abilities.iter() {
        if id_set.contains(&a.id) {
            msgs.push(format!("Duplicate ability id: {}", a.id));
        }
        id_set.insert(a.id);
    }

    // check next_id references
    for a in abilities.iter() {
        if let Some(n) = a.next_id {
            if !id_set.contains(&n) {
                msgs.push(format!("Ability {} has invalid next_id {}", a.id, n));
            }
        }
        // Buff effects referencing other ability ids in effects.effects
        for eff in a.effects.iter() {
            if let AbilityEffect::Buff { effects: Some(list), .. } = eff {
                for &eid in list.iter() {
                    if !id_set.contains(&eid) {
                        msgs.push(format!("Ability {} has Buff effect referencing missing ability id {}", a.id, eid));
                    }
                }
            }
        }
    }

    if msgs.is_empty() {
        msgs.push("No issues detected.".into());
    }
    msgs
}

// ---------------- UI ----------------
fn ui_system(mut contexts: EguiContexts, mut r: ResMut<AbilitiesResource>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    // command accumulators
    let mut cmd_new = false;
    let mut cmd_load = false;
    let mut cmd_save = false;
    let mut cmd_validate = false;
    let mut cmd_select: Option<usize> = None;
    let mut cmd_delete: Option<usize> = None;

    // editing accumulators
    let mut edit_id: Option<u16> = None;
    let mut edit_next_id: Option<Option<u16>> = None;
    let mut edit_name: Option<String> = None;
    let mut edit_health: Option<i32> = None;
    let mut edit_magic: Option<i32> = None;
    let mut edit_stamina: Option<i32> = None;
    let mut edit_cooldown: Option<u8> = None;
    let mut edit_description: Option<String> = None;
    let mut edit_duration: Option<u8> = None;
    let mut edit_targets: Option<u8> = None;

    // effect commands
    struct EffCmd { index: usize, replace: Option<AbilityEffect>, delete: bool }
    let mut eff_cmds: Vec<EffCmd> = Vec::new();
    let mut cmd_add_effect: Option<AbilityEffect> = None;

    // shape command
    let mut edit_shape: Option<AbilityShape> = None;

    // Top panel
    egui::TopBottomPanel::top("top").show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button("New Ability").clicked() { cmd_new = true; }
            if ui.button("Load").clicked() { cmd_load = true; }
            if ui.button("Save").clicked() { cmd_save = true; }
            if ui.button("Validate").clicked() { cmd_validate = true; }
            if r.dirty { ui.label("* Unsaved"); } else { ui.label("Saved"); }
            ui.label("Path:");
            let mut path_str = r.file_path.display().to_string();
            if ui.text_edit_singleline(&mut path_str).changed() {
                r.file_path = PathBuf::from(path_str);
            }
        });
    });

    // Left panel - list
    egui::SidePanel::left("left").resizable(true).min_width(220.0).show(ctx, |ui| {
        ui.heading("Abilities"); ui.separator();
        let ids: Vec<u16> = r.abilities.iter().map(|a| a.id).collect();
        for (i, id) in ids.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.button("Edit").clicked() { cmd_select = Some(i); }
                if ui.button("Del").clicked() { cmd_delete = Some(i); }
                if Some(i) == r.selected { ui.label(format!("> {}", id)); } else { ui.label(id.to_string()); }
            }); ui.separator();
        }
    });

    // Central panel - editor
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("Ability Editor"); ui.separator();

        if r.abilities.is_empty() {
            ui.label("No abilities. Create one above.");
            return;
        }

        // show selected
        let Some(sel) = r.selected else { ui.label("Select an ability"); return; };
        if sel >= r.abilities.len() { ui.label("Invalid selection"); return; }

        let ability = &r.abilities[sel];

        // snapshot for dropdowns
        let id_list: Vec<u16> = r.abilities.iter().map(|a| a.id).collect();

        // ID
        let mut tmp_id = ability.id;
        ui.horizontal(|ui| {
            ui.label("ID:");
            if ui.add(egui::DragValue::new(&mut tmp_id)).changed() { edit_id = Some(tmp_id); }
            if ui.button("AutoID").clicked() { edit_id = Some(next_free_id(&r.abilities, 0)); }
        });

        // next_id
        let mut tmp_next = ability.next_id;
        ui.horizontal(|ui| {
            ui.label("Next ID:");
            // show a combobox with None and available ids
            let mut sel_string = tmp_next.map(|v| v.to_string()).unwrap_or_else(|| "<none>".into());
            egui::ComboBox::from_id_source("next_id_combo").selected_text(&sel_string).show_ui(ui, |ui| {
                ui.selectable_value(&mut sel_string, "<none>".into(), "<none>");
                for &id in id_list.iter() { ui.selectable_value(&mut sel_string, id.to_string(), id.to_string()); }
            });
            let new_next = if sel_string == "<none>" { None } else { sel_string.parse::<u16>().ok() };
            if new_next != tmp_next { edit_next_id = Some(new_next); }
        });

        // Name
        let mut tmp_name = ability.name.clone();
        ui.horizontal(|ui| { ui.label("Name:"); if ui.text_edit_singleline(&mut tmp_name).changed() { edit_name = Some(tmp_name.clone()); } });

        // costs and cooldown
        let mut tmp_h = ability.health_cost;
        let mut tmp_m = ability.magic_cost;
        let mut tmp_s = ability.stamina_cost;
        ui.horizontal(|ui| {
            ui.label("Health:"); if ui.add(egui::DragValue::new(&mut tmp_h)).changed() { edit_health = Some(tmp_h); }
            ui.label("Magic:"); if ui.add(egui::DragValue::new(&mut tmp_m)).changed() { edit_magic = Some(tmp_m); }
            ui.label("Stamina:"); if ui.add(egui::DragValue::new(&mut tmp_s)).changed() { edit_stamina = Some(tmp_s); }
        });

        let mut tmp_cd = ability.cooldown;
        if ui.add(egui::DragValue::new(&mut tmp_cd)).changed() { edit_cooldown = Some(tmp_cd); }

        // description
        let mut tmp_desc = ability.description.clone(); ui.label("Description:"); if ui.text_edit_multiline(&mut tmp_desc).changed() { edit_description = Some(tmp_desc.clone()); }

        // duration & targets
        let mut tmp_dur = ability.duration; let mut tmp_targs = ability.targets;
        ui.horizontal(|ui| { ui.label("Duration:"); if ui.add(egui::DragValue::new(&mut tmp_dur)).changed() { edit_duration = Some(tmp_dur); } ui.label("Targets:"); if ui.add(egui::DragValue::new(&mut tmp_targs)).changed() { edit_targets = Some(tmp_targs); } });

        // Shape editor
        ui.separator(); ui.label("Shape:");
        match &ability.shape {
            AbilityShape::Radius(v) => { let mut val = *v; if ui.add(egui::DragValue::new(&mut val)).changed() { edit_shape = Some(AbilityShape::Radius(val)); } }
            AbilityShape::Line { length, thickness } => { let mut l = *length; let mut t = *thickness; ui.horizontal(|ui| { ui.label("Length:"); if ui.add(egui::DragValue::new(&mut l)).changed() { edit_shape = Some(AbilityShape::Line { length: l, thickness: t }); } ui.label("Thickness:"); if ui.add(egui::DragValue::new(&mut t)).changed() { edit_shape = Some(AbilityShape::Line { length: l, thickness: t }); } }); }
            AbilityShape::Cone { angle, radius } => { let mut a = *angle; let mut rrad = *radius; ui.horizontal(|ui| { ui.label("Angle:"); if ui.add(egui::DragValue::new(&mut a)).changed() { edit_shape = Some(AbilityShape::Cone { angle: a, radius: rrad }); } ui.label("Radius:"); if ui.add(egui::DragValue::new(&mut rrad)).changed() { edit_shape = Some(AbilityShape::Cone { angle: a, radius: rrad }); } }); }
            AbilityShape::Select => { if ui.button("Set Radius 1.0").clicked() { edit_shape = Some(AbilityShape::Radius(1.0)); } }
        }

        // Effects
        ui.separator(); ui.label("Effects:");
        ui.horizontal(|ui| {
            if ui.button("Add Heal").clicked() { cmd_add_effect = Some(AbilityEffect::Heal { floor: 1, ceiling: 2, scaled_with: Stat::Mind }); }
            if ui.button("Add Damage").clicked() { cmd_add_effect = Some(AbilityEffect::Damage { floor: 1, ceiling: 2, damage_type: DamageType::Physical, scaled_with: Stat::Mind, defended_with: Stat::Mind }); }
            if ui.button("Add Buff").clicked() { cmd_add_effect = Some(AbilityEffect::Buff { stat: Stat::Morale, multiplier: 1.0, effects: None, scaled_with: Stat::Mind }); }
        });

        for (i, eff) in ability.effects.iter().enumerate() {
            ui.separator(); ui.label(format!("Effect {}:", i));
            match eff {
                AbilityEffect::Heal { floor, ceiling, scaled_with } => {
                    let mut f = *floor; let mut c = *ceiling; ui.horizontal(|ui| { ui.label("Floor:"); if ui.add(egui::DragValue::new(&mut f)).changed() { eff_cmds.push(EffCmd { index: i, replace: Some(AbilityEffect::Heal { floor: f, ceiling: c, scaled_with: scaled_with.clone() }), delete: false }); } ui.label("Ceiling:"); if ui.add(egui::DragValue::new(&mut c)).changed() { eff_cmds.push(EffCmd { index: i, replace: Some(AbilityEffect::Heal { floor: f, ceiling: c, scaled_with: scaled_with.clone() }), delete: false }); } });
                    // scaled_with
                    let mut sw = format!("{:?}", scaled_with);
                    ui.horizontal(|ui| { ui.label("Scaled with:"); if ui.text_edit_singleline(&mut sw).changed() { /* user can type stat name; parse later on apply */ } });
                }
                AbilityEffect::Damage { floor, ceiling, damage_type, scaled_with, defended_with } => {
                    let mut f = *floor; let mut c = *ceiling; ui.horizontal(|ui| { ui.label("Floor:"); if ui.add(egui::DragValue::new(&mut f)).changed() { eff_cmds.push(EffCmd { index: i, replace: Some(AbilityEffect::Damage { floor: f, ceiling: c, damage_type: damage_type.clone(), scaled_with: scaled_with.clone(), defended_with: defended_with.clone() }), delete: false }); } ui.label("Ceiling:"); if ui.add(egui::DragValue::new(&mut c)).changed() { eff_cmds.push(EffCmd { index: i, replace: Some(AbilityEffect::Damage { floor: f, ceiling: c, damage_type: damage_type.clone(), scaled_with: scaled_with.clone(), defended_with: defended_with.clone() }), delete: false }); } });
                    // damage type selector
                    // small string representation for now
                    let mut dt = format!("{:?}", damage_type);
                    ui.horizontal(|ui| { ui.label("Damage Type:"); if ui.text_edit_singleline(&mut dt).changed() { /* parse later */ } });
                }
                AbilityEffect::Buff { stat, multiplier, effects, scaled_with } => {
                    let mut mul = *multiplier; ui.horizontal(|ui| { ui.label("Multiplier:"); if ui.add(egui::DragValue::new(&mut mul)).changed() { eff_cmds.push(EffCmd { index: i, replace: Some(AbilityEffect::Buff { stat: stat.clone(), multiplier: mul, effects: effects.clone(), scaled_with: scaled_with.clone() }), delete: false }); } });
                    if let Some(list) = effects {
                        ui.label(format!("Triggers: {:?}", list));
                    }
                }
            }
            if ui.button("Remove Effect").clicked() { eff_cmds.push(EffCmd { index: i, replace: None, delete: true }); }
        }

        // validation messages
        if !r.validation_messages.is_empty() {
            ui.separator(); ui.heading("Validation Messages:"); for msg in &r.validation_messages { ui.label(msg); }
        }
    }); // central panel

    // ---------------- Apply commands (outside closures) ----------------
    if cmd_new {
        let mut a = Ability::default();
        a.id = next_free_id(&r.abilities, 0);
        r.abilities.push(a);
        r.selected = Some(r.abilities.len() - 1);
        r.dirty = true;
    }

    if cmd_load {
        match fs::read_to_string(&r.file_path) {
            Ok(content) => match serde_json::from_str::<Vec<Ability>>(&content) {
                Ok(v) => { r.abilities = v; r.validation_messages.clear(); r.selected = None; r.dirty = false; }
                Err(e) => { r.validation_messages = vec![format!("Failed to parse JSON: {}", e)]; }
            },
            Err(e) => { r.validation_messages = vec![format!("Failed to read {}: {}", r.file_path.display(), e)]; }
        }
    }

    if cmd_save {
        if let Err(e) = save_abilities_to_path(&r.abilities, &r.file_path) {
            r.validation_messages.push(format!("Failed to save: {}", e));
        } else { r.dirty = false; }
    }

    if cmd_validate { r.validation_messages = validate_abilities(&r.abilities); }

    if let Some(i) = cmd_select { if i < r.abilities.len() { r.selected = Some(i); } }

    if let Some(i) = cmd_delete { if i < r.abilities.len() { r.abilities.remove(i); r.dirty = true; if let Some(sel) = r.selected { if sel == i { r.selected = None; } else if sel > i { r.selected = Some(sel - 1); } } } }

    // apply edits to selected ability
    if let Some(sel) = r.selected {
        if sel < r.abilities.len() {
            let mut dirty = false;
            {
                let a = &mut r.abilities[sel];
                if let Some(id) = edit_id { a.id = id; dirty = true; }
                if let Some(nid) = edit_next_id { a.next_id = nid; dirty = true; }
                if let Some(name) = edit_name { a.name = name; dirty = true; }
                if let Some(h) = edit_health { a.health_cost = h; dirty = true; }
                if let Some(m) = edit_magic { a.magic_cost = m; dirty = true; }
                if let Some(s) = edit_stamina { a.stamina_cost = s; dirty = true; }
                if let Some(cd) = edit_cooldown { a.cooldown = cd; dirty = true; }
                if let Some(desc) = edit_description { a.description = desc; dirty = true; }
                if let Some(dur) = edit_duration { a.duration = dur; dirty = true; }
                if let Some(t) = edit_targets { a.targets = t; dirty = true; }
                if let Some(shape) = edit_shape { a.shape = shape; dirty = true; }

                if let Some(eff) = cmd_add_effect { a.effects.push(eff); dirty = true; }

                if !eff_cmds.is_empty() {
                    // process deletions first
                    let mut deletes: Vec<usize> = eff_cmds.iter().filter(|c| c.delete).map(|c| c.index).collect();
                    deletes.sort_unstable(); deletes.dedup();
                    for idx in deletes.into_iter().rev() { if idx < a.effects.len() { a.effects.remove(idx); dirty = true; } }

                    // then replacements
                    for cmd in eff_cmds.into_iter() {
                        if !cmd.delete && cmd.index < a.effects.len() {
                            if let Some(rep) = cmd.replace { a.effects[cmd.index] = rep; dirty = true; }
                        }
                    }
                }
            }
            if dirty { r.dirty = true; }
        }
    }
}
