use bevy::ecs::system::command::{insert_batch, insert_resource};
use bevy::prelude::*;
use std::fmt::Debug;
use std::f32::consts::PI;
use serde::{Serialize, Deserialize};
use std::cmp::Ordering;
use std::sync::Arc;
use std::sync::RwLock;
use std::fs;
use std::f64::log

//
// === Base Character Component ===
//
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Character {
    pub id: u32,
    pub name: String,
    pub class: String,
    pub health: u32,
    pub max_health: u32,
    pub health_regen: u32,
    pub magic: u32,
    pub max_magic: u32,
    pub magic_regen: u32,
    pub stamina: u32,
    pub max_stamina: u32,
    pub stamina_regen: u32,
    pub buffs: Vec<Buff>,
    pub movement: u32,
    pub base_lethality: u32,
    pub hit: u32,
    pub armor: u32,
    pub agility: u32,
    pub mind: u32,
    pub max_morale: u32,
    pub morale: u32,
    pub marale_regen: u32,
    pub accumulated_agility: u32,
    pub damage_multipliers: Vec<DamageType, f32>,
    pub before_attack: Option<Vec<u16>>,
    pub after_attack: Option<Vec<u16>>,
    pub before_attacked: Option<Vec<u16>>,
    pub after_attacked: Option<Vec<u16>>,
    pub abilities: Vec<u16>,
    pub equipments: Vec<Equipment>.
    pub ai_parameters: Option<AIParameters>,
}

pub struct Buff {
    pub stat: Stat,
    pub multiplier: f32, // debuffs will be negative
    pub ends_in: u32, // every unit of duration will be equivalent of 4 seconds, that means everytime a the function to get the turn order is called, 1 << 2 seconds passes and this will be decremented by 1
    pub effects: Option<Vec<AbilityEffect>>,
}

pub struct EquipBuff {
    pub stats: Stat,
    pub multiplier: f32,
    pub effects: Option<AbilityEffect>,
    pub before_attack: Option<u16>,
    pub after_attack: Option<u16>,
    pub before_attacked: Option<u16>,
    pub after_attacked: Option<u16>,
}

pub struct Equipment {
    pub id: u32,
    pub equipment_type: EquipmentType,
    pub name: String,
    pub lethality: u32,
    pub hit: u32,
    pub armor: u32,
    pub agility: u32,
    pub mind: u32,
    pub morale: u32,
    pub buffs: Vec<EquipBuff>,
}

pub enum EquipmentType {
    Weapon{WeaponType},
    Armor{ArmorType},
    Accessory{AccessoryType},
}

pub enum WeaponType {
    Sword,
    Axe,
    Mace,
    TwoHSword,
    TwoHAxe,
    TwoHMace,
    Dagger,
    Staff,
    Wand,
    Bow,
    Sling,    
}

pub enum ArmorType {
    Light,
    Medium,
    Heavy,
}

pub enum AccessoryType {
    Ring,
    Amulet,
    Belt,
    Earring,
    Bracelet,
    Cloak,
}

//
// === AI PARAMETERS ===
//
#[derive(Clone, Debug)]
pub struct AIParameters {
    pub aggressiveness:  u8,
    pub caution:  u8,
    pub curiosity:  u8,
    pub perception:  u8,
    pub bravery:  u8,
    pub patience:  u8,
}

impl Default for AIParameters {
    fn default() -> Self {
        Self {     
            aggressiveness: 5,
            caution: 5,
            curiosity: 5,
            perception: 5,
            bravery: 5,
            patience: 5,
        }
    }
}

 pub enum Stat {
    Health,
    HealthRegen,
    Magic,
    MagicRegen,
    Stamina,
    StaminaRegen,
    Lethality,
    Hit,
    Agility,
    Mind,
    Morale,
}

//
// === BASE CHARACTER TRAIT ===
//
pub trait CharacterBase: Send + Sync + Debug {
    fn id(&self) -> u32;
    fn name(&self) -> &str;
    fn health(&self) -> u32;
    fn max_health(&self) -> u32;
    fn agility(&self) -> u32;
    fn accumulated_agility(&self) ->  u32;
    fn set_accumulated_agility(&mut self, value:  u32);
    fn ai(&self) -> Option<&AIParameters>;
    fn character_type(&self) -> &str;

    fn attack(&mut self, target: &mut dyn CharacterBase);

    fn heal(&mut self, amount: u32);
    fn damage(&mut self, amount: u32, damage_type: DamageType);

    fn buff(&mut self, stat: Stat, multiplier: f32, now: u32, duration: u32, effects: Option<Vec<u16>>);
    // To make sure buffs and debuffs doesn't cause a permanent change in the character, everytime a battle ends and everytime a permanent change needs to occur in the character, the character will be loaded from storage
    
    fn remove_buff(&mut self, now: u32);

    // Optional: characters can override this for class-specific effects
    fn attack_power(&self) -> u32 {
        10
    }
}

fn add_with_max(a: u32, b: u32, max_value: u32) -> Option<u32> {
    a.checked_add(b).map(|result| result.min(max_value))
}

//
// === CHARACTER COMPONENT WRAPPER ===
//
#[derive(Component)]
pub struct CharacterComponent {
    pub character: Box<dyn CharacterBase>,
}

//
// === CONCRETE CHARACTERS ===
//

