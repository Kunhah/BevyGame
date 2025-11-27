// bevy_egui_dialogue_editor.rs
// A simple dialogue editor using Bevy + bevy_egui + serde_json
// Features:
// - Load / Save dialogues.json from ./dialogues/dialogues.json
// - List dialogues, add/remove
// - Edit id, speaker, text
// - Set `next` via dropdown (auto-populated)
// - Edit choices: add/remove, edit event/text/next
// - Basic validation: duplicate IDs, dangling references, orphans

// Cargo.toml dependencies (add to your Cargo.toml):
// [dependencies]
// bevy = { version = "0.11" }
// bevy_egui = "0.22"
// serde = { version = "1.0", features = ["derive"] }
// serde_json = "1.0"
// (rfd removed for simplicity; the editor will read/write a fixed path ./dialogues/dialogues.json)

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

const DEFAULT_DIALOGUE_DIR: &str = "dialogues";
const DEFAULT_DIALOGUE_FILE: &str = "dialogues.json";

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Dialogue {
    pub id: String,
    pub speaker: String,
    pub text: String,
    pub next: Option<String>,
    pub choices: Option<Vec<DialogueChoice>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DialogueChoice {
    pub event: u32,
    pub text: String,
    pub next: String,
}

#[derive(Resource)]
struct DialoguesResource {
    dialogues: Vec<Dialogue>,
    file_path: PathBuf,
    dirty: bool,
    validation_messages: Vec<String>,
}

impl Default for DialoguesResource {
    fn default() -> Self {
        let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        path.push(DEFAULT_DIALOGUE_DIR);
        path.push(DEFAULT_DIALOGUE_FILE);
        Self {
            dialogues: Vec::new(),
            file_path: path,
            dirty: false,
            validation_messages: Vec::new(),
        }
    }
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EguiPlugin)
        .init_resource::<DialoguesResource>()
        .add_systems(Startup, setup_system)
        .add_systems(Update, ui_system)
        .run();
}

fn setup_system(mut d: ResMut<DialoguesResource>) {
    // Start with a minimal example if empty
    if d.dialogues.is_empty() {
        d.dialogues = vec![Dialogue {
            id: "Intro_1".to_string(),
            speaker: "Narrator".to_string(),
            text: "Welcome to the demo.".to_string(),
            next: None,
            choices: None,
        }];
        d.dirty = true;
    }
}

