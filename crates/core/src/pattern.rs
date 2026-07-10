//! Detection and morphological corner rounding of two-color block patterns.

use std::collections::HashMap;

use image::RgbaImage;
use tracing::warn;

/// Squared RGB distance beyond which a pixel no longer counts as background.
const FG_DISTANCE_SQ: i32 = 900;
/// A two-color pattern requires at least 1/200 (0.5%) of the pixels to be
/// far from the background color.
const MIN_FG_DENOMINATOR: usize = 200;
/// Foreground runs shorter than this are ignored when estimating the block
/// size, so stray anti-aliased pixels cannot masquerade as a cell.
const MIN_RUN: usize = 4;
/// Sentinel for "no seed on this line" in the distance transform. Large
/// enough to dominate any real squared distance, small enough to stay finite.
const FAR: f64 = 1e20;

/// The two dominant colors of an identicon-like image and the foreground
/// membership mask in row-major order.
struct Pattern {
    bg: [u8; 3],
    fg: [u8; 3],
    mask: Vec<bool>,
}

fn rgb(pixel: image::Rgba<u8>) -> [u8; 3] {
    [pixel[0], pixel[1], pixel[2]]
}

fn color_distance_sq(a: [u8; 3], b: [u8; 3]) -> i32 {
    let dr = i32::from(a[0]) - i32::from(b[0]);
    let dg = i32::from(a[1]) - i32::from(b[1]);
    let db = i32::from(a[2]) - i32::from(b[2]);
    dr * dr + dg * dg + db * db
}

/// Detects the background (majority color of the 1px border ring) and the
/// foreground (most frequent color far from the background). Returns `None`
/// when the image has no discernible two-color pattern.
fn detect_pattern(image: &RgbaImage) -> Option<Pattern> {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return None;
    }

    let mut border: HashMap<[u8; 3], usize> = HashMap::new();
    for x in 0..width {
        *border.entry(rgb(*image.get_pixel(x, 0))).or_insert(0) += 1;
        *border
            .entry(rgb(*image.get_pixel(x, height - 1)))
            .or_insert(0) += 1;
    }
    for y in 0..height {
        *border.entry(rgb(*image.get_pixel(0, y))).or_insert(0) += 1;
        *border
            .entry(rgb(*image.get_pixel(width - 1, y)))
            .or_insert(0) += 1;
    }
    let bg = border
        .into_iter()
        .max_by_key(|&(color, count)| (count, color))
        .map(|(color, _)| color)?;

    let mut candidates: HashMap<[u8; 3], usize> = HashMap::new();
    let mut candidate_total = 0usize;
    for pixel in image.pixels() {
        let color = rgb(*pixel);
        if color_distance_sq(color, bg) > FG_DISTANCE_SQ {
            *candidates.entry(color).or_insert(0) += 1;
            candidate_total += 1;
        }
    }
    let total = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    if candidate_total * MIN_FG_DENOMINATOR < total {
        return None;
    }
    let fg = candidates
        .into_iter()
        .max_by_key(|&(color, count)| (count, color))
        .map(|(color, _)| color)?;

    let mask = image
        .pixels()
        .map(|pixel| {
            let color = rgb(*pixel);
            color_distance_sq(color, fg) < color_distance_sq(color, bg)
        })
        .collect();

    Some(Pattern { bg, fg, mask })
}

/// Estimates the block (cell) size of the pattern as the shortest horizontal
/// or vertical run of consecutive foreground pixels, ignoring runs shorter
/// than [`MIN_RUN`].
fn detect_cell(mask: &[bool], width: usize, height: usize) -> Option<usize> {
    let mut shortest: Option<usize> = None;
    let mut consider = |run: usize| {
        if run >= MIN_RUN && shortest.is_none_or(|current| run < current) {
            shortest = Some(run);
        }
    };

    for y in 0..height {
        let mut run = 0usize;
        for x in 0..width {
            if mask[y * width + x] {
                run += 1;
            } else {
                consider(run);
                run = 0;
            }
        }
        consider(run);
    }
    for x in 0..width {
        let mut run = 0usize;
        for y in 0..height {
            if mask[y * width + x] {
                run += 1;
            } else {
                consider(run);
                run = 0;
            }
        }
        consider(run);
    }

    shortest
}

