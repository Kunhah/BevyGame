use bevy::prelude::*;

use crate::constants::{MAX_LEVELS, MAX_OBJECTS};

#[derive(Component, Clone)]
pub struct Collider {
    pub bounds: Rect,
}

#[derive(Resource, Default)]
pub struct QuadTree(pub QuadtreeNode);

#[derive(Resource, Default)]
pub struct CachedColliders(pub Vec<(Transform, Collider)>);

pub struct QuadtreeNode {
    pub bounds: Rect,
    pub level: usize,
    pub objects: Vec<Collider>,
    pub children: Option<[Box<QuadtreeNode>; 4]>,
}

pub fn aabb_collision(rect1: Rect, rect2: Rect) -> bool {
    rect1.min.x < rect2.max.x
        && rect1.max.x > rect2.min.x
        && rect1.min.y < rect2.max.y
        && rect1.max.y > rect2.min.y
}

impl QuadtreeNode {
    pub fn new(bounds: Rect, level: usize) -> Self {
        Self {
            bounds,
            level,
            objects: Vec::new(),
            children: None,
        }
    }

    fn subdivide(&mut self) {
        let center = self.bounds.center();

        let [min, max] = [self.bounds.min, self.bounds.max];
        let mid = center;

        self.children = Some([
            Box::new(QuadtreeNode::new(
                Rect::from_corners(min, mid),
                self.level + 1,
            )), // bottom-left
            Box::new(QuadtreeNode::new(
                Rect::from_corners(Vec2::new(mid.x, min.y), Vec2::new(max.x, mid.y)),
                self.level + 1,
            )), // bottom-right
            Box::new(QuadtreeNode::new(
                Rect::from_corners(Vec2::new(min.x, mid.y), Vec2::new(mid.x, max.y)),
                self.level + 1,
            )), // top-left
            Box::new(QuadtreeNode::new(Rect::from_corners(mid, max), self.level + 1)), // top-right
        ]);
    }

    pub fn insert(&mut self, collider: Collider) {
        if !aabb_collision(self.bounds, collider.bounds) {
            return;
        }

        if self.children.is_some() {
            if let Some(children) = &mut self.children {
                for child in children.iter_mut() {
                    if child.bounds.contains(collider.bounds.center()) {
                        child.insert(collider);
                        return;
                    }
                }
            }
        }

        self.objects.push(collider);

        if self.objects.len() > MAX_OBJECTS && self.level < MAX_LEVELS {
            if self.children.is_none() {
                self.subdivide();
            }

            if let Some(children) = &mut self.children {
                let mut reinsert = Vec::new();
                std::mem::swap(&mut self.objects, &mut reinsert);
                for obj in reinsert {
                    self.insert(obj);
                }
            }
        }
    }

    pub fn query<'a>(&'a self, area: Rect, found: &mut Vec<&'a Collider>) {
        if !aabb_collision(self.bounds, area) {
            return;
        }

        for collider in &self.objects {
            if aabb_collision(collider.bounds, area) {
                found.push(collider);
            }
        }

        if let Some(children) = &self.children {
            for child in children {
                child.query(area, found);
            }
        }
    }
}

impl Default for QuadtreeNode {
    fn default() -> Self {
        Self {
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            level: 0,
            objects: Vec::new(),
            children: None,
        }
    }
}