// -- Toshiko (Warlock) --
#[derive(Clone, Debug)]
pub struct Toshiko {
    pub base_character: Character,
    pub experience: u32, 
    pub dark: u32,
}

impl Toshiko {
    pub fn new(id: u32) -> Self {
        Self {
            base_character: Character {
                id,
                name: "Toshiko".to_string(),
                class: "Warlock".to_string(),
                health: 100,
                max_health: 100,
                magic: 150,
                max_magic: 150,
                stamina: 70,
                max_stamina: 70,
                buffs: vec![],
                movement: 4,
                base_lethality: 12,
                hit: 85,
                agility: 9,
                mind: 16,
                morale: 90,
                accumulated_agility: 0,
                damage_multipliers: vec![(DamageType::Physical, 1.0)],
                abilities: vec![],
                ai_parameters: Some(AIParameters {
                    aggressiveness: 7,
                    caution: 4,
                    curiosity: 5,
                    perception: 6,
                    bravery: 8,
                    patience: 3,
                }),
            },
            level: 1,
            dark: 25,
            equippments: vec![],
            equipped_lethality: 0,
            equipped_hit: 0,
            equipped_armor: 0,
            equipped_agility: 0,
            equipped_mind: 0,
            equipped_morale: 0,

        }
    }

    pub fn equip(&mut self, equipment_id: u32) {
        let equipment: Equipment = get_equipment_by_id(equipment_id);

        if !(equipment.equipment_type == EquipmentType::Accessory && equipment.equipment_type.accessory_type != AccessoryType::Belt) {
            return;
        }
        
        self.equipments.push(equipment);
        self.equipped_lethality += equipment.lethality;
        self.equipped_hit += equipment.hit;
        self.equipped_armor += equipment.armor;
        self.equipped_agility += equipment.agility;
        self.equipped_mind += equipment.mind;
        self.equipped_morale += equipment.morale;
    }

    pub fn unequip(&mut self, equipment_id: u32) {
        for i in 1..self.equipments.length() {
            if self.equipments[i].id == equipment_id {
                self.equipments[i] == self.equipments[self.equipments.length() - 1];
                self.equipments.pop();
                self.equipped_lethality -= equipment.lethality;
                self.equipped_hit -= equipment.hit;
                self.equipped_armor -= equipment.armor;
                self.equipped_agility -= equipment.agility;
                self.equipped_mind -= equipment.mind;
                self.equipped_morale -= equipment.morale;
                break;
            }
        }
    }

    pub fn get_level(&self) -> u8 { 
        self.experience >> 16
    }

    pub fn calculate_xp(&mut self, enemy_experience: u32) {  // for every level it is necessary 65_536 xp 2**16
        let ratio: f32 = enemy_experience as f32 / self.experience as f32;
        let amount: f32;
        
        if ratio > 0.946 {
            // Using log with base 1.25 - Rust doesn't have direct base-n log, so we use change of base
            amount = ((ratio - 0.2).ln() / 1.25f32.ln() + 1.5) << 14 // * 16_384; 2**14
        } else {
            amount = ratio.powf(30.2) << 14 // * 16_384; 2**14
        }
        
        self.experience += amount as u32;
    }

    pub fn level_stamina(&self) -> u32 {
        self.max_stamina += 500 - (self.max_stamina.powf(1.955) >> 19); // 2**19 = 524_288
        self.stamina_regen += 50 - (self.max_stamina_regen.powf(2.05) >> 19);
    }

    pub fn level_health(&self) -> u32 {
        self.max_health += 500 - (self.max_health.powf(1.955) >> 19); // 2**19 = 524_288
        self.health_regen += 50 - (self.max_health_regen.powf(2.21) >> 19);
    }

    pub fn level_agility(&self) -> u32 {
        self.agility += 500 - (self.max_agility.powf(1.955) >> 19); // 2**19 = 524_288
        self.movement += 500 - (self.max_movement.powf(1.955) >> 19);
    }

    pub fn level_lethality(&self) -> u32 {
        self.base_lethality += 500 - (self.max_lethality.powf(1.955) >> 19); // 2**19 = 524_288
        self.hit += 500 - (self.max_hit.powf(1.955) >> 19);
    }

    pub fn level_magic(&self) -> u32 {
        self.max_magic += 500 - (self.max_magic.powf(1.955) >> 19); // 2**19 = 524_288
        self.magic_regen += 50 - (self.max_magic_regen.powf(2.21) >> 19);
    }

    pub fn level_mind(&self) -> u32 {
        self.mind += 500 - (self.max_mind.powf(1.955) >> 19); // 2**19 = 524_288
        self.max_morale += 500 - (self.max_morale.powf(1.955) >> 19); // 2**19 = 524_288
        self.morale_regen += 50 - (self.max_morale_regen.powf(2.21) >> 19);
    }
    
}

impl CharacterBase for Toshiko {
    fn id(&self) -> u32 { self.base_character.id }
    fn name(&self) -> &str { &self.base_character.name }
    fn health(&self) -> u32 { self.base_character.health }
    fn max_health(&self) -> u32 { self.base_character.max_health }
    fn agility(&self) -> u32 { self.base_character.agility }
    fn accumulated_agility(&self) ->  u32 { self.base_character.accumulated_agility }
    fn set_accumulated_agility(&mut self, value:  u32) { self.base_character.accumulated_agility = value; }
    fn ai(&self) -> Option<&AIParameters> { self.base_character.ai_parameters.as_ref() }
    fn character_type(&self) -> &str { "Warlock" }