/// 1D squared distance transform (Felzenszwalb & Huttenlocher lower envelope
/// of parabolas). `sample` holds 0.0 at seeds and [`FAR`] elsewhere; `out`
/// receives the squared distance to the nearest seed along this axis.
#[expect(
    clippy::cast_precision_loss,
    reason = "indices are bounded by the image dimensions, far below f64's exact-integer range"
)]
fn distance_transform_1d(sample: &[f64], out: &mut [f64], hull: &mut [usize], bounds: &mut [f64]) {
    let len = sample.len();
    let mut top = 0usize;
    hull[0] = 0;
    bounds[0] = f64::NEG_INFINITY;
    bounds[1] = f64::INFINITY;

    let parabola = |index: usize| sample[index] + (index * index) as f64;

    for index in 1..len {
        let mut crossing =
            (parabola(index) - parabola(hull[top])) / (2.0 * (index - hull[top]) as f64);
        while crossing <= bounds[top] {
            top -= 1;
            crossing = (parabola(index) - parabola(hull[top])) / (2.0 * (index - hull[top]) as f64);
        }
        top += 1;
        hull[top] = index;
        bounds[top] = crossing;
        bounds[top + 1] = f64::INFINITY;
    }

    top = 0;
    for (index, slot) in out.iter_mut().enumerate() {
        while bounds[top + 1] < index as f64 {
            top += 1;
        }
        let delta = index as f64 - hull[top] as f64;
        *slot = delta * delta + sample[hull[top]];
    }
}

/// Exact squared euclidean distance from every pixel to the nearest `true`
/// pixel of `mask`, computed with two separable 1D transforms.
fn edt_squared(mask: &[bool], width: usize, height: usize) -> Vec<f64> {
    let mut grid: Vec<f64> = mask.iter().map(|&m| if m { 0.0 } else { FAR }).collect();
    let longest = width.max(height);
    let mut sample = vec![0.0f64; longest];
    let mut out = vec![0.0f64; longest];
    let mut hull = vec![0usize; longest];
    let mut bounds = vec![0.0f64; longest + 1];

    for y in 0..height {
        let row = y * width..(y + 1) * width;
        sample[..width].copy_from_slice(&grid[row.clone()]);
        distance_transform_1d(
            &sample[..width],
            &mut out[..width],
            &mut hull[..width],
            &mut bounds[..=width],
        );
        grid[row].copy_from_slice(&out[..width]);
    }
    for x in 0..width {
        for y in 0..height {
            sample[y] = grid[y * width + x];
        }
        distance_transform_1d(
            &sample[..height],
            &mut out[..height],
            &mut hull[..height],
            &mut bounds[..=height],
        );
        for y in 0..height {
            grid[y * width + x] = out[y];
        }
    }
    grid
}

