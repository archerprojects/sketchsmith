//! SketchSmith — dimensions and text in drawings | v1.0.0 | Thursday 12 November 2026
//!
//! The dimensions and leader text of what a drawing takes, worked out as
//! the canvas draws them and projected into the drawing plane: Linear
//! (extension lines and the dimension line, an arrow at each end), Radial
//! (`R` or `Ø`, the line from the centre or across, an arrow at the rim),
//! Angular (the arc and its arms) and Leader (the leader line, an arrow at
//! what it points at). Screen text belongs to the canvas and is left out.
//!
//! Lines are in drawing-plane millimetres like the model; arrowheads and
//! text are only placed here, and sized on paper. Figures are the true 3D
//! values, written by the caller's own unit formatting, so they read as on
//! screen. Marks are never hidden.

use glam::{DMat3, DMat4, DVec2, DVec3};
use smith_model::{Annotation, Document, Entities, InstanceId, LeaderText};

use super::{DrawingView, Scope};

/// A value a mark shows, for the caller to write in its units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Measure {
    /// A length, mm.
    Length(f64),
    /// An area, mm².
    Area(f64),
    /// An angle, degrees.
    Angle(f64),
}

/// Writes a measure as the drawing shows it.
pub type Labels<'a> = &'a dyn Fn(Measure) -> String;

/// One dimension or leader text in the drawing plane, mm.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mark {
    /// Its lines.
    pub lines: Vec<[DVec2; 2]>,
    /// Its arrowheads: the tip, and a point the arrow comes from.
    pub arrows: Vec<[DVec2; 2]>,
    /// Its text, centred on a point.
    pub text: Option<(DVec2, String)>,
}

/// The dimensions and leader text `scope` takes of `doc`, seen from `view`.
pub fn marks(
    doc: &Document,
    scope: Scope<'_>,
    view: &DrawingView,
    labels: Labels<'_>,
) -> Vec<Mark> {
    let flat = view.flatten();
    let mut out = Vec::new();
    let mut walk = |e: &Entities, world: DMat4| {
        for (_, a) in e.annotations() {
            if let Some(m) = mark(e, a, world, &flat, labels) {
                out.push(m);
            }
        }
    };
    match scope {
        Scope::Model => {
            walk(doc.root(), DMat4::IDENTITY);
            let all: Vec<InstanceId> = doc.root().instance_ids().collect();
            instances(doc, doc.root(), &all, DMat4::IDENTITY, &mut walk);
        }
        Scope::Selection {
            context,
            local,
            instances: picked,
            ..
        } => {
            let entities = match context {
                None => Some(doc.root()),
                Some(id) => doc.definition_entities(id),
            };
            if let Some(e) = entities {
                walk(e, local);
                instances(doc, e, picked, local, &mut walk);
            }
        }
    }
    out
}

/// Walks the shown `ids` of `e` placed by `parent`, and everything in them.
fn instances(
    doc: &Document,
    e: &Entities,
    ids: &[InstanceId],
    parent: DMat4,
    walk: &mut impl FnMut(&Entities, DMat4),
) {
    for id in ids {
        let Some(inst) = e.instance(*id) else {
            continue;
        };
        if inst.hidden || !doc.layer_visible(inst.layer) {
            continue;
        }
        let Some(inner) = doc.definition_entities(inst.definition) else {
            continue;
        };
        let world = parent * inst.transform;
        walk(inner, world);
        let nested: Vec<InstanceId> = inner.instance_ids().collect();
        instances(doc, inner, &nested, world, walk);
    }
}

