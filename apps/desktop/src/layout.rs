use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub type PaneId = u64;
pub const MAX_PANES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Horizontal,
    Vertical,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Layout {
    Pane {
        id: PaneId,
    },
    Split {
        axis: Axis,
        ratio: f32,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
#[derive(Clone, Copy)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}
impl Layout {
    pub fn pane(id: PaneId) -> Self {
        Self::Pane { id }
    }
    pub fn ids(&self) -> Vec<PaneId> {
        match self {
            Self::Pane { id } => vec![*id],
            Self::Split { first, second, .. } => {
                let mut ids = first.ids();
                ids.extend(second.ids());
                ids
            }
        }
    }
    pub fn validate(&self) -> bool {
        fn visit(node: &Layout, depth: usize, ids: &mut BTreeSet<PaneId>) -> bool {
            if depth > MAX_PANES {
                return false;
            }
            match node {
                Layout::Pane { id } => *id > 0 && ids.insert(*id) && ids.len() <= MAX_PANES,
                Layout::Split {
                    ratio,
                    first,
                    second,
                    ..
                } => {
                    ratio.is_finite()
                        && (0.05..=0.95).contains(ratio)
                        && visit(first, depth + 1, ids)
                        && visit(second, depth + 1, ids)
                }
            }
        }
        visit(self, 0, &mut BTreeSet::new())
    }
    pub fn split(&mut self, target: PaneId, id: PaneId, axis: Axis) -> bool {
        let ids = self.ids();
        if ids.len() >= MAX_PANES || ids.contains(&id) || id == 0 {
            return false;
        }
        self.split_inner(target, id, axis)
    }
    fn split_inner(&mut self, target: PaneId, id: PaneId, axis: Axis) -> bool {
        match self {
            Self::Pane { id: old } if *old == target => {
                *self = Self::Split {
                    axis,
                    ratio: 0.5,
                    first: Box::new(Self::pane(target)),
                    second: Box::new(Self::pane(id)),
                };
                true
            }
            Self::Split { first, second, .. } => {
                first.split_inner(target, id, axis) || second.split_inner(target, id, axis)
            }
            _ => false,
        }
    }
    pub fn remove(self, target: PaneId) -> Option<Self> {
        match self {
            Self::Pane { id } => (id != target).then_some(Self::pane(id)),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => match (first.remove(target), second.remove(target)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    axis,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (node, None) | (None, node) => node,
            },
        }
    }
    pub fn set_ratio(&mut self, path: &[bool], value: f32) {
        if !value.is_finite() {
            return;
        }
        if let Self::Split {
            ratio,
            first,
            second,
            ..
        } = self
        {
            match path.split_first() {
                None => *ratio = value.clamp(0.05, 0.95),
                Some((false, tail)) => first.set_ratio(tail, value),
                Some((true, tail)) => second.set_ratio(tail, value),
            }
        }
    }
    pub fn rectangles(&self, bounds: Rect) -> Vec<(PaneId, Rect)> {
        let mut result = Vec::new();
        self.collect_rectangles(bounds, &mut result);
        result
    }
    fn collect_rectangles(&self, bounds: Rect, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Self::Pane { id } => out.push((*id, bounds)),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let (mut a, mut b) = (bounds, bounds);
                match axis {
                    Axis::Horizontal => {
                        a.width *= ratio;
                        b.x += a.width;
                        b.width -= a.width;
                    }
                    Axis::Vertical => {
                        a.height *= ratio;
                        b.y += a.height;
                        b.height -= a.height;
                    }
                }
                first.collect_rectangles(a, out);
                second.collect_rectangles(b, out);
            }
        }
    }
    pub fn neighbor(&self, id: PaneId, direction: Direction) -> Option<PaneId> {
        let rectangles = self.rectangles(Rect {
            width: 1.0,
            height: 1.0,
            ..Rect::default()
        });
        let origin = rectangles.iter().find(|(i, _)| *i == id)?.1;
        let (x, y) = (
            origin.x + origin.width / 2.0,
            origin.y + origin.height / 2.0,
        );
        rectangles
            .iter()
            .filter(|(i, _)| *i != id)
            .filter_map(|(i, r)| {
                let (dx, dy) = (r.x + r.width / 2.0 - x, r.y + r.height / 2.0 - y);
                let (forward, sideways) = match direction {
                    Direction::Left => (-dx, dy.abs()),
                    Direction::Right => (dx, dy.abs()),
                    Direction::Up => (-dy, dx.abs()),
                    Direction::Down => (dy, dx.abs()),
                };
                (forward > 0.001).then_some((*i, forward + sideways * 4.0))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_splits_close_without_orphans_and_direction_follows_geometry() {
        let mut tree = Layout::pane(1);
        assert!(tree.split(1, 2, Axis::Horizontal));
        assert!(tree.split(2, 3, Axis::Vertical));
        assert_eq!(tree.neighbor(1, Direction::Right), Some(2));
        assert_eq!(tree.neighbor(2, Direction::Down), Some(3));
        assert_eq!(tree.neighbor(3, Direction::Left), Some(1));
        let tree = tree.remove(2).unwrap();
        assert_eq!(tree.ids(), [1, 3]);
        assert!(tree.validate());
        assert_eq!(tree.remove(1), Some(Layout::pane(3)));
    }
    #[test]
    fn limits_duplicate_ids_and_saved_ratios_are_checked() {
        let mut tree = Layout::pane(1);
        for id in 2..=8 {
            assert!(tree.split(1, id, Axis::Horizontal));
        }
        assert!(!tree.split(1, 9, Axis::Vertical));
        assert!(!tree.split(1, 2, Axis::Vertical));
        tree.set_ratio(&[], f32::NAN);
        assert!(tree.validate());
        let invalid = Layout::Split {
            axis: Axis::Horizontal,
            ratio: 0.0,
            first: Box::new(Layout::pane(1)),
            second: Box::new(Layout::pane(1)),
        };
        assert!(!invalid.validate());
    }
}