    fn attack(&mut self, target: &mut CharacterBase) {
        self.base_character.before_attack();
        for equipment in self.equipments.iter() {
            for buff in equipment.buffs.iter() {
                buff.before_attack();
            }
        }



        self.base_character.after_attack();
        for equipment in self.equipments.iter() {
            for buff in equipment.buffs.iter() {
                buff.after_attack();
            }
        }
    }

    fn heal(&mut self, amount: u32) {
        self.base_character.health += amount;
    }

    fn damage(&mut self, amount: u32, damage_type: DamageType) {

        self.base_character.before_attacked();
        for equipment in self.equipments.iter() {
            for buff in equipment.buffs.iter() {
                buff.before_attacked();
            }
        }

        let mut total_damage = amount;
        for multiplier in &self.base_character.damage_multipliers {
            if multiplier.0 == damage_type {
                total_damage = (total_damage as f32 * multiplier.1) as u32;
                break;
            }
        }
        if self.dark > 0 {
            if self.dark >= total_damage {
                self.dark -= total_damage;
                return;
            } else {
                total_damage.saturating_sub(self.dark);
                self.dark = 0;
                self.base_character.health = self.base_character.health.saturating_sub(total_damage);
            }
        }

        self.base_character.after_attacked();
        for equipment in self.equipments.iter() {
            for buff in equipment.buffs.iter() {
                buff.after_attacked();
            }
        }
    }

    fn buff(&mut self, stat: Stat, multiplier: f32, now: u32, duration: u32, effects: Option<Vec<u16>>) {
        self.base_character.buffs.push(
            Buff {
                stat, 
                multiplier, 
                ends_in: now + duration, 
                effects
            } 
        );
        match stat {
            Stat::Health => self.base_character.max_health = (self.base_character.max_health as f32 * multiplier).round() as u32,
            Stat::HealthRegen => self.base_character.max_health_regen = (self.base_character.max_health_regen as f32 * multiplier).round() as u32,
            Stat::Magic => self.base_character.max_magic = (self.base_character.max_magic as f32 * multiplier).round() as u32,
            Stat::MagicRegen => self.base_character.max_magic_regen = (self.base_character.max_magic_regen as f32 * multiplier).round() as u32,
            Stat::Stamina => self.base_character.max_stamina = (self.base_character.max_stamina as f32 * multiplier).round() as u32,
            Stat::StaminaRegen => self.base_character.max_stamina_regen = (self.base_character.max_stamina_regen as f32 * multiplier).round() as u32,
            Stat::Lethality => self.base_character.base_lethality = (self.base_character.base_lethality as f32 * multiplier).round() as u32,
            Stat::Hit => self.base_character.hit = (self.base_character.hit as f32 * multiplier).round() as u32,
            Stat::Agility => self.base_character.accumulated_agility = (self.base_character.accumulated_agility as f32 * multiplier).round() as u32,
            Stat::Mind => self.base_character.mind = (self.base_character.mind as f32 * multiplier).round() as u32,
            Stat::Morale => self.base_character.morale = (self.base_character.morale as f32 * multiplier) .round()as u32,
        }
        if effects.is_some() {
            for effect in effects.unwrap() {
                let ability = ability_tree.find(effect).unwrap();
                handle_ability(ability, &mut self, now);
            }
        }
    }

    fn remove_buff(&mut self, now: u32) {
        for buff in self.base_character.buffs {
            if now > buff.ends_in { 
                match buff.stat {
                    Stat::Health => self.base_character.health = (self.base_character.max_health as f32 / buff.multiplier).round() as u32,
                    Stat::HealthRegen => self.base_character.max_health_regen = (self.base_character.max_health_regen as f32 / buff.multiplier).round() as u32,
                    Stat::Magic => self.base_character.magic = (self.base_character.max_magic as f32 / buff.multiplier).round() as u32,
                    Stat::MagicRegen => self.base_character.max_magic_regen = (self.base_character.max_magic_regen as f32 / buff.multiplier).round() as u32,
                    Stat::Stamina => self.base_character.stamina = (self.base_character.max_stamina as f32 / buff.multiplier).round() as u32,
                    Stat::StaminaRegen => self.base_character.max_stamina_regen = (self.base_character.max_stamina_regen as f32 / buff.multiplier).round() as u32,
                    Stat::Lethality => self.base_character.base_lethality = (self.base_character.base_lethality as f32 / buff.multiplier).round() as u32,    
                    Stat::Hit => self.base_character.hit = (self.base_character.hit as f32 / buff.multiplier).round() as u32,
                    Stat::Agility => self.base_character.accumulated_agility = (self.base_character.accumulated_agility as f32 / buff.multiplier).round() as u32,
                    Stat::Mind => self.base_character.mind = (self.base_character.mind as f32 / buff.multiplier).round() as u32,
                    Stat::Morale => self.base_character.morale = (self.base_character.morale as f32 / buff.multiplier).round() as u32,
                }
            }
        }
    }

    fn attack_power(&self) -> u32 {
        self.base_character.base_lethality + (self.base_character.mind / 2)
    }
}

// -- Petrus (Paladin) --
#[derive(Clone, Debug)]
pub struct Petrus {
    pub base_character: Character,
    pub experience: u32,
}

