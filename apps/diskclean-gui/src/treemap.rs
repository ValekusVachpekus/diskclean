//! Squarified treemap layout.
//!
//! The layout is intentionally independent of the scanner and the UI state so
//! it can be unit-tested and reused later for snapshots or exported images.

use diskclean_core::NodeId;
use eframe::egui::{Rect, pos2};

#[derive(Debug, Clone, Copy)]
pub(crate) struct TreemapTile {
    pub(crate) id: NodeId,
    pub(crate) rect: Rect,
}

/// Lay out items proportional to their values inside `rect`.
///
/// Zero-sized items are ignored. The returned rectangles cover the input rect
/// (minus floating-point rounding) and never overlap.
#[must_use]
pub(crate) fn layout(items: &[(NodeId, u64)], rect: Rect) -> Vec<TreemapTile> {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return Vec::new();
    }

    let mut sorted: Vec<(NodeId, u64)> = items
        .iter()
        .copied()
        .filter(|(_, value)| *value > 0)
        .collect();
    if sorted.is_empty() {
        return Vec::new();
    }
    sorted.sort_unstable_by_key(|(_, value)| std::cmp::Reverse(*value));

    let total: u128 = sorted.iter().map(|(_, value)| u128::from(*value)).sum();
    if total == 0 {
        return Vec::new();
    }

    let area = f64::from(rect.width()) * f64::from(rect.height());
    let areas: Vec<f64> = sorted
        .iter()
        .map(|(_, value)| (u128::from(*value) as f64 / total as f64) * area)
        .collect();

    let mut tiles = Vec::with_capacity(sorted.len());
    let mut remaining = rect;
    let mut index = 0;

    while index < sorted.len() {
        let side = f64::from(remaining.width().min(remaining.height()));
        if side <= f64::EPSILON {
            break;
        }

        let mut end = index;
        let mut row_area = areas[index];
        let mut worst = worst_ratio(row_area, side);
        while end + 1 < areas.len() {
            let candidate = row_area + areas[end + 1];
            let candidate_worst = worst_ratio(candidate, side);
            if candidate_worst > worst {
                break;
            }
            end += 1;
            row_area = candidate;
            worst = candidate_worst;
        }

        let (rects, rest) = layout_row(&areas, index, end, row_area, remaining);
        for (offset, tile_rect) in rects.into_iter().enumerate() {
            tiles.push(TreemapTile {
                id: sorted[index + offset].0,
                rect: tile_rect,
            });
        }
        remaining = rest;
        index = end + 1;
    }

    tiles
}

fn worst_ratio(area: f64, side: f64) -> f64 {
    if area <= f64::EPSILON || side <= f64::EPSILON {
        return f64::INFINITY;
    }
    let thickness = area / side;
    (side / thickness).max(thickness / side)
}

fn layout_row(
    areas: &[f64],
    start: usize,
    end: usize,
    row_area: f64,
    rect: Rect,
) -> (Vec<Rect>, Rect) {
    let mut rects = Vec::with_capacity(end - start + 1);

    if rect.width() >= rect.height() {
        let row_width =
            (row_area / f64::from(rect.height())).clamp(1e-6, f64::from(rect.width())) as f32;
        let mut y = rect.top();
        for (offset, area) in areas[start..=end].iter().enumerate() {
            let height = (area / f64::from(row_width.max(1e-6))) as f32;
            let bottom = if start + offset == end {
                rect.bottom()
            } else {
                (y + height).min(rect.bottom())
            };
            rects.push(Rect::from_min_max(
                pos2(rect.left(), y),
                pos2((rect.left() + row_width).min(rect.right()), bottom),
            ));
            y = bottom;
        }
        let rest = Rect::from_min_max(
            pos2((rect.left() + row_width).min(rect.right()), rect.top()),
            rect.max,
        );
        (rects, rest)
    } else {
        let row_height =
            (row_area / f64::from(rect.width())).clamp(1e-6, f64::from(rect.height())) as f32;
        let mut x = rect.left();
        for (offset, area) in areas[start..=end].iter().enumerate() {
            let width = (area / f64::from(row_height.max(1e-6))) as f32;
            let right = if start + offset == end {
                rect.right()
            } else {
                (x + width).min(rect.right())
            };
            rects.push(Rect::from_min_max(
                pos2(x, rect.top()),
                pos2(right, (rect.top() + row_height).min(rect.bottom())),
            ));
            x = right;
        }
        let rest = Rect::from_min_max(
            pos2(rect.left(), (rect.top() + row_height).min(rect.bottom())),
            rect.max,
        );
        (rects, rest)
    }
}

#[cfg(test)]
mod tests {
    use super::layout;
    use eframe::egui::{Rect, pos2};

    #[test]
    fn empty_input_has_no_tiles() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0));
        assert!(layout(&[], rect).is_empty());
    }

    #[test]
    fn zero_values_are_ignored() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0));
        assert!(layout(&[(1, 0)], rect).is_empty());
    }

    #[test]
    fn tiles_stay_inside_the_bounds() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 100.0));
        let tiles = layout(&[(1, 50), (2, 30), (3, 20)], rect);
        assert_eq!(tiles.len(), 3);
        for tile in tiles {
            assert!(tile.rect.min.x >= rect.min.x - 0.01);
            assert!(tile.rect.min.y >= rect.min.y - 0.01);
            assert!(tile.rect.max.x <= rect.max.x + 0.01);
            assert!(tile.rect.max.y <= rect.max.y + 0.01);
        }
    }

    #[test]
    fn area_roughly_matches_values() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 100.0));
        let tiles = layout(&[(1, 60), (2, 40)], rect);
        let total: f32 = tiles
            .iter()
            .map(|tile| tile.rect.width() * tile.rect.height())
            .sum();
        assert!((total - rect.width() * rect.height()).abs() < 1.0);
    }
}
