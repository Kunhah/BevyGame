#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum AINode {
    IfHPBelow { percent: u8, then: String },
    IfAllyHPBelow { percent: u8, then: String },
    IfCanKillTarget { then: String },
    IfEnemyStunned { then: String },
    IfEnemyIsolated { then: String },
    IfTargetHasLowDefense { then: String },
    IfThreatHigh { then: String },
    Always(String)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AIProfile {
    pub logic: Vec<AINode>
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AIBehaviors {
    pub profiles: HashMap<String, AIProfile>
}

pub fn evaluate_ai(
    profile: &AIProfile,
    actor: Entity,
    world: &WorldQueryData,       // HP, threat, stats, etc
    abilities: &AbilityStore,
) -> Option<AbilityUse> {

    for node in &profile.logic {
        match node {
            AINode::IfHPBelow { percent, then } => {
                if world.hp_of(actor).percent() <= *percent {
                    return pick_ability(actor, then, world, abilities);
                }
            }

            AINode::IfAllyHPBelow { percent, then } => {
                if let Some(ally) = world.find_ally_with_hp_below(*percent) {
                    return pick_ability(actor, then, world, abilities);
                }
            }

            AINode::IfCanKillTarget { then } => {
                if world.predict_kill(actor) {
                    return pick_ability(actor, then, world, abilities);
                }
            }

            AINode::IfEnemyStunned { then } => {
                if world.target_is_stunned(actor) {
                    return pick_ability(actor, then, world, abilities);
                }
            }

            AINode::IfEnemyIsolated { then } => {
                if world.target_is_isolated(actor) {
                    return pick_ability(actor, then, world, abilities);
                }
            }

            AINode::IfTargetHasLowDefense { then } => {
                if world.target_armor(actor) <= 10 {
                    return pick_ability(actor, then, world, abilities);
                }
            }

            AINode::IfThreatHigh { then } => {
                if world.threat_level(actor) > 70 {
                    return pick_ability(actor, then, world, abilities);
                }
            }

            AINode::Always(label) => {
                return pick_ability(actor, label, world, abilities);
            }
        }
    }

    None
}