impl Petrus {
    pub fn new(id: u32) -> Self {
        Self {
            base_character: Character {
            id,
            name: "Petrus".to_string(),
            class: "Paladin".to_string(),
            health: 180,
            max_health: 180,
            magic: 60,
            max_magic: 60,
            stamina: 100,
            max_stamina: 100,
            movement: 5,
            base_lethality: 18,
            hit: 80,
            agility: 7,
            mind: 10,
            morale: 95,
            accumulated_agility: 0,
            damage_multipliers: vec![(DamageType::Physical, 1.0)],
            abilities: vec![],
            ai_parameters: Some(AIParameters {
                aggressiveness: 6,
                caution: 4,
                curiosity: 4,
                perception: 7,
                bravery: 9,
                patience: 6,
            }),
        },
        level: 1,
        }
    }

    pub fn equip(&mut self, equipment_id: u32) {
        let equipment: Equipment = get_equipment_by_id(equipment_id);

        if !() {
            return;
        }
        
        self.equipments.push(equipment);
        self.equipped_lethality += equipment.lethality;
        self.equipped_hit += equipment.hit;
        self.equipped_armor += equipment.armor;
        self.equipped_agility += equipment.agility;
        self.equipped_mind += equipment.mind;
        self.equipped_morale += equipment.morale;
    }

    pub fn unequip(&mut self, equipment_id: u32) {
        for i in 1..self.equipments.length() {
            if self.equipments[i].id == equipment_id {
                self.equipments[i] == self.equipments[self.equipments.length() - 1];
                self.equipments.pop();
                self.equipped_lethality -= equipment.lethality;
                self.equipped_hit -= equipment.hit;
                self.equipped_armor -= equipment.armor;
                self.equipped_agility -= equipment.agility;
                self.equipped_mind -= equipment.mind;
                self.equipped_morale -= equipment.morale;
                break;
            }
        }
    }

    pub fn get_level(&self) -> u8 { 
        self.experience >> 16
    }

    pub fn calculate_xp(&mut self, enemy_experience: u32) {  // for every level it is necessary 65_536 xp 2**16
        let ratio: f32 = enemy_experience as f32 / self.experience as f32;
        let amount: f32;
        
        if ratio > 0.946 {
            // Using log with base 1.25 - Rust doesn't have direct base-n log, so we use change of base
            amount = ((ratio - 0.2).ln() / 1.25f32.ln() + 1.5) << 14 // * 16_384; 2**14
        } else {
            amount = ratio.powf(30.2) << 14 // * 16_384; 2**14
        }
        
        self.experience += amount as u32;
    }

    pub fn level_stamina(&self) -> u32 {
        self.max_stamina += 500 - (self.max_stamina.powf(1.955) >> 19); // 2**19 = 524_288
        self.stamina_regen += 50 - (self.max_stamina_regen.powf(2.05) >> 19);
    }

    pub fn level_health(&self) -> u32 {
        self.max_health += 500 - (self.max_health.powf(1.955) >> 19); // 2**19 = 524_288
        self.health_regen += 50 - (self.max_health_regen.powf(2.21) >> 19);
    }

    pub fn level_agility(&self) -> u32 {
        self.agility += 500 - (self.max_agility.powf(1.955) >> 19); // 2**19 = 524_288
        self.movement += 500 - (self.max_movement.powf(1.955) >> 19);
    }

    pub fn level_lethality(&self) -> u32 {
        self.base_lethality += 500 - (self.max_lethality.powf(1.955) >> 19); // 2**19 = 524_288
        self.hit += 500 - (self.max_hit.powf(1.955) >> 19);
    }

    pub fn level_magic(&self) -> u32 {
        self.max_magic += 500 - (self.max_magic.powf(1.955) >> 19); // 2**19 = 524_288
        self.magic_regen += 50 - (self.max_magic_regen.powf(2.21) >> 19);
    }

    pub fn level_mind(&self) -> u32 {
        self.mind += 500 - (self.max_mind.powf(1.955) >> 19); // 2**19 = 524_288
        self.max_morale += 500 - (self.max_morale.powf(1.955) >> 19); // 2**19 = 524_288
        self.morale_regen += 50 - (self.max_morale_regen.powf(2.21) >> 19);
    }
    
}

impl CharacterBase for Petrus {
    fn id(&self) -> u32 { self.base_character.id }
    fn name(&self) -> &str { &self.base_character.name }
    fn health(&self) -> u32 { self.base_character.health }
    fn max_health(&self) -> u32 { self.base_character.max_health }
    fn agility(&self) -> u32 { self.base_character.agility }
    fn accumulated_agility(&self) ->  u32 { self.base_character.accumulated_agility }
    fn set_accumulated_agility(&mut self, value:  u32) { self.base_character.accumulated_agility = value; }
    fn ai(&self) -> Option<&AIParameters> { self.base_character.ai_parameters.as_ref() }
    fn character_type(&self) -> &str { "Paladin" }

    fn attack(&mut self, target: &mut CharacterBase) {
        self.base_character.before_attack();
        for equipment in self.equipments.iter() {
            for buff in equipment.buffs.iter() {
                buff.before_attack();
            }
        }



        self.base_character.after_attack();
        for equipment in self.equipments.iter() {
            for buff in equipment.buffs.iter() {
                buff.after_attack();
            }
        }
    }

    fn heal(&mut self, amount: u32) {
        self.base_character.health += amount;
    }