fn ui_system(mut contexts: EguiContexts, mut d: ResMut<DialoguesResource>) {
    let ctx = contexts.ctx_mut().unwrap();

    // --- COMMAND ACCUMULATORS (to be applied later) ---
    let mut cmd_new_dialogue = false;
    let mut cmd_load = false;
    let mut cmd_save = false;
    let mut cmd_validate = false;

    let mut cmd_select: Option<usize> = None;
    let mut cmd_delete: Option<usize> = None;

    // For editing a single dialogue
    let mut edit_id: Option<String> = None;
    let mut edit_speaker: Option<String> = None;
    let mut edit_text: Option<String> = None;
    let mut edit_next: Option<Option<String>> = None;

    // For modifying choices
    struct ChoiceCmd {
        index: usize,
        new_text: Option<String>,
        new_event: Option<u32>,
        new_next: Option<String>,
        delete: bool,
    }
    let mut choice_cmds: Vec<ChoiceCmd> = Vec::new();
    let mut cmd_add_choice = false;


    // ================================================================
    // TOP PANEL
    // ================================================================
    egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button("New Dialogue").clicked() {
                cmd_new_dialogue = true;
            }

            if ui.button("Load").clicked() {
                cmd_load = true;
            }

            if ui.button("Save").clicked() {
                cmd_save = true;
            }

            if ui.button("Validate").clicked() {
                cmd_validate = true;
            }

            if d.dirty {
                ui.label("* Unsaved changes");
            } else {
                ui.label("Saved");
            }

            ui.label(format!("Path: {}", d.file_path.display()));
        });
    });


    // ================================================================
    // LEFT PANEL (LIST + SELECTION)
    // ================================================================
    egui::SidePanel::left("left_panel").resizable(true).min_width(220.0).show(ctx, |ui| {
        ui.heading("Dialogues");
        ui.separator();

        let ids: Vec<String> = d.dialogues.iter().map(|dlg| dlg.id.clone()).collect();

        for (i, id) in ids.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.button("Edit").clicked() {
                    cmd_select = Some(i);
                }
                if ui.button("Del").clicked() {
                    cmd_delete = Some(i);
                }

                if Some(i) == d.selected {
                    ui.label(format!("> {}", id));
                } else {
                    ui.label(id);
                }
            });
            ui.separator();
        }
    });


    // ================================================================
    // CENTRAL PANEL (EDITOR)
    // ================================================================
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("Editor");
        ui.separator();

        let Some(selected) = d.selected else {
            if d.dialogues.is_empty() {
                ui.label("No dialogues. Create one from the top bar.");
            } else {
                ui.label("Select a dialogue to edit.");
            }
            return;
        };

        if selected >= d.dialogues.len() {
            ui.label("Invalid dialogue index.");
            return;
        }

        let dlg = &d.dialogues[selected];

        // Snapshot ids
        let id_list: Vec<String> = d.dialogues.iter().map(|x| x.id.clone()).collect();

        // ---- ID + Duplicate ----
        let mut temp_id = dlg.id.clone();
        ui.horizontal(|ui| {
            ui.label("ID:");
            if ui.text_edit_singleline(&mut temp_id).changed() {
                edit_id = Some(temp_id.clone());
            }
            if ui.button("Duplicate").clicked() {
                cmd_new_dialogue = true; // handled below with special logic
                                         // The actual duplicate will be appended after apply
            }
        });

        // ---- Speaker ----
        let mut temp_speaker = dlg.speaker.clone();
        ui.horizontal(|ui| {
            ui.label("Speaker:");
            if ui.text_edit_singleline(&mut temp_speaker).changed() {
                edit_speaker = Some(temp_speaker.clone());
            }
        });

        // ---- Text ----
        let mut temp_text = dlg.text.clone();
        ui.label("Text:");
        if ui.text_edit_multiline(&mut temp_text).changed() {
            edit_text = Some(temp_text.clone());
        }

        // ---- Next ----
        let mut current_next = dlg.next.clone().unwrap_or_else(|| "<none>".into());
        ui.horizontal(|ui| {
            ui.label("Next:");
            egui::ComboBox::from_id_salt("next_combo")
                .selected_text(&current_next)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut current_next, "<none>".into(), "<none>");
                    for id in id_list.iter() {
                        ui.selectable_value(&mut current_next, id.clone(), id);
                    }
                });
        });

        let next_to_apply =
            if current_next == "<none>" { None } else { Some(current_next.clone()) };
        if next_to_apply != dlg.next {
            edit_next = Some(next_to_apply.clone());
        }

        // ---- Choices ----
        ui.separator();
        ui.label("Choices:");
        if ui.button("Add Choice").clicked() {
            cmd_add_choice = true;
        }

        if let Some(choices) = &dlg.choices {
            for (i, choice) in choices.iter().enumerate() {
                ui.separator();
                ui.label(format!("Choice {}:", i));

                let mut new_text = choice.text.clone();
                let mut new_event = choice.event;
                let mut new_next = choice.next.clone();
                let mut delete = false;

                ui.horizontal(|ui| {
                    ui.label("Text:");
                    if ui.text_edit_singleline(&mut new_text).changed() {
                        choice_cmds.push(ChoiceCmd {
                            index: i,
                            new_text: Some(new_text.clone()),
                            new_event: None,
                            new_next: None,
                            delete: false,
                        });
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Event:");
                    if ui.add(egui::DragValue::new(&mut new_event)).changed() {
                        choice_cmds.push(ChoiceCmd {
                            index: i,
                            new_text: None,
                            new_event: Some(new_event),
                            new_next: None,
                            delete: false,
                        });
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Next:");
                    egui::ComboBox::from_id_salt(format!("choice_next_{}", i))
                        .selected_text(&new_next)
                        .show_ui(ui, |ui| {
                            for id in id_list.iter() {
                                ui.selectable_value(&mut new_next, id.clone(), id);
                            }
                        });

                    if new_next != choice.next {
                        choice_cmds.push(ChoiceCmd {
                            index: i,
                            new_text: None,
                            new_event: None,
                            new_next: Some(new_next.clone()),
                            delete: false,
                        });
                    }

                    if ui.button("Del").clicked() {
                        delete = true;
                    }
                });

                if delete {
                    choice_cmds.push(ChoiceCmd {
                        index: i,
                        new_text: None,
                        new_event: None,
                        new_next: None,
                        delete: true,
                    });
                }
            }
        }

        // --- Validation messages ---
        if !d.validation_messages.is_empty() {
            ui.separator();
            ui.heading("Validation Messages:");
            for msg in &d.validation_messages {
                ui.label(msg);
            }
        }
    });


    // ================================================================
    // APPLY COMMANDS (safe — outside closures)
    // ================================================================

    // New dialogue
    if cmd_new_dialogue {
        let new_id = unique_id(&d.dialogues, "Dlg_", "Seq_");
        d.dialogues.push(Dialogue {
            id: new_id,
            speaker: "".into(),
            text: "".into(),
            next: None,
            choices: None,
        });
        d.selected = Some(d.dialogues.len() - 1);
        d.dirty = true;
    }

    // Load
    if cmd_load {
        match fs::read_to_string(&d.file_path) {
            Ok(content) => match serde_json::from_str::<Vec<Dialogue>>(&content) {
                Ok(v) => {
                    d.dialogues = v;
                    d.validation_messages.clear();
                    d.selected = None;
                    d.dirty = false;
                }
                Err(e) => {
                    d.validation_messages = vec![format!("Failed to parse JSON: {}", e)];
                }
            },
            Err(e) => {
                d.validation_messages =
                    vec![format!("Failed to read {}: {}", d.file_path.display(), e)];
            }
        }
    }

    // Save
    if cmd_save {
        if let Err(e) = save_dialogues_to_path(&d.dialogues, &d.file_path) {
            d.validation_messages.push(format!("Failed to save: {}", e));
        } else {
            d.dirty = false;
        }
    }

    // Validate
    if cmd_validate {
        d.validation_messages = validate_dialogues(&d.dialogues);
    }

    // Selection
    if let Some(i) = cmd_select {
        if i < d.dialogues.len() {
            d.selected = Some(i);
        }
    }

    // Delete
    if let Some(i) = cmd_delete {
        if i < d.dialogues.len() {
            d.dialogues.remove(i);
            d.dirty = true;

            if let Some(sel) = d.selected {
                if sel == i {
                    d.selected = None;
                } else if sel > i {
                    d.selected = Some(sel - 1);
                }
            }
        }
    }

    // Apply main field changes
    if let Some(sel) = d.selected {
        if sel < d.dialogues.len() {
            let dlg = &mut d.dialogues[sel];

            if let Some(id) = edit_id {
                dlg.id = id;
                d.dirty = true;
            }
            if let Some(s) = edit_speaker {
                dlg.speaker = s;
                d.dirty = true;
            }
            if let Some(t) = edit_text {
                dlg.text = t;
                d.dirty = true;
            }
            if let Some(n) = edit_next {
                dlg.next = n;
                d.dirty = true;
            }

            // Add choice
            if cmd_add_choice {
                let new_choice = DialogueChoice {
                    event: 0,
                    text: "New choice".into(),
                    next: unique_id(&d.dialogues, "Dlg_", "Seq_"),
                };
                dlg.choices.get_or_insert(Vec::new()).push(new_choice);
                d.dirty = true;
            }

            // Apply choice edits
            if let Some(choices) = &mut dlg.choices {
                // Process deletions first
                choice_cmds.sort_by_key(|c| (c.delete, c.index));

                let mut to_delete: Vec<usize> = Vec::new();

                for cmd in &choice_cmds {
                    if cmd.delete {
                        to_delete.push(cmd.index);
                    }
                }

                // Delete in reverse order
                for &idx in to_delete.iter().rev() {
                    if idx < choices.len() {
                        choices.remove(idx);
                        d.dirty = true;
                    }
                }

                // Now apply edits
                for cmd in choice_cmds {
                    if cmd.index < choices.len() {
                        let c = &mut choices[cmd.index];
                        if let Some(t) = cmd.new_text {
                            c.text = t;
                            d.dirty = true;
                        }
                        if let Some(e) = cmd.new_event {
                            c.event = e;
                            d.dirty = true;
                        }
                        if let Some(n) = cmd.new_next {
                            c.next = n;
                            d.dirty = true;
                        }
                    }
                }
            }
        }
    }
}

