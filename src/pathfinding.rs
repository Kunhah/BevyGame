use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use bevy::math::ops::powf;
use bevy::prelude::*;

use crate::constants::{GRID_HEIGHT, GRID_WIDTH};
use crate::core::Position;
use crate::quadtree::{aabb_collision, QuadTree};

#[derive(Copy, Clone, Eq, PartialEq)]
struct Node_P {
    position: Position,
    cost: i32,
    priority: i32,
}

impl Ord for Node_P {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| other.cost.cmp(&self.cost))
    }
}

impl PartialOrd for Node_P {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(
            other
                .priority
                .cmp(&self.priority)
                .then_with(|| other.cost.cmp(&self.cost)),
        )
    }
}

fn distance(a: Position, b: Position) -> i32 {
    (10.0
        * ((powf((a.x - b.x).abs() as f32, 2.0)
            + powf((a.y - b.y).abs() as f32, 2.0))
            .sqrt()))
    .round() as i32
}

pub fn is_walkable_move(pos: Position, quad_tree: &QuadTree) -> bool {
    if pos.x.abs() as u32 > GRID_WIDTH || pos.y.abs() as u32 > GRID_HEIGHT {
        return false;
    }

    let pos_center = Vec2::new(pos.x as f32, pos.y as f32);
    let player_rect = Rect::from_center_size(pos_center, Vec2::new(32.0, 32.0));

    let mut possible_colliders: Vec<&crate::quadtree::Collider> = Vec::new();
    quad_tree.0.query(player_rect, &mut possible_colliders);

    !possible_colliders
        .iter()
        .any(|collider| aabb_collision(player_rect, collider.bounds))
}

pub fn is_walkable_path(pos: Position, quad_tree: &QuadTree) -> bool {
    if pos.x.abs() as u32 > GRID_WIDTH || pos.y.abs() as u32 > GRID_HEIGHT {
        return false;
    }

    let pos_center = Vec2::new(pos.x as f32, pos.y as f32);
    let player_rect = Rect::from_center_size(pos_center, Vec2::new(32.0, 32.0));

    let mut possible_colliders: Vec<&crate::quadtree::Collider> = Vec::new();
    quad_tree.0.query(player_rect, &mut possible_colliders);

    !possible_colliders
        .iter()
        .any(|collider| aabb_collision(player_rect, collider.bounds))
}

pub fn pathfinding(quad_tree: &QuadTree, start: Position, goal: Position, margin: i32) -> Vec<Position> {
    if !is_walkable_path(start, quad_tree) || !is_walkable_path(goal, quad_tree) {
        return Vec::new();
    }

    let mut open_set = BinaryHeap::new();
    open_set.push(Node_P {
        position: start,
        cost: 0,
        priority: distance(start, goal),
    });

    let mut next_Node_P: Node_P = Node_P {
        position: start,
        cost: 0,
        priority: distance(start, goal),
    };

    let mut came_from: HashMap<Position, Position> = HashMap::new();
    let mut g_score: HashMap<Position, i32> = HashMap::new();
    g_score.insert(start, 0);

    let mut visited: HashSet<Position> = HashSet::new();

    while !((next_Node_P.position.x - goal.x).abs() < margin
        && (next_Node_P.position.y - goal.y).abs() < margin)
    {
        if visited.contains(&next_Node_P.position) {
            next_Node_P = open_set.pop().unwrap();
            continue;
        }
        if visited.len() > 1000 {
            let mut previou_Node_P_position = next_Node_P.position;
            while previou_Node_P_position != start {
                previou_Node_P_position = came_from
                    .get(&previou_Node_P_position)
                    .unwrap()
                    .clone();
            }
            break;
        }
        visited.insert(next_Node_P.position);

        let neighbors = [
            Position {
                x: next_Node_P.position.x + margin,
                y: next_Node_P.position.y - margin,
            },
            Position {
                x: next_Node_P.position.x + margin,
                y: next_Node_P.position.y,
            },
            Position {
                x: next_Node_P.position.x + margin,
                y: next_Node_P.position.y + margin,
            },
            Position {
                x: next_Node_P.position.x,
                y: next_Node_P.position.y + margin,
            },
            Position {
                x: next_Node_P.position.x,
                y: next_Node_P.position.y - margin,
            },
            Position {
                x: next_Node_P.position.x - margin,
                y: next_Node_P.position.y - margin,
            },
            Position {
                x: next_Node_P.position.x - margin,
                y: next_Node_P.position.y,
            },
            Position {
                x: next_Node_P.position.x - margin,
                y: next_Node_P.position.y + margin,
            },
        ];

        for neighbor in neighbors {
            if !is_walkable_path(neighbor, quad_tree) {
                continue;
            }

            let movement_cost =
                if neighbor.x == next_Node_P.position.x || neighbor.y == next_Node_P.position.y {
                    10
                } else {
                    14
                };

            let tentative_g = g_score
                .get(&next_Node_P.position)
                .unwrap_or(&i32::MAX)
                + movement_cost;

            if tentative_g < *g_score.get(&neighbor).unwrap_or(&i32::MAX) {
                came_from.insert(neighbor, next_Node_P.position);
                g_score.insert(neighbor, tentative_g);

                open_set.push(Node_P {
                    position: neighbor,
                    cost: tentative_g,
                    priority: tentative_g + distance(neighbor, goal),
                });
            }
        }
        let old_Node_P = next_Node_P;

        next_Node_P = open_set.pop().unwrap();

        if next_Node_P == old_Node_P {
            break;
        }
    }

    let mut path = vec![next_Node_P.position];
    let mut curr = next_Node_P.position;
    while let Some(&prev) = came_from.get(&curr) {
        path.push(prev);
        curr = prev;
    }
    path.reverse();

    path
}
