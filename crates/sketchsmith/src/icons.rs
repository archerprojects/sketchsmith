//! SketchSmith — tool icons | v1.0.0 | Thursday 12 November 2026
//!
//! Original line icons for the Geometry panel, drawn as vectors with the egui
//! painter. Drawing them in code (rather than shipping image files) keeps them
//! sharp at any display scale, lets them follow the theme colour and the
//! disabled state, and adds no dependencies. None imitates SketchUp's icons.
//!
//! Each icon is described in a unit square (0..1, y down) and scaled into the
//! button. [`paint`] returns `false` for an unknown name so the caller can fall
//! back to a short text label.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2};

/// Draws the icon `name` into `rect`. Returns `false` if there is no icon.
pub fn paint(painter: &Painter, rect: Rect, name: &str, color: Color32) -> bool {
    let p = Pen {
        painter,
        rect: rect.shrink(rect.width() * 0.16),
        stroke: Stroke::new(1.5, color),
        color,
    };
    match name {
        "Select" => p.poly(
            &[
                (0.25, 0.1),
                (0.25, 0.85),
                (0.43, 0.67),
                (0.58, 0.95),
                (0.7, 0.89),
                (0.55, 0.61),
                (0.8, 0.61),
            ],
            true,
        ),
        "Line" => {
            p.line((0.15, 0.85), (0.85, 0.15));
            p.dot((0.15, 0.85));
            p.dot((0.85, 0.15));
        }
        "Rectangle" => p.poly(&[(0.1, 0.25), (0.9, 0.25), (0.9, 0.75), (0.1, 0.75)], true),
        "Rotated Rectangle" => p.poly(&[(0.1, 0.55), (0.55, 0.1), (0.9, 0.45), (0.45, 0.9)], true),
        "Circle" => {
            p.circle((0.5, 0.5), 0.42);
            p.dot((0.5, 0.5));
        }
        "Arc" => {
            p.arc((0.5, 0.75), 0.42, PI, TAU);
            p.dot((0.08, 0.75));
            p.dot((0.92, 0.75));
        }
        "Polygon" => {
            let pts: Vec<(f32, f32)> = (0..6)
                .map(|i| {
                    let a = TAU * i as f32 / 6.0;
                    (0.5 + 0.42 * a.cos(), 0.5 + 0.42 * a.sin())
                })
                .collect();
            p.poly(&pts, true);
        }
        "Bézier Spline" => {
            p.curve(|t| (0.1 + 0.8 * t, 0.5 - 0.32 * (TAU * t).sin()));
            p.dot((0.1, 0.5));
            p.dot((0.9, 0.5));
        }
        "Curve Maker" => {
            // A flat spiral, the Curve Maker's simplest shape.
            p.curve(|t| {
                let a = TAU * 2.25 * t;
                let r = 0.04 + 0.36 * t;
                (0.5 + r * a.cos(), 0.5 - r * a.sin())
            });
        }
        "Extrude" => {
            p.poly(&[(0.1, 0.65), (0.6, 0.65), (0.9, 0.85), (0.4, 0.85)], true);
            p.line((0.5, 0.6), (0.5, 0.1));
            p.poly(&[(0.38, 0.24), (0.5, 0.1), (0.62, 0.24)], false);
        }
        "Move" => {
            p.line((0.5, 0.08), (0.5, 0.92));
            p.line((0.08, 0.5), (0.92, 0.5));
            for (a, b, c) in [
                ((0.4, 0.18), (0.5, 0.08), (0.6, 0.18)),
                ((0.4, 0.82), (0.5, 0.92), (0.6, 0.82)),
                ((0.18, 0.4), (0.08, 0.5), (0.18, 0.6)),
                ((0.82, 0.4), (0.92, 0.5), (0.82, 0.6)),
            ] {
                p.poly(&[a, b, c], false);
            }
        }
        "Rotate" => {
            p.arc((0.5, 0.5), 0.38, -FRAC_PI_2, PI);
            p.poly(&[(0.02, 0.42), (0.12, 0.55), (0.24, 0.43)], false);
        }
        "Scale" => {
            p.poly(&[(0.1, 0.1), (0.9, 0.1), (0.9, 0.9), (0.1, 0.9)], true);
            p.poly(&[(0.1, 0.55), (0.45, 0.55), (0.45, 0.9)], false);
            p.line((0.45, 0.55), (0.82, 0.18));
            p.poly(&[(0.62, 0.18), (0.82, 0.18), (0.82, 0.38)], false);
        }
        "Offset (Inset)" => {
            p.poly(
                &[(0.08, 0.08), (0.92, 0.08), (0.92, 0.92), (0.08, 0.92)],
                true,
            );
            p.poly(&[(0.3, 0.3), (0.7, 0.3), (0.7, 0.7), (0.3, 0.7)], true);
        }
        "Eraser" => {
            p.poly(&[(0.2, 0.7), (0.6, 0.15), (0.9, 0.38), (0.5, 0.92)], true);
            p.line((0.35, 0.5), (0.65, 0.73));
        }
        "Mirror" => {
            p.dashed((0.5, 0.05), (0.5, 0.95));
            p.poly(&[(0.4, 0.25), (0.08, 0.75), (0.4, 0.75)], true);
            p.poly(&[(0.6, 0.25), (0.92, 0.75), (0.6, 0.75)], true);
        }
        "Loop Cut" => {
            p.ellipse((0.5, 0.15), 0.35, 0.09);
            p.line((0.15, 0.15), (0.15, 0.85));
            p.line((0.85, 0.15), (0.85, 0.85));
            p.ellipse((0.5, 0.85), 0.35, 0.09);
            p.ellipse_thick((0.5, 0.5), 0.35, 0.09);
        }
        "Slide" => {
            // A point moving along its edge: the edge, the point, an arrow.
            p.line((0.1, 0.75), (0.9, 0.25));
            p.dot((0.38, 0.575));
            p.dashed((0.38, 0.575), (0.66, 0.4));
            p.line((0.66, 0.4), (0.54, 0.42));
            p.line((0.66, 0.4), (0.6, 0.5));
        }
        "Dissolve" => {
            // Two straight edges; the point between them fades to a ring.
            p.line((0.08, 0.7), (0.42, 0.5));
            p.line((0.58, 0.41), (0.92, 0.2));
            p.circle((0.5, 0.455), 0.07);
        }
        "Knife" => {
            p.poly(
                &[(0.08, 0.92), (0.55, 0.45), (0.92, 0.08), (0.7, 0.55)],
                true,
            );
            p.line((0.08, 0.92), (0.3, 0.7));
        }
        "Extrude along Path" => {
            p.curve(|t| (0.2 + 0.7 * t, 0.85 - 0.7 * t * t));
            p.ellipse((0.2, 0.85), 0.1, 0.06);
            p.ellipse((0.9, 0.15), 0.06, 0.1);
        }
        "Loft along Path" => {
            // A square changing into a circle along a curve.
            p.curve(|t| (0.2 + 0.5 * t, 0.8 - 0.5 * t * t));
            p.line((0.08, 0.72), (0.32, 0.72));
            p.line((0.32, 0.72), (0.32, 0.88));
            p.line((0.32, 0.88), (0.08, 0.88));
            p.line((0.08, 0.88), (0.08, 0.72));
            p.ellipse((0.7, 0.3), 0.14, 0.14);
        }
        "Weld" => {
            // Two pieces meeting, with a weld bead's ripples over the joint.
            p.line((0.08, 0.75), (0.42, 0.75));
            p.line((0.58, 0.75), (0.92, 0.75));
            for x in [0.38f32, 0.5, 0.62] {
                p.arc((x, 0.62), 0.09, PI, PI * 2.0);
            }
            p.line((0.42, 0.75), (0.58, 0.75));
        }
        "Trim / Extend" => {
            // Two lines carried to meet at a corner.
            p.line((0.12, 0.82), (0.55, 0.82));
            p.line((0.82, 0.12), (0.82, 0.55));
            p.line((0.6, 0.82), (0.82, 0.82));
            p.line((0.82, 0.6), (0.82, 0.82));
            p.ellipse((0.82, 0.82), 0.05, 0.05);
        }
        "Extend to Face" => {
            // A line carried up to a sloping face.
            p.line((0.25, 0.92), (0.25, 0.45));
            p.line((0.25, 0.45), (0.25, 0.3));
            p.line((0.05, 0.4), (0.95, 0.1));
            p.ellipse((0.25, 0.33), 0.05, 0.05);
        }
        "Skinning" => {
            // Arcs rising from a base to a top, the cells between them
            // shaded with lines across.
            p.line((0.1, 0.85), (0.9, 0.85));
            p.arc((0.5, 0.85), 0.4, PI, PI * 1.5);
            p.arc((0.5, 0.85), 0.4, PI * 1.5, PI * 2.0);
            p.line((0.5, 0.85), (0.5, 0.45));
            for x in [0.22f32, 0.35] {
                p.line((x, 0.85), (x + 0.12, 0.55));
            }
            for x in [0.65f32, 0.78] {
                p.line((x, 0.85), (x - 0.12, 0.55));
            }
        }
        "Bridge" => {
            // Two rings joined by a band.
            p.ellipse((0.2, 0.5), 0.1, 0.25);
            p.ellipse((0.8, 0.5), 0.1, 0.25);
            p.line((0.2, 0.25), (0.8, 0.25));
            p.line((0.2, 0.75), (0.8, 0.75));
        }
        "Bisector" => {
            // An angle split in half by a dashed line.
            p.line((0.1, 0.9), (0.9, 0.9));
            p.line((0.1, 0.9), (0.6, 0.1));
            for i in 0..4 {
                let t0 = i as f32 * 0.22;
                let t1 = t0 + 0.12;
                p.line(
                    (0.1 + 0.85 * t0, 0.9 - 0.4 * t0),
                    (0.1 + 0.85 * t1, 0.9 - 0.4 * t1),
                );
            }
        }
        "Space" => {
            // Points evenly along a line, with spacing ticks.
            p.line((0.08, 0.6), (0.92, 0.6));
            for x in [0.08f32, 0.36, 0.64, 0.92] {
                p.ellipse((x, 0.6), 0.05, 0.05);
            }
            p.line((0.08, 0.35), (0.36, 0.35));
            p.line((0.64, 0.35), (0.92, 0.35));
        }
        "Circle Loop" => {
            // A square of points pulled round onto a circle.
            p.ellipse((0.5, 0.5), 0.36, 0.36);
            for (x, y) in [(0.24f32, 0.24f32), (0.76, 0.24), (0.76, 0.76), (0.24, 0.76)] {
                p.ellipse((x, y), 0.05, 0.05);
            }
        }
        "Flatten" => {
            // A wavy line pressed flat onto a plane.
            p.curve(|t| (0.1 + 0.8 * t, 0.35 + 0.1 * (t * 12.0).sin()));
            p.line((0.1, 0.72), (0.9, 0.72));
            p.line((0.5, 0.48), (0.5, 0.64));
        }
        "Smooth" => {
            // A jagged line and the smooth one it becomes.
            p.line((0.08, 0.4), (0.3, 0.2));
            p.line((0.3, 0.2), (0.5, 0.45));
            p.line((0.5, 0.45), (0.7, 0.18));
            p.line((0.7, 0.18), (0.92, 0.4));
            p.curve(|t| (0.08 + 0.84 * t, 0.75 - 0.08 * (t * PI).sin()));
        }
        "Relax" => {
            // A smooth line with its points evened out.
            p.curve(|t| (0.08 + 0.84 * t, 0.6 - 0.25 * (t * PI).sin()));
            for t in [0.0f32, 0.33, 0.66, 1.0] {
                p.ellipse((0.08 + 0.84 * t, 0.6 - 0.25 * (t * PI).sin()), 0.05, 0.05);
            }
        }
        "Intersect Faces" => {
            // An upright sheet through a flat one, the cut line where they
            // cross marked at its ends.
            p.poly(
                &[(0.06, 0.6), (0.62, 0.6), (0.94, 0.86), (0.38, 0.86)],
                true,
            );
            p.poly(
                &[(0.36, 0.1), (0.64, 0.1), (0.64, 0.94), (0.36, 0.94)],
                true,
            );
            p.line((0.36, 0.73), (0.64, 0.73));
            p.dot((0.36, 0.73));
            p.dot((0.64, 0.73));
        }
        "Make Manifold" => {
            // A box, its open top closed by a dashed edge and a tick.
            p.poly(&[(0.1, 0.4), (0.1, 0.92), (0.62, 0.92), (0.62, 0.4)], false);
            p.dashed((0.1, 0.4), (0.62, 0.4));
            p.line((0.68, 0.26), (0.78, 0.36));
            p.line((0.78, 0.36), (0.94, 0.12));
        }
        "Inspect Manifold" => {
            // A magnifying glass over a box.
            p.poly(&[(0.08, 0.92), (0.08, 0.5), (0.5, 0.5), (0.5, 0.92)], true);
            p.poly(
                &[(0.08, 0.5), (0.22, 0.36), (0.64, 0.36), (0.5, 0.5)],
                false,
            );
            p.circle((0.66, 0.34), 0.2);
            p.line((0.8, 0.48), (0.94, 0.62));
        }
        "Inspect Faces" => {
            // A magnifying glass over a face's corner.
            p.poly(&[(0.08, 0.92), (0.08, 0.45), (0.55, 0.45)], false);
            p.circle((0.58, 0.42), 0.22);
            p.line((0.74, 0.58), (0.92, 0.76));
        }
        "Close Gaps" => {
            // Two lines meeting short of a corner, an arrow closing the gap.
            p.line((0.15, 0.85), (0.15, 0.3));
            p.line((0.3, 0.15), (0.85, 0.15));
            p.dot((0.15, 0.3));
            p.dot((0.3, 0.15));
            p.line((0.15, 0.15), (0.24, 0.24));
            p.dashed((0.15, 0.3), (0.15, 0.15));
            p.dashed((0.3, 0.15), (0.15, 0.15));
        }
        "Clean Up" => {
            // A brush sweeping loose bits off an edge.
            p.line((0.08, 0.88), (0.92, 0.88));
            p.line((0.62, 0.12), (0.42, 0.62));
            p.poly(
                &[(0.3, 0.58), (0.56, 0.68), (0.5, 0.84), (0.22, 0.74)],
                true,
            );
            p.dot((0.78, 0.7));
            p.dot((0.86, 0.56));
        }
        "Point at Percentage" => {
            // A point part-way along an edge, with a tick.
            p.line((0.08, 0.6), (0.92, 0.6));
            p.ellipse((0.35, 0.6), 0.07, 0.07);
            p.line((0.35, 0.2), (0.35, 0.45));
        }
        "Divide" => {
            // An edge cut into equal parts.
            p.line((0.08, 0.5), (0.92, 0.5));
            for x in [0.08f32, 0.36, 0.64, 0.92] {
                p.line((x, 0.38), (x, 0.62));
            }
        }
        "Revolve" => {
            // A profile turned round an axis.
            p.line((0.5, 0.06), (0.5, 0.94));
            p.ellipse((0.5, 0.5), 0.36, 0.14);
            p.line((0.62, 0.3), (0.86, 0.3));
            p.line((0.86, 0.3), (0.86, 0.7));
            p.line((0.86, 0.7), (0.62, 0.7));
            p.line((0.62, 0.7), (0.62, 0.3));
        }
        "Extrude Edges along Path" => {
            // An open edge carried along a curve: a sheet.
            p.curve(|t| (0.12 + 0.76 * t, 0.86 - 0.5 * t * t));
            p.curve(|t| (0.12 + 0.3 * t, 0.86 - 0.5 * (t - 0.5).abs()));
            p.curve(|t| (0.58 + 0.3 * t, 0.36 - 0.5 * (t - 0.5).abs()));
        }
        "Loft" => {
            // A square swelling into a circle with no path between.
            p.line((0.08, 0.72), (0.32, 0.72));
            p.line((0.32, 0.72), (0.32, 0.92));
            p.line((0.32, 0.92), (0.08, 0.92));
            p.line((0.08, 0.92), (0.08, 0.72));
            p.ellipse((0.72, 0.28), 0.16, 0.16);
            p.curve(|t| (0.08 + 0.48 * t, 0.72 - 0.44 * t.powf(0.6)));
            p.curve(|t| (0.32 + 0.56 * t, 0.92 - 0.64 * t.powf(1.6)));
        }
        "Copy Along Path" => {
            p.curve(|t| (0.08 + 0.84 * t, 0.7 - 0.4 * (PI * t).sin()));
            for t in [0.15f32, 0.5, 0.85] {
                let c = (0.08 + 0.84 * t, 0.7 - 0.4 * (PI * t).sin());
                p.square(c, 0.07);
            }
        }
        "Fuse" => {
            // Two overlapping squares, their shared part outlined thick.
            p.square((0.38, 0.38), 0.26);
            p.square((0.62, 0.62), 0.26);
            p.poly(
                &[(0.36, 0.64), (0.64, 0.64), (0.64, 0.36), (0.36, 0.36)],
                true,
            );
            p.line((0.36, 0.5), (0.64, 0.5));
        }
        "Fill" => {
            // An open outline, closed by a dashed edge, the face hatched in.
            p.line((0.15, 0.85), (0.15, 0.15));
            p.line((0.15, 0.15), (0.85, 0.15));
            p.dashed((0.85, 0.15), (0.15, 0.85));
            for t in [0.3f32, 0.45, 0.6] {
                p.line((0.15, t), (t, 0.15));
            }
            p.dot((0.15, 0.85));
            p.dot((0.85, 0.15));
        }
        "Bevel" => {
            p.line((0.15, 0.9), (0.15, 0.55));
            p.arc((0.5, 0.55), 0.35, PI, PI * 1.5);
            p.line((0.5, 0.2), (0.9, 0.2));
        }
        "Tape Measure" => {
            p.line((0.08, 0.6), (0.92, 0.6));
            for i in 0..5 {
                let x = 0.12 + 0.19 * i as f32;
                let h = if i % 2 == 0 { 0.3 } else { 0.42 };
                p.line((x, 0.6), (x, h));
            }
        }
        "Protractor" => {
            p.arc((0.5, 0.78), 0.42, PI, TAU);
            p.line((0.08, 0.78), (0.92, 0.78));
            p.line((0.5, 0.78), (0.75, 0.42));
        }
        "Find Center" => {
            // A circle with crosshairs meeting at its centre.
            p.circle((0.5, 0.5), 0.36);
            p.dashed((0.5, 0.05), (0.5, 0.95));
            p.dashed((0.05, 0.5), (0.95, 0.5));
            p.dot((0.5, 0.5));
        }
        "Dimensions" => {
            p.line((0.1, 0.3), (0.1, 0.8));
            p.line((0.9, 0.3), (0.9, 0.8));
            p.line((0.1, 0.5), (0.9, 0.5));
            p.poly(&[(0.22, 0.42), (0.1, 0.5), (0.22, 0.58)], false);
            p.poly(&[(0.78, 0.42), (0.9, 0.5), (0.78, 0.58)], false);
        }
        "Text" => {
            p.line((0.2, 0.15), (0.8, 0.15));
            p.line((0.5, 0.15), (0.5, 0.65));
            p.line((0.5, 0.75), (0.2, 0.95));
        }
        "Shapes" => {
            // A box in a three-quarter view.
            p.poly(
                &[(0.15, 0.35), (0.6, 0.35), (0.6, 0.85), (0.15, 0.85)],
                true,
            );
            p.poly(
                &[(0.15, 0.35), (0.4, 0.12), (0.85, 0.12), (0.6, 0.35)],
                false,
            );
            p.poly(&[(0.85, 0.12), (0.85, 0.62), (0.6, 0.85)], false);
        }
        "Edit Shape" => {
            // A box with a pencil.
            p.poly(&[(0.1, 0.4), (0.5, 0.4), (0.5, 0.85), (0.1, 0.85)], true);
            p.poly(&[(0.1, 0.4), (0.3, 0.22), (0.7, 0.22), (0.5, 0.4)], false);
            p.line((0.6, 0.75), (0.9, 0.2));
            p.line((0.7, 0.8), (0.95, 0.32));
            p.line((0.6, 0.75), (0.7, 0.8));
        }
        "Edit Extrude" | "Edit Loft" | "Edit Revolve" | "Edit Skin" => {
            // A swept curve with a pencil.
            p.curve(|t| (0.1 + 0.45 * t, 0.85 - 0.55 * t * t));
            p.ellipse((0.1, 0.85), 0.07, 0.05);
            p.ellipse((0.55, 0.3), 0.05, 0.07);
            p.line((0.6, 0.75), (0.9, 0.2));
            p.line((0.7, 0.8), (0.95, 0.32));
            p.line((0.6, 0.75), (0.7, 0.8));
        }
        "Explode" => {
            // A dashed box with its contents bursting out.
            p.dashed_rect((0.3, 0.3), (0.7, 0.7));
            for (a, b) in [
                ((0.3, 0.3), (0.08, 0.08)),
                ((0.7, 0.3), (0.92, 0.08)),
                ((0.3, 0.7), (0.08, 0.92)),
                ((0.7, 0.7), (0.92, 0.92)),
            ] {
                p.line(a, b);
            }
        }
        "Make Unique" => {
            // Two copies, one marked as its own.
            p.poly(&[(0.08, 0.3), (0.48, 0.3), (0.48, 0.7), (0.08, 0.7)], true);
            p.poly(&[(0.52, 0.3), (0.92, 0.3), (0.92, 0.7), (0.52, 0.7)], true);
            p.dot((0.72, 0.5));
        }
        "Lock" => {
            // A closed padlock.
            p.poly(&[(0.2, 0.48), (0.8, 0.48), (0.8, 0.92), (0.2, 0.92)], true);
            p.arc(
                (0.5, 0.48),
                0.22,
                std::f32::consts::PI,
                std::f32::consts::TAU,
            );
            p.dot((0.5, 0.7));
        }
        "Unlock" => {
            // An open padlock: the shackle lifted to one side.
            p.poly(&[(0.2, 0.48), (0.8, 0.48), (0.8, 0.92), (0.2, 0.92)], true);
            p.arc(
                (0.64, 0.3),
                0.18,
                std::f32::consts::PI,
                std::f32::consts::TAU,
            );
            p.line((0.46, 0.3), (0.46, 0.48));
        }
        "Reset Scale" => {
            // A stretched box with an arrow back to its first size.
            p.poly(&[(0.1, 0.4), (0.9, 0.4), (0.9, 0.9), (0.1, 0.9)], true);
            p.dashed_rect((0.1, 0.55), (0.6, 0.9));
            p.line((0.9, 0.2), (0.6, 0.2));
            p.poly(&[(0.7, 0.1), (0.6, 0.2), (0.7, 0.3)], false);
        }
        "Change Axes" => {
            // Axes with a turn about their corner.
            p.line((0.2, 0.85), (0.85, 0.85));
            p.line((0.2, 0.85), (0.2, 0.2));
            p.arc((0.2, 0.85), 0.42, -1.3, -0.25);
            p.dot((0.2, 0.85));
        }
        "Hide" => {
            // An eye, struck through.
            p.ellipse((0.5, 0.5), 0.4, 0.22);
            p.circle((0.5, 0.5), 0.1);
            p.line((0.15, 0.85), (0.85, 0.15));
        }
        "Unhide" => {
            // An open eye.
            p.ellipse((0.5, 0.5), 0.4, 0.22);
            p.circle((0.5, 0.5), 0.1);
            p.dot((0.5, 0.5));
        }
        "Hide Others" => {
            // One item kept, the others dashed away.
            p.poly(
                &[(0.35, 0.35), (0.65, 0.35), (0.65, 0.65), (0.35, 0.65)],
                true,
            );
            p.dashed_rect((0.05, 0.05), (0.25, 0.25));
            p.dashed_rect((0.75, 0.05), (0.95, 0.25));
            p.dashed_rect((0.05, 0.75), (0.25, 0.95));
            p.dashed_rect((0.75, 0.75), (0.95, 0.95));
        }
        "Unhide All" => {
            // An open eye over several items.
            p.ellipse((0.5, 0.38), 0.36, 0.2);
            p.circle((0.5, 0.38), 0.09);
            for x in [0.2, 0.5, 0.8] {
                p.square((x, 0.82), 0.08);
            }
        }
        "Make Group" => {
            p.dashed_rect((0.05, 0.05), (0.95, 0.95));
            p.circle((0.35, 0.38), 0.15);
            p.poly(
                &[(0.5, 0.55), (0.82, 0.55), (0.82, 0.82), (0.5, 0.82)],
                true,
            );
        }
        "Make Component" => {
            p.poly(
                &[(0.15, 0.3), (0.55, 0.3), (0.55, 0.85), (0.15, 0.85)],
                true,
            );
            p.poly(&[(0.15, 0.3), (0.4, 0.12), (0.8, 0.12), (0.55, 0.3)], false);
            p.poly(&[(0.8, 0.12), (0.8, 0.67), (0.55, 0.85)], false);
        }
        "Orbit" => {
            p.ellipse((0.5, 0.5), 0.42, 0.22);
            p.dot((0.5, 0.5));
            p.poly(&[(0.78, 0.18), (0.92, 0.5), (0.68, 0.36)], false);
        }
        "Pan" => {
            p.line((0.5, 0.15), (0.5, 0.85));
            p.line((0.15, 0.5), (0.85, 0.5));
            p.circle((0.5, 0.5), 0.12);
        }
        "Zoom" => {
            p.circle((0.42, 0.42), 0.28);
            p.line((0.62, 0.62), (0.9, 0.9));
        }
        "Zoom Extents" => {
            p.circle((0.45, 0.45), 0.22);
            p.line((0.61, 0.61), (0.8, 0.8));
            for (a, b, c) in [
                ((0.05, 0.25), (0.05, 0.05), (0.25, 0.05)),
                ((0.75, 0.05), (0.95, 0.05), (0.95, 0.25)),
                ((0.05, 0.75), (0.05, 0.95), (0.25, 0.95)),
            ] {
                p.poly(&[a, b, c], false);
            }
        }
        "Paint" => {
            p.poly(&[(0.2, 0.4), (0.55, 0.1), (0.85, 0.45), (0.5, 0.8)], true);
            p.curve(|t| (0.2 - 0.08 * (PI * t).sin(), 0.4 + 0.45 * t));
        }
        "Mark Edges" => {
            p.curve(|t| (0.08 + 0.42 * t, 0.8 - 0.5 * (FRAC_PI_2 * t).sin()));
            p.line((0.5, 0.3), (0.92, 0.8));
        }
        "Line Style" => {
            // A solid line, a dashed one and a dash-dot one, under each other.
            p.line((0.1, 0.22), (0.9, 0.22));
            for i in 0..3 {
                let x = 0.1 + 0.28 * f32::from(i as u8);
                p.line((x, 0.5), (x + 0.18, 0.5));
            }
            p.line((0.1, 0.78), (0.5, 0.78));
            p.line((0.6, 0.78), (0.64, 0.78));
            p.line((0.74, 0.78), (0.9, 0.78));
        }
        "Section Plane" => {
            p.poly(
                &[(0.15, 0.25), (0.65, 0.1), (0.65, 0.75), (0.15, 0.9)],
                true,
            );
            p.line((0.4, 0.5), (0.92, 0.5));
            p.poly(&[(0.8, 0.4), (0.92, 0.5), (0.8, 0.6)], false);
        }
        "Save Render" => {
            p.poly(
                &[(0.08, 0.25), (0.75, 0.25), (0.75, 0.8), (0.08, 0.8)],
                true,
            );
            p.poly(
                &[(0.75, 0.42), (0.95, 0.3), (0.95, 0.75), (0.75, 0.63)],
                false,
            );
            p.line((0.42, 0.38), (0.42, 0.68));
            p.line((0.27, 0.53), (0.57, 0.53));
        }
        "Axes" => {
            p.line((0.2, 0.8), (0.9, 0.8));
            p.line((0.2, 0.8), (0.2, 0.1));
            p.line((0.2, 0.8), (0.65, 0.4));
            p.dot((0.2, 0.8));
        }
        "Select Connected" => {
            p.poly(&[(0.08, 0.3), (0.5, 0.3), (0.5, 0.7), (0.08, 0.7)], true);
            p.poly(&[(0.5, 0.3), (0.92, 0.3), (0.92, 0.7), (0.5, 0.7)], true);
            p.dot((0.29, 0.5));
            p.dot((0.71, 0.5));
        }
        "Select Vertices" => {
            // Four points, nothing joining them.
            for c in [(0.2, 0.2), (0.8, 0.2), (0.8, 0.8), (0.2, 0.8)] {
                p.blob(c, 0.09);
            }
        }
        "Select Edges" => {
            // The same four points, joined.
            p.poly(&[(0.2, 0.2), (0.8, 0.2), (0.8, 0.8), (0.2, 0.8)], true);
            for c in [(0.2, 0.2), (0.8, 0.2), (0.8, 0.8), (0.2, 0.8)] {
                p.blob(c, 0.09);
            }
        }
        "Select Faces" => {
            // A face alone, no points.
            p.face(&[(0.2, 0.2), (0.8, 0.2), (0.8, 0.8), (0.2, 0.8)]);
        }
        "Select All" => {
            // Everything inside the selection box.
            p.dashed_rect((0.08, 0.08), (0.92, 0.92));
            p.face(&[(0.28, 0.28), (0.72, 0.28), (0.72, 0.72), (0.28, 0.72)]);
        }
        "Select None" => {
            // An empty selection box.
            p.dashed_rect((0.08, 0.08), (0.92, 0.92));
            p.line((0.3, 0.7), (0.7, 0.3));
        }
        "Cut" => {
            // Scissors.
            p.circle((0.27, 0.75), 0.14);
            p.circle((0.73, 0.75), 0.14);
            p.line((0.36, 0.64), (0.72, 0.1));
            p.line((0.64, 0.64), (0.28, 0.1));
        }
        "Copy" => {
            // Two sheets, one over the other.
            p.poly(
                &[(0.12, 0.12), (0.6, 0.12), (0.6, 0.66), (0.12, 0.66)],
                true,
            );
            p.poly(
                &[(0.4, 0.34), (0.88, 0.34), (0.88, 0.88), (0.4, 0.88)],
                true,
            );
        }
        "Paste" => {
            // A clipboard with a sheet on it.
            p.poly(
                &[(0.16, 0.14), (0.84, 0.14), (0.84, 0.92), (0.16, 0.92)],
                true,
            );
            p.poly(
                &[(0.36, 0.06), (0.64, 0.06), (0.64, 0.2), (0.36, 0.2)],
                true,
            );
            p.poly(&[(0.3, 0.36), (0.7, 0.36), (0.7, 0.8), (0.3, 0.8)], true);
        }
        "Select by Type" => {
            p.circle((0.28, 0.28), 0.17);
            p.poly(
                &[(0.56, 0.12), (0.88, 0.12), (0.88, 0.44), (0.56, 0.44)],
                true,
            );
            p.poly(&[(0.1, 0.88), (0.28, 0.56), (0.46, 0.88)], true);
            p.line((0.56, 0.88), (0.88, 0.56));
        }
        "Undo" => {
            p.poly(
                &[(0.85, 0.8), (0.85, 0.5), (0.65, 0.35), (0.15, 0.35)],
                false,
            );
            p.line((0.15, 0.35), (0.35, 0.15));
            p.line((0.15, 0.35), (0.35, 0.55));
        }
        "Redo" => {
            p.poly(
                &[(0.15, 0.8), (0.15, 0.5), (0.35, 0.35), (0.85, 0.35)],
                false,
            );
            p.line((0.85, 0.35), (0.65, 0.15));
            p.line((0.85, 0.35), (0.65, 0.55));
        }
        _ => return false,
    }
    true
}