/// One annotation of `e` (placed by `world`) as a mark, worked out as the
/// canvas does; `None` for screen text and for angles with no arms.
fn mark(
    e: &Entities,
    a: &Annotation,
    world: DMat4,
    flat: &impl Fn(DVec3) -> DVec2,
    labels: Labels<'_>,
) -> Option<Mark> {
    let at = |p: DVec3| flat(world.transform_point3(p));
    let mut m = Mark::default();
    let line = |p: DVec3, q: DVec3, m: &mut Mark| m.lines.push([at(p), at(q)]);
    match a {
        Annotation::Linear { a, b, offset } => {
            let (pa, pb) = (a.position(e), b.position(e));
            let (qa, qb) = (pa + *offset, pb + *offset);
            line(pa, qa, &mut m);
            line(pb, qb, &mut m);
            line(qa, qb, &mut m);
            m.arrows.push([at(qa), at(qb)]);
            m.arrows.push([at(qb), at(qa)]);
            // The true length, measured in the model's own space.
            let true_len = world
                .transform_point3(pa)
                .distance(world.transform_point3(pb));
            m.text = Some((at((qa + qb) * 0.5), labels(Measure::Length(true_len))));
        }
        Annotation::Radial {
            curve,
            diameter,
            centre,
            radius,
            label,
        } => {
            let (c, r, n) = curve
                .and_then(|k| e.curve(k))
                .and_then(|rec| match rec.curve() {
                    smith_geom::curve::Curve::Circle(x) => {
                        Some((x.frame().center(), x.radius(), x.frame().normal()))
                    }
                    smith_geom::curve::Curve::Arc(x) => {
                        Some((x.frame().center(), x.radius(), x.frame().normal()))
                    }
                    _ => None,
                })
                .unwrap_or((*centre, *radius, DVec3::Z));
            let l = c + *label;
            let d = (*label - n * label.dot(n)).normalize_or(DVec3::X);
            let rim = c + d * r;
            let start = if *diameter { c - d * r } else { c };
            line(start, rim, &mut m);
            line(rim, l, &mut m);
            m.arrows.push([at(rim), at(start)]);
            let true_r = world
                .transform_point3(rim)
                .distance(world.transform_point3(c));
            m.text = Some((
                at(l),
                if *diameter {
                    format!("Ø {}", labels(Measure::Length(2.0 * true_r)))
                } else {
                    format!("R {}", labels(Measure::Length(true_r)))
                },
            ));
        }
        Annotation::Angular {
            centre,
            a,
            b,
            radius,
        } => {
            let c = centre.position(e);
            let (da, db) = (a.position(e) - c, b.position(e) - c);
            let (ua, ub, n) = (
                da.try_normalize()?,
                db.try_normalize()?,
                da.cross(db).try_normalize()?,
            );
            let angle = ua.cross(ub).dot(n).atan2(ua.dot(ub));
            let steps = 24;
            let arc = |t: f64| c + DMat3::from_axis_angle(n, angle * t) * ua * *radius;
            for i in 0..steps {
                let (t0, t1) = (
                    f64::from(i) / f64::from(steps),
                    f64::from(i + 1) / f64::from(steps),
                );
                line(arc(t0), arc(t1), &mut m);
            }
            line(c, c + ua * *radius, &mut m);
            line(c, c + ub * *radius, &mut m);
            m.text = Some((at(arc(0.5)), labels(Measure::Angle(angle.to_degrees()))));
        }
        Annotation::Leader {
            anchor,
            label,
            text,
        } => {
            // The text stays where it was placed; the leader stretches to
            // the anchor.
            let p = anchor.position(e);
            let l = anchor.fallback() + *label;
            line(p, l, &mut m);
            m.arrows.push([at(p), at(l)]);
            let words = match text {
                LeaderText::Custom(t) => t.clone(),
                LeaderText::Length { edge, value } => labels(Measure::Length(
                    edge.and_then(|k| e.edge_points(k))
                        .map_or(*value, |(a, b)| {
                            world
                                .transform_point3(a)
                                .distance(world.transform_point3(b))
                        }),
                )),
                LeaderText::Area { face, value } => labels(Measure::Area(
                    face.and_then(|f| e.face_area(f)).unwrap_or(*value),
                )),
                LeaderText::Coordinates => {
                    let w = world.transform_point3(p);
                    format!(
                        "{}, {}, {}",
                        labels(Measure::Length(w.x)),
                        labels(Measure::Length(w.y)),
                        labels(Measure::Length(w.z))
                    )
                }
            };
            m.text = Some((at(l), words));
        }
        Annotation::Screen { .. } => return None,
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith_model::{Anchor, ModelError};

    fn mm(m: Measure) -> String {
        match m {
            Measure::Length(v) => format!("{v:.1}"),
            Measure::Area(v) => format!("{v:.1} mm²"),
            Measure::Angle(v) => format!("{v:.0}°"),
        }
    }

    fn front() -> DrawingView {
        DrawingView {
            centre: DVec3::ZERO,
            direction: DVec3::Y,
            up: DVec3::Z,
        }
    }

    #[test]
    fn a_linear_dimension_projects_with_its_true_length() {
        let mut doc = Document::new();
        doc.transaction("dim", |d| {
            d.root_mut().add_annotation(Annotation::Linear {
                a: Anchor::Point(DVec3::ZERO),
                b: Anchor::Point(DVec3::new(50.0, 0.0, 0.0)),
                offset: DVec3::new(0.0, 0.0, 10.0),
            })?;
            d.root_mut().add_annotation(Annotation::Screen {
                at: [0.5, 0.5],
                text: "Note".into(),
            })?;
            Ok::<(), ModelError>(())
        })
        .unwrap();
        let ms = marks(&doc, Scope::Model, &front(), &mm);
        assert_eq!(ms.len(), 1);
        let m = &ms[0];
        assert_eq!(m.lines.len(), 3);
        assert_eq!(m.arrows.len(), 2);
        let (at, text) = m.text.clone().unwrap();
        assert_eq!(text, "50.0");
        assert!((at - DVec2::new(25.0, 10.0)).length() < 1e-9);
        // Seen end-on along X the line shrinks, but the figure stays true.
        let side = DrawingView {
            centre: DVec3::ZERO,
            direction: DVec3::new(1.0, 1.0, 0.0),
            up: DVec3::Z,
        };
        let m = &marks(&doc, Scope::Model, &side, &mm)[0];
        assert_eq!(m.text.as_ref().unwrap().1, "50.0");
        let [a, b] = m.lines[2];
        assert!((a.distance(b) - 50.0 / 2f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn radial_angular_and_leader() {
        let mut doc = Document::new();
        doc.transaction("dims", |d| {
            let r = d.root_mut();
            r.add_annotation(Annotation::Radial {
                curve: None,
                diameter: true,
                centre: DVec3::ZERO,
                radius: 5.0,
                label: DVec3::new(20.0, 0.0, 0.0),
            })?;
            r.add_annotation(Annotation::Angular {
                centre: Anchor::Point(DVec3::ZERO),
                a: Anchor::Point(DVec3::X),
                b: Anchor::Point(DVec3::Z),
                radius: 10.0,
            })?;
            r.add_annotation(Annotation::Leader {
                anchor: Anchor::Point(DVec3::new(1.0, 0.0, 2.0)),
                label: DVec3::new(5.0, 0.0, 5.0),
                text: LeaderText::Coordinates,
            })?;
            Ok::<(), ModelError>(())
        })
        .unwrap();
        let ms = marks(&doc, Scope::Model, &front(), &mm);
        assert_eq!(ms[0].text.as_ref().unwrap().1, "Ø 10.0");
        assert_eq!(ms[1].text.as_ref().unwrap().1, "90°");
        assert_eq!(ms[1].lines.len(), 26);
        assert_eq!(ms[2].text.as_ref().unwrap().1, "1.0, 0.0, 2.0");
        assert_eq!(ms[2].arrows[0][0], DVec2::new(1.0, 2.0));
    }
}