    fn damage(&mut self, amount: u32, damage_type: DamageType) {
        let mut total_damage = amount;
        for multiplier in &self.base_character.damage_multipliers {
            if multiplier.0 == damage_type {
                total_damage = (total_damage as f32 * multiplier.1) as u32;
                break;
            }
        }
        // Paladin reduces incoming damage slightly
        let reduced = (total_damage.saturating_sub(1));
        self.base_character.health = self.base_character.health.saturating_sub(reduced);
    }

    fn buff(&mut self, stat: Stat, multiplier: f32, duration: u8, effects: Option<Vec<AbilityEffect>>) {
        self.base_character.;
    }

    fn attack_power(&self) -> u32 {
        self.base_character.base_lethality + (self.base_character.stamina / 4)
    }
}

// -- Rina (Rogue/Thief) --
#[derive(Clone, Debug)]
pub struct Rina {
    pub base_character: Character,
    pub experience: u32,
}

impl Rina {
    pub fn new(id: u32) -> Self {
        Self {
            base_character: Character {
            id,
            name: "Rina".to_string(),
            class: "Rogue".to_string(),
            health: 90,
            max_health: 90,
            magic: 40,
            max_magic: 40,
            stamina: 80,
            max_stamina: 80,
            movement: 7,
            base_lethality: 14,
            hit: 90,
            agility: 14,
            mind: 9,
            morale: 85,
            accumulated_agility: 0,
            damage_multipliers: vec![(DamageType::Physical, 1.0)],
            abilities: vec![],
            ai_parameters: Some(AIParameters {
                aggressiveness: 5,
                caution: 7,
                curiosity: 6,
                perception: 9,
                bravery: 6,
                patience: 4,
            }),
        },
        level: 1,
        }
    }

    pub fn get_level(&self) -> u8 { 
        self.experience >> 16
    }

    pub fn calculate_xp(&mut self, enemy_experience: u32) {  // for every level it is necessary 65_536 xp 2**16
        let ratio: f32 = enemy_experience as f32 / self.experience as f32;
        let amount: f32;
        
        if ratio > 0.946 {
            // Using log with base 1.25 - Rust doesn't have direct base-n log, so we use change of base
            amount = ((ratio - 0.2).ln() / 1.25f32.ln() + 1.5) << 14 // * 16_384; 2**14
        } else {
            amount = ratio.powf(30.2) << 14 // * 16_384; 2**14
        }
        
        self.experience += amount as u32;
    }

    pub fn level_stamina(&self) -> u32 {
        self.max_stamina += 500 - (self.max_stamina.powf(1.955) >> 19); // 2**19 = 524_288
        self.stamina_regen += 50 - (self.max_stamina_regen.powf(2.05) >> 19);
    }

    pub fn level_health(&self) -> u32 {
        self.max_health += 500 - (self.max_health.powf(1.955) >> 19); // 2**19 = 524_288
        self.health_regen += 50 - (self.max_health_regen.powf(2.21) >> 19);
    }

    pub fn level_agility(&self) -> u32 {
        self.agility += 500 - (self.max_agility.powf(1.955) >> 19); // 2**19 = 524_288
        self.movement += 500 - (self.max_movement.powf(1.955) >> 19);
    }

    pub fn level_lethality(&self) -> u32 {
        self.base_lethality += 500 - (self.max_lethality.powf(1.955) >> 19); // 2**19 = 524_288
        self.hit += 500 - (self.max_hit.powf(1.955) >> 19);
    }

    pub fn level_magic(&self) -> u32 {
        self.max_magic += 500 - (self.max_magic.powf(1.955) >> 19); // 2**19 = 524_288
        self.magic_regen += 50 - (self.max_magic_regen.powf(2.21) >> 19);
    }

    pub fn level_mind(&self) -> u32 {
        self.mind += 500 - (self.max_mind.powf(1.955) >> 19); // 2**19 = 524_288
        self.max_morale += 500 - (self.max_morale.powf(1.955) >> 19); // 2**19 = 524_288
        self.morale_regen += 50 - (self.max_morale_regen.powf(2.21) >> 19);
    }

}

impl CharacterBase for Rina {
    fn id(&self) -> u32 { self.base_character.id }
    fn name(&self) -> &str { &self.base_character.name }
    fn health(&self) -> u32 { self.base_character.health }
    fn max_health(&self) -> u32 { self.base_character.max_health }
    fn agility(&self) -> u32 { self.base_character.agility }
    fn accumulated_agility(&self) ->  u32 { self.base_character.accumulated_agility }
    fn set_accumulated_agility(&mut self, value:  u32) { self.base_character.accumulated_agility = value; }
    fn ai(&self) -> Option<&AIParameters> { self.base_character.ai_parameters.as_ref() }
    fn character_type(&self) -> &str { "Rogue" }

    fn heal(&mut self, amount: u32) {
        self.base_character.health += amount;
    }

    fn damage(&mut self, amount: u32, damage_type: DamageType) {
        for multiplier in &self.base_character.damage_multipliers {
            if multiplier.0 == damage_type {
                amount = (amount as f32 * multiplier.1) as u32;
                break;
            }
        }
        // Rogue evades part of attacks based on agility
        // let reduced = (amount as  u32 * (1.0.saturing_sub(self.base_character.agility / 100.0))) as u32; // I prefer to increase the change of evasion somehow
        self.base_character.health = self.base_character.health.saturating_sub(reduced);
    }