/// Unit-square drawing helper.
struct Pen<'a> {
    painter: &'a Painter,
    rect: Rect,
    stroke: Stroke,
    color: Color32,
}

impl Pen<'_> {
    fn at(&self, (x, y): (f32, f32)) -> Pos2 {
        pos2(
            self.rect.min.x + x * self.rect.width(),
            self.rect.min.y + y * self.rect.height(),
        )
    }

    fn scale(&self, r: f32) -> f32 {
        r * self.rect.width()
    }

    fn line(&self, a: (f32, f32), b: (f32, f32)) {
        self.painter
            .line_segment([self.at(a), self.at(b)], self.stroke);
    }

    fn poly(&self, pts: &[(f32, f32)], closed: bool) {
        let mut v: Vec<Pos2> = pts.iter().map(|p| self.at(*p)).collect();
        if closed && let Some(first) = v.first().copied() {
            v.push(first);
        }
        self.painter.add(egui::Shape::line(v, self.stroke));
    }

    fn dot(&self, c: (f32, f32)) {
        self.painter
            .circle_filled(self.at(c), self.stroke.width * 1.4, self.color);
    }

    /// A filled point of radius `r` (unit square).
    fn blob(&self, c: (f32, f32), r: f32) {
        self.painter
            .circle_filled(self.at(c), self.scale(r), self.color);
    }

    /// A shaded face with its outline (convex points, unit square).
    fn face(&self, pts: &[(f32, f32)]) {
        let v: Vec<Pos2> = pts.iter().map(|p| self.at(*p)).collect();
        self.painter.add(egui::Shape::convex_polygon(
            v,
            self.color.gamma_multiply(0.45),
            self.stroke,
        ));
    }

    fn square(&self, c: (f32, f32), h: f32) {
        self.poly(
            &[
                (c.0 - h, c.1 - h),
                (c.0 + h, c.1 - h),
                (c.0 + h, c.1 + h),
                (c.0 - h, c.1 + h),
            ],
            true,
        );
    }

    fn circle(&self, c: (f32, f32), r: f32) {
        self.painter
            .circle_stroke(self.at(c), self.scale(r), self.stroke);
    }

    fn arc(&self, c: (f32, f32), r: f32, a0: f32, a1: f32) {
        self.curve(|t| {
            let a = a0 + (a1 - a0) * t;
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        });
    }

    fn ellipse(&self, c: (f32, f32), rx: f32, ry: f32) {
        self.curve(|t| {
            let a = TAU * t;
            (c.0 + rx * a.cos(), c.1 + ry * a.sin())
        });
    }

    fn ellipse_thick(&self, c: (f32, f32), rx: f32, ry: f32) {
        let v: Vec<Pos2> = (0..=32)
            .map(|i| {
                let a = TAU * i as f32 / 32.0;
                self.at((c.0 + rx * a.cos(), c.1 + ry * a.sin()))
            })
            .collect();
        self.painter.add(egui::Shape::line(
            v,
            Stroke::new(self.stroke.width * 1.8, self.color),
        ));
    }

    fn curve(&self, f: impl Fn(f32) -> (f32, f32)) {
        let v: Vec<Pos2> = (0..=24).map(|i| self.at(f(i as f32 / 24.0))).collect();
        self.painter.add(egui::Shape::line(v, self.stroke));
    }

    fn dashed(&self, a: (f32, f32), b: (f32, f32)) {
        self.painter.extend(egui::Shape::dashed_line(
            &[self.at(a), self.at(b)],
            self.stroke,
            self.scale(0.1),
            self.scale(0.07),
        ));
    }

    fn dashed_rect(&self, a: (f32, f32), b: (f32, f32)) {
        let pts = [
            self.at(a),
            self.at((b.0, a.1)),
            self.at(b),
            self.at((a.0, b.1)),
            self.at(a),
        ];
        self.painter.extend(egui::Shape::dashed_line(
            &pts,
            self.stroke,
            self.scale(0.1),
            self.scale(0.07),
        ));
    }
}