fn unique_id(dialogues: &Vec<Dialogue>, prefix: &str, second_prefix: &str) -> String {
    let mut i = 1;
    let existing: HashSet<String> = dialogues.iter().map(|d| d.id.clone()).collect();
    loop {
        let candidate = format!("{}{}{}", prefix, second_prefix, i);
        if !existing.contains(&candidate) {
            return candidate;
        }
        i += 1;
    }
}

// fn fix_duplicate_ids(dialogues: &mut Vec<Dialogue>) {
//     let mut used_ids = HashSet::new();
    
//     for dlg in dialogues.iter_mut() {
//         // Handle empty IDs
//         if dlg.id.trim().is_empty() {
//             let mut i = 1;
//             loop {
//                 let candidate = format!("Dialogue_{}", i);
//                 if !used_ids.contains(&candidate) {
//                     dlg.id = candidate.clone();
//                     used_ids.insert(candidate);
//                     break;
//                 }
//                 i += 1;
//             }
//             continue;
//         }
        
//         // Handle duplicate IDs
//         if used_ids.contains(&dlg.id) {
//             let base = dlg.id.clone();
//             let mut suffix = 1;
//             loop {
//                 let candidate = format!("{}_{}", base, suffix);
//                 if !used_ids.contains(&candidate) {
//                     dlg.id = candidate.clone();
//                     used_ids.insert(candidate);
//                     break;
//                 }
//                 suffix += 1;
//             }
//         } else {
//             used_ids.insert(dlg.id.clone());
//         }
//     }
// }