    fn attack_power(&self) -> u32 {
        self.base_character.base_lethality + (self.base_character.agility / 2)
    }
}

// -- Sayaka (Cleric) --
#[derive(Clone, Debug)]
pub struct Sayaka {
    pub base_character: Character,
    pub experience: u32,
}

impl Sayaka {
    pub fn new(id: u32) -> Self {
        Self {
            base_character: Character {
            id,
            name: "Sayaka".to_string(),
            class: "Cleric".to_string(),
            health: 110,
            max_health: 110,
            magic: 120,
            max_magic: 120,
            stamina: 70,
            max_stamina: 70,
            movement: 5,
            base_lethality: 10,
            hit: 80,
            agility: 8,
            mind: 15,
            morale: 100,
            accumulated_agility: 0,
            damage_multipliers: vec![(DamageType::Physical, 1.0)],
            abilities: vec![],
            ai_parameters: Some(AIParameters {
                aggressiveness: 2,
                caution: 6,
                curiosity: 7,
                perception: 8,
                bravery: 5,
                patience: 9,
            }),
        },
        level: 1,
        }
    }

    pub fn get_level(&self) -> u8 { 
        self.experience >> 16
    }

    pub fn calculate_xp(&mut self, enemy_experience: u32) {  // for every level it is necessary 65_536 xp 2**16
        let ratio: f32 = enemy_experience as f32 / self.experience as f32;
        let amount: f32;
        
        if ratio > 0.946 {
            // Using log with base 1.25 - Rust doesn't have direct base-n log, so we use change of base
            amount = ((ratio - 0.2).ln() / 1.25f32.ln() + 1.5) << 14 // * 16_384; 2**14
        } else {
            amount = ratio.powf(30.2) << 14 // * 16_384; 2**14
        }
        
        self.experience += amount as u32;
    }

    pub fn level_stamina(&self) -> u32 {
        self.max_stamina += 500 - (self.max_stamina.powf(1.955) >> 19); // 2**19 = 524_288
        self.stamina_regen += 50 - (self.max_stamina_regen.powf(2.05) >> 19);
    }

    pub fn level_health(&self) -> u32 {
        self.max_health += 500 - (self.max_health.powf(1.955) >> 19); // 2**19 = 524_288
        self.health_regen += 50 - (self.max_health_regen.powf(2.21) >> 19);
    }

    pub fn level_agility(&self) -> u32 {
        self.agility += 500 - (self.max_agility.powf(1.955) >> 19); // 2**19 = 524_288
        self.movement += 500 - (self.max_movement.powf(1.955) >> 19);
    }

    pub fn level_lethality(&self) -> u32 {
        self.base_lethality += 500 - (self.max_lethality.powf(1.955) >> 19); // 2**19 = 524_288
        self.hit += 500 - (self.max_hit.powf(1.955) >> 19);
    }

    pub fn level_magic(&self) -> u32 {
        self.max_magic += 500 - (self.max_magic.powf(1.955) >> 19); // 2**19 = 524_288
        self.magic_regen += 50 - (self.max_magic_regen.powf(2.21) >> 19);
    }

    pub fn level_mind(&self) -> u32 {
        self.mind += 500 - (self.max_mind.powf(1.955) >> 19); // 2**19 = 524_288
        self.max_morale += 500 - (self.max_morale.powf(1.955) >> 19); // 2**19 = 524_288
        self.morale_regen += 50 - (self.max_morale_regen.powf(2.21) >> 19);
    }
}

impl CharacterBase for Sayaka {
    fn id(&self) -> u32 { self.base_character.id }
    fn name(&self) -> &str { &self.base_character.name }
    fn health(&self) -> u32 { self.base_character.health }
    fn max_health(&self) -> u32 { self.base_character.max_health }
    fn agility(&self) -> u32 { self.base_character.agility }
    fn accumulated_agility(&self) ->  u32 { self.base_character.accumulated_agility }
    fn set_accumulated_agility(&mut self, value:  u32) { self.base_character.accumulated_agility = value; }
    fn ai(&self) -> Option<&AIParameters> { self.base_character.ai_parameters.as_ref() }
    fn character_type(&self) -> &str { "Cleric" }

    fn heal(&mut self, amount: u32) {
        self.base_character.health += amount;
    }

    fn damage(&mut self, amount: u32, damage_type: DamageType) {
        for multiplier in &self.base_character.damage_multipliers {
            if multiplier.0 == damage_type {
                amount = (amount as f32 * multiplier.1) as u32;
                break;
            }
        }
        self.base_character.health = self.base_character.health.saturating_sub(amount);
    }

    fn attack_power(&self) -> u32 {
        self.base_character.base_lethality + (self.base_character.mind / 2)
    }
}

//
// === TURN MANAGER RESOURCE ===
//
#[derive(Resource)]
pub struct Turn_Manager(TurnManager);

pub struct TurnManager {
    pub characters: Vec<Box<dyn CharacterBase>>,
    pub turn_threshold:  u32,
    pub maximum_value: u32
}