/// Every icon name (used to choose icon or text label, and by the coverage test).
pub const NAMES_SET: &[&str] = NAMES;

/// Every icon name.
pub const NAMES: &[&str] = &[
    "Select",
    "Select Connected",
    "Select by Type",
    "Select Vertices",
    "Select Edges",
    "Select Faces",
    "Select All",
    "Select None",
    "Cut",
    "Copy",
    "Paste",
    "Undo",
    "Redo",
    "Line",
    "Rectangle",
    "Rotated Rectangle",
    "Circle",
    "Arc",
    "Polygon",
    "Bézier Spline",
    "Curve Maker",
    "Shapes",
    "Extrude",
    "Move",
    "Rotate",
    "Scale",
    "Offset (Inset)",
    "Eraser",
    "Mirror",
    "Loop Cut",
    "Slide",
    "Dissolve",
    "Knife",
    "Extrude along Path",
    "Revolve",
    "Divide",
    "Weld",
    "Point at Percentage",
    "Trim / Extend",
    "Extend to Face",
    "Bridge",
    "Bisector",
    "Space",
    "Circle Loop",
    "Flatten",
    "Smooth",
    "Relax",
    "Intersect Faces",
    "Clean Up",
    "Close Gaps",
    "Inspect Faces",
    "Inspect Manifold",
    "Make Manifold",
    "Extrude Edges along Path",
    "Loft along Path",
    "Loft",
    "Skinning",
    "Copy Along Path",
    "Bevel",
    "Fill",
    "Fuse",
    "Tape Measure",
    "Protractor",
    "Find Center",
    "Dimensions",
    "Text",
    "Make Group",
    "Make Component",
    "Edit Shape",
    "Edit Extrude",
    "Edit Loft",
    "Edit Skin",
    "Edit Revolve",
    "Explode",
    "Make Unique",
    "Lock",
    "Unlock",
    "Reset Scale",
    "Change Axes",
    "Hide",
    "Unhide",
    "Hide Others",
    "Unhide All",
    "Orbit",
    "Pan",
    "Zoom",
    "Zoom Extents",
    "Paint",
    "Mark Edges",
    "Line Style",
    "Section Plane",
    "Save Render",
    "Axes",
];