fn save_dialogues_to_path(dialogues: &Vec<Dialogue>, path: &PathBuf) -> Result<(), String> {
    match serde_json::to_string_pretty(dialogues) {
        Ok(json) => match fs::write(path, json) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to write file: {}", e)),
        },
        Err(e) => Err(format!("Failed to serialize: {}", e)),
    }
}

fn validate_dialogues(dialogues: &Vec<Dialogue>) -> Vec<String> {
    let mut msgs = Vec::new();
    let mut id_set = HashSet::new();
    for d in dialogues.iter() {
        if d.id.trim().is_empty() {
            msgs.push("Empty ID found".into());
        }
        if id_set.contains(&d.id) {
            msgs.push(format!("Duplicate ID: {}", d.id));
        }
        id_set.insert(d.id.clone());
    }

    // check references
    for d in dialogues.iter() {
        if let Some(n) = &d.next {
            if !id_set.contains(n) {
                msgs.push(format!("Dangling next reference from '{}' -> '{}'", d.id, n));
            }
        }
        if let Some(choices) = &d.choices {
            for c in choices.iter() {
                if !id_set.contains(&c.next) {
                    msgs.push(format!("Dangling choice reference from '{}' -> '{}'", d.id, c.next));
                }
            }
        }
    }

    // find orphans (not referenced by anyone and not the first entry)
    let mut referenced = HashSet::new();
    if !dialogues.is_empty() {
        // treat first as reachable entry point
        referenced.insert(dialogues[0].id.clone());
    }
    for d in dialogues.iter() {
        if let Some(n) = &d.next {
            referenced.insert(n.clone());
        }
        if let Some(choices) = &d.choices {
            for c in choices.iter() {
                referenced.insert(c.next.clone());
            }
        }
    }
    for d in dialogues.iter() {
        if !referenced.contains(&d.id) {
            msgs.push(format!("Orphan dialogue (not referenced): {}", d.id));
        }
    }

    msgs
}