impl TurnManager {
    pub fn new(characters: Vec<Box<dyn CharacterBase>>) -> Self {
        let avg_agility =
            characters.iter().map(|c| c.agility() as  u32).sum::< u32>() / characters.len() as  u32;
        let turn_threshold = avg_agility << 1;

        let maximum_value = (characters.iter().map(|c| c.level() as  u32).sum::< u32>() / characters.len() as  u32) << 3;

        Self { characters, turn_threshold, maximum_value }
    }

    pub fn calculate_turn_order(&mut self) -> Vec<u32> { // require characters vec to be empty
        // This needs to be called with a call to decrease duration
        // To avoid inconsistency for the player, in the turn order the descrease of duration has to be visible
        let mut order = Vec::new();

        for character in self.characters.iter_mut() {
            let mut acc = character.accumulated_agility() + character.agility() + rand::rng().gen_range(0..self.maximum_value) as  u32;
            while acc >= self.turn_threshold {
                acc -= self.turn_threshold;
                order.push(character.id());
                self.characters.push(character.clone());
            }
            // Update accumulated agility
            let mut_clone: &mut dyn CharacterBase = unsafe {
                // safe in this context as we’re the only owner in the loop
                std::mem::transmute(&mut **character)
            };
            mut_clone.set_accumulated_agility(acc);
        }

        order
    }
}

//
// === SETUP HELPERS ===
//
pub fn setup_turn_manager() -> TurnManager {
    let characters: Vec<Box<dyn CharacterBase>> = vec![
        Box::new(Toshiko::new(1)),
        Box::new(Petrus::new(2)),
        Box::new(Rina::new(3)),
        Box::new(Sayaka::new(4)),
    ];

    TurnManager::new(characters)
}

pub fn spawn_default_characters(commands: &mut Commands) {
    commands.spawn(CharacterComponent { character: Box::new(Toshiko::new(1)) });
    commands.spawn(CharacterComponent { character: Box::new(Petrus::new(2)) });
    commands.spawn(CharacterComponent { character: Box::new(Rina::new(3)) });
    commands.spawn(CharacterComponent { character: Box::new(Sayaka::new(4)) });
}