/// Rounds the corners of the two-color block pattern by `radius_ratio` of the
/// detected cell size (capped just below half a cell so isolated cells become
/// circles instead of vanishing). Implemented as morphological closing then
/// opening — dilate(r), erode(2r), dilate(r) — with exact euclidean distance
/// transforms and an anti-aliased final edge. Both convex and concave corners
/// are rounded; the canvas, its background, and the alpha channel stay as-is.
/// Returns `None` when no two-color pattern is detected.
#[must_use]
pub fn round_pattern(image: &RgbaImage, radius_ratio: f32) -> Option<RgbaImage> {
    let pattern = detect_pattern(image)?;
    let (width, height) = image.dimensions();
    let w = usize::try_from(width).ok()?;
    let h = usize::try_from(height).ok()?;

    let cell = detect_cell(&pattern.mask, w, h)?;
    let cell_f = f64::from(u32::try_from(cell).ok()?);
    let radius = (f64::from(radius_ratio.clamp(0.0, 0.5)) * cell_f).min(cell_f / 2.0 - 1.0);
    if radius <= 0.0 {
        warn!("detected cell size {cell} is too small to round; returning image unchanged");
        return Some(image.clone());
    }

    let dist_to_fg = edt_squared(&pattern.mask, w, h);
    let dilated: Vec<bool> = dist_to_fg.iter().map(|&d| d <= radius * radius).collect();

    let outside_dilated: Vec<bool> = dilated.iter().map(|&covered| !covered).collect();
    let dist_to_outside = edt_squared(&outside_dilated, w, h);
    let erosion = 2.0 * radius;
    let core: Vec<bool> = dist_to_outside
        .iter()
        .map(|&d| d > erosion * erosion)
        .collect();
    if !core.contains(&true) {
        warn!("pattern vanished during erosion; returning image unchanged");
        return Some(image.clone());
    }

    let dist_to_core = edt_squared(&core, w, h);

    let mut result = image.clone();
    for (index, pixel) in result.pixels_mut().enumerate() {
        // Pixel centers sit half a pixel inside the region boundary, so full
        // coverage is reached at distance == radius (area-consistent AA).
        let coverage = (1.0 - (dist_to_core[index].sqrt() - radius)).clamp(0.0, 1.0);
        for channel in 0..3 {
            let blended = f64::from(pattern.bg[channel])
                + (f64::from(pattern.fg[channel]) - f64::from(pattern.bg[channel])) * coverage;
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "blended is a convex combination of two u8 values, so its rounding fits in u8"
            )]
            let value = blended.round() as u8;
            pixel[channel] = value;
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::{color_distance_sq, detect_cell, detect_pattern, edt_squared, round_pattern};
    use crate::DEFAULT_RADIUS_RATIO;
    use image::{Rgba, RgbaImage};

    const BG: [u8; 3] = [240, 240, 240];
    const FG: [u8; 3] = [50, 100, 200];
    const CELL: u32 = 70;

    fn pattern_image(width: u32, height: u32, cells: &[(u32, u32)]) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([BG[0], BG[1], BG[2], 255]));
        for &(cx, cy) in cells {
            for y in cy * CELL..(cy + 1) * CELL {
                for x in cx * CELL..(cx + 1) * CELL {
                    img.put_pixel(x, y, Rgba([FG[0], FG[1], FG[2], 255]));
                }
            }
        }
        img
    }

    fn rgb_at(img: &RgbaImage, x: u32, y: u32) -> [u8; 3] {
        let pixel = img.get_pixel(x, y);
        [pixel[0], pixel[1], pixel[2]]
    }

    #[test]
    #[expect(
        clippy::cast_precision_loss,
        reason = "test coordinates are tiny integers, exactly representable"
    )]
    fn edt_of_single_seed_is_squared_euclidean() {
        let mut mask = vec![false; 25];
        mask[0] = true;
        let dist = edt_squared(&mask, 5, 5);
        for y in 0..5usize {
            for x in 0..5usize {
                let expected = (x * x + y * y) as f64;
                assert!(
                    (dist[y * 5 + x] - expected).abs() < 1e-9,
                    "distance at ({x}, {y}) should be {expected}, got {}",
                    dist[y * 5 + x]
                );
            }
        }
    }

    #[test]
    fn single_block_corners_become_background() {
        let img = pattern_image(210, 210, &[(1, 1)]);
        let Some(out) = round_pattern(&img, DEFAULT_RADIUS_RATIO) else {
            panic!("pattern should be detected");
        };
        assert_eq!(out.dimensions(), (210, 210));
        assert_eq!(
            rgb_at(&out, 70, 70),
            BG,
            "block corner must be rounded away"
        );
        assert_eq!(rgb_at(&out, 139, 139), BG, "opposite block corner too");
        assert_eq!(
            rgb_at(&out, 105, 105),
            FG,
            "block center must stay foreground"
        );
        assert_eq!(
            rgb_at(&out, 105, 70),
            FG,
            "top edge midpoint must stay foreground"
        );
        assert_eq!(
            rgb_at(&out, 70, 105),
            FG,
            "left edge midpoint must stay foreground"
        );
        assert_eq!(rgb_at(&out, 0, 0), BG, "image corner must stay background");
        assert!(out.pixels().all(|p| p[3] == 255), "canvas must stay opaque");
    }

    #[test]
    fn concave_corner_is_filled() {
        let img = pattern_image(280, 280, &[(1, 1), (2, 1), (1, 2)]);
        let Some(out) = round_pattern(&img, DEFAULT_RADIUS_RATIO) else {
            panic!("pattern should be detected");
        };
        let color = rgb_at(&out, 145, 145);
        assert!(
            color_distance_sq(color, FG) < color_distance_sq(color, BG),
            "concave inner corner should be filled by closing, got {color:?}"
        );
    }

    #[test]
    #[expect(
        clippy::cast_precision_loss,
        reason = "pixel counts are far below f64's exact-integer range"
    )]
    fn max_ratio_turns_isolated_cell_into_circle() {
        let img = pattern_image(210, 210, &[(1, 1)]);
        let Some(out) = round_pattern(&img, 0.5) else {
            panic!("pattern should be detected");
        };
        let fg_count = out
            .pixels()
            .filter(|p| {
                let color = [p[0], p[1], p[2]];
                color_distance_sq(color, FG) < color_distance_sq(color, BG)
            })
            .count();
        assert!(fg_count > 0, "the cell must survive erosion");
        assert_eq!(
            rgb_at(&out, 70, 70),
            BG,
            "corner must be far outside the circle"
        );
        assert_eq!(rgb_at(&out, 105, 105), FG, "center must stay foreground");
        let circle_area = std::f64::consts::PI / 4.0 * f64::from(CELL * CELL);
        let deviation = (fg_count as f64 - circle_area).abs() / circle_area;
        assert!(
            deviation < 0.15,
            "shape should approximate a circle: {fg_count} px vs {circle_area:.0} px"
        );
    }

    #[test]
    fn uniform_image_has_no_pattern() {
        let img = pattern_image(210, 210, &[]);
        assert!(detect_pattern(&img).is_none());
        assert!(round_pattern(&img, DEFAULT_RADIUS_RATIO).is_none());
    }

    #[test]
    fn cell_size_is_detected_from_runs() {
        let img = pattern_image(280, 280, &[(1, 1), (2, 2)]);
        let Some(pattern) = detect_pattern(&img) else {
            panic!("pattern should be detected");
        };
        assert_eq!(detect_cell(&pattern.mask, 280, 280), Some(70));
    }
}