pub fn load_character_from_json(path: &str) -> Result<Character, Box<dyn std::error::Error>> {
    let data = fs::read_to_string(path)?;
    let character: Character = serde_json::from_str(&data)?;
    Ok(character)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AbilityEffect {
    Heal { floor: u32, ceiling: u32 },
    Damage { floor: u32, ceiling: u32, damage_type: DamageType },
    Buff { stat: String, multiplier: f32, effects: Option<Vec<u16>> },   // e.g. "agility", 1.2 multiplier
    //Debuff { stat: String, multiplier: f32 },
    //Custom(String),      // for scripting later
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AbilityShape {
    Radius(f32),
    Line { length: f32, thickness: f32 },
    Cone { angle: f32, radius: f32 },
    Select,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DamageType {
    Slashing,
    Piercing,
    Blunt,
    Fire,
    Ice,
    Lightning,
    Acid,
    Bleeding,
    Poison,
    Dark,
    Light,
    True,
}

//
// === Updated Ability Struct (your version) ===
//

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ability {
    pub id: u16, // first 8 bits is level, second 8 bits is sub-id
    pub next_id: Option<u16>,
    pub name: String,
    pub health_cost: i32,
    pub magic_cost: i32,
    pub stamina_cost: i32,
    pub cooldown: u8,
    pub description: String,
    pub effects: Vec<AbilityEffect>,
    pub shape: AbilityShape,
    pub duration: u8, // 0 for single turn instantenous
    pub targets: u8,
}

impl Ability {
    pub fn get_level(&self) -> u8 { (self.id << 8).try_into().unwrap() }
    pub fn get_sub_id(&self) -> u8 { (self.id >> 8).try_into().unwrap() }
}

//
// === Binary Tree for Abilities ===
//

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
pub struct Ability_Tree(AbilityTree);

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
        
        // INSERTION MUST BE MADE WITH THE FIRST SUB-ID OF EACH LEVEL IN ORDER, SO THE ENTIRE TREE IS AT THE LEFT AND THE LEVELS ARE ALL ONE IN THE RIGHT OF THE OTHER
        let new_id = new_node.read().unwrap().ability.id;
        let current_id = current.read().unwrap().ability.id;

        match new_id.cmp(&current_id) {
            Ordering::Less => {
                if let Some(left) = &current.read().unwrap().left {
                    Self::insert_node(left.clone(), new_node);
                } else {
                    current.write().unwrap().left = Some(new_node);
                }
            }
            Ordering::Greater => {
                if let Some(right) = &current.read().unwrap().right {
                    Self::insert_node(right.clone(), new_node);
                } else {
                    current.write().unwrap().right = Some(new_node);
                }
            }
            Ordering::Equal => {
                // duplicate ID; ignore or replace
                current.write().unwrap().ability = new_node.read().unwrap().ability.clone();
            }
        }
    }

    pub fn find(&self, id: u16) -> Option<Ability> {
        Self::find_node(self.root.clone(), id)
    }

    fn find_node(node: Option<Arc<RwLock<AbilityNode>>>, id: u16) -> Option<Ability> {
        if let Some(n) = node {
            let n_borrow = n.read().unwrap();
            if id == n_borrow.ability.id {
                return Some(n_borrow.ability.clone());
            } else if id < n_borrow.ability.id {
                return Self::find_node(n_borrow.left.clone(), id);
            } else {
                return Self::find_node(n_borrow.right.clone(), id);
            }
        }
        None
    }

    fn find_all_level(&self, level: u8) -> Option<Vec<Ability>> {
        let mut current_node = self.root.clone();

        while let Some(n) = current_node {
            let n_borrow = n.read().unwrap();
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

            let n_borrow = n.read().unwrap();
            results.push(n_borrow.ability.clone());

            // Explore children safely
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
            let n_borrow = n.read().unwrap();
            all.push(n_borrow.ability.clone());
            Self::collect_all(n_borrow.left.clone(), all);
            Self::collect_all(n_borrow.right.clone(), all);
        }
    }
}

pub fn handle_ability(ability: &Ability, affected_characters: &Vec<Box<dyn CharacterBase>>, now: u32 ) {

    let effects: Vec<AbilityEffect> = ability.effects;
    
    for character in affected_characters {
        for effect in effects {
            match effect {
                AbilityEffect::Heal { floor, ceiling } => {
                    let amount = rand::rng().gen_range(floor..ceiling);
                    character.heal(amount);
                }
                AbilityEffect::Damage { floor, ceiling, damage_type } => {
                    let amount = rand::rng().gen_range(floor..ceiling);
                    character.damage(amount, damage_type);
                }
                AbilityEffect::Buff { stat, multiplier, effects } => {
                    character.buff(stat, multiplier, now, ability.duration, effects);
                }
                // AbilityEffect::Debuff { stat, multiplier } => {
                //     character.debuff(stat, multiplier, ability.duration);
                // }
            }
        }
    }

    // TODO: FIND A WAY TO TARGET IN THE NEXT ABILITY, I THINK THAT THE BEST OPTION IS TO CALL THIS MULTIPLE TIMES INSTEAD OF CALLING THIS FUNCTION RECURSIVELY

    // if ability.next_id.is_some() {
    //     let next_id = ability.next_id.clone().unwrap();
    //     let next_ability = ability_tree.find(next_id).unwrap();
    //     let next_affected_characters = get_affected_characters(&next_ability, affected_characters, ); 
    //     handle_ability(&next_ability, &next_affected_characters)
    // }
}

pub fn get_affected_characters(
    ability: &Ability,
    characters: &Vec<Box<dyn CharacterBase>>,
    positions: &Vec<(f32, f32)>,
    player_position: (f32, f32),
    cursor_position: (f32, f32),
) -> Vec<Box<dyn CharacterBase>> {
    let mut affected = Vec::new();

    for (i, &pos) in positions.iter().enumerate() {
        let affected_flag = match &ability.shape {
            AbilityShape::Radius(radius) => is_in_radius(*radius, player_position, pos),

            AbilityShape::Line { length, thickness } => {
                is_in_line(*length, *thickness, player_position, cursor_position, pos)
            }

            AbilityShape::Cone { angle, radius } => {
                is_in_cone(*angle, *radius, player_position, cursor_position, pos)
            }

            AbilityShape::Select => {
                // Single target (for point-click skills)
                let dist = distance(pos, cursor_position);
                dist < 0.5 // small threshold for direct selection
            }
        };

        if affected_flag {
            affected.push(characters[i].clone());
        }
    }

    affected
}

//
// === Geometry Helpers ===
//

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Check if position is inside a circle (radius AoE)
fn is_in_radius(radius: f32, origin: (f32, f32), target: (f32, f32)) -> bool {
    distance(origin, target) <= radius
}

/// Check if position is inside a rectangular line AoE
fn is_in_line(length: f32, thickness: f32, origin: (f32, f32), cursor: (f32, f32), target: (f32, f32)) -> bool {
    // Direction vector (normalized)
    let dir = normalize((cursor.0 - origin.0, cursor.1 - origin.1));
    let to_target = (target.0 - origin.0, target.1 - origin.1);

    // Projection length along the line
    let proj = dot(to_target, dir);

    if proj < 0.0 || proj > length {
        return false;
    }

    // Perpendicular distance to line
    let closest = (origin.0 + dir.0 * proj, origin.1 + dir.1 * proj);
    let dist = distance(closest, target);
    dist <= thickness / 2.0
}

/// Check if position is inside a cone (angle, radius)
fn is_in_cone(angle_deg: f32, radius: f32, origin: (f32, f32), cursor: (f32, f32), target: (f32, f32)) -> bool {
    let dir = normalize((cursor.0 - origin.0, cursor.1 - origin.1));
    let to_target = (target.0 - origin.0, target.1 - origin.1);
    let dist = length(to_target);

    if dist > radius {
        return false;
    }

    let norm_target = normalize(to_target);
    let dot_val = dot(dir, norm_target).clamp(-1.0, 1.0);
    let angle_to_target = dot_val.acos() * (180.0 / PI); // convert to degrees

    angle_to_target <= angle_deg / 2.0
}

//
// === Vector Math ===
//

fn length(v: (f32, f32)) -> f32 {
    (v.0 * v.0 + v.1 * v.1).sqrt()
}

fn normalize(v: (f32, f32)) -> (f32, f32) {
    let len = length(v);
    if len == 0.0 {
        (0.0, 0.0)
    } else {
        (v.0 / len, v.1 / len)
    }
}

fn dot(a: (f32, f32), b: (f32, f32)) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins()
            .add_systems(Startup, )
            .add_systems(Startup, )
            .add_systems(Startup, );
            .insert_resource(TurnManager)
            .insert_resource()
    }
}

