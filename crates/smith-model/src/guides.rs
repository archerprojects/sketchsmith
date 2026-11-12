//! SketchSmith — guides | v1.0.0 | Thursday 12 November 2026
//!
//! Guides are construction geometry: endless guide lines and single guide
//! points that the measuring tools leave behind for placing geometry
//! exactly. They are drawn dashed and offered as snaps, but they are not
//! geometry: they never split edges, bound faces or appear in exports.
//!
//! Each context keeps its own guides, alongside its vertices, edges and
//! faces, so a guide made inside an open group belongs to the group. They
//! are journaled like every other entity: adding or erasing a guide is one
//! undo step.

use smith_geom::{DMat4, DVec3};

use crate::entities::Entities;
use crate::error::ModelError;
use crate::ids::GuideId;

/// A guide line or guide point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Guide {
    /// An endless line through `point` along unit `direction`.
    Line {
        /// A point on the line.
        point: DVec3,
        /// Unit direction.
        direction: DVec3,
    },
    /// A single point.
    Point {
        /// The point.
        point: DVec3,
    },
}

impl Guide {
    /// A guide line through `point` along `direction` (normalised). `None`
    /// for a zero or non-finite direction or a non-finite point.
    pub fn line(point: DVec3, direction: DVec3) -> Option<Self> {
        let direction = direction.try_normalize()?;
        point
            .is_finite()
            .then_some(Guide::Line { point, direction })
    }

    /// A guide point. `None` for a non-finite point.
    pub fn point(point: DVec3) -> Option<Self> {
        point.is_finite().then_some(Guide::Point { point })
    }

    /// Whether the guide is well formed: finite, and a line's direction a
    /// unit vector (within 1e-9).
    pub fn is_valid(&self) -> bool {
        match self {
            Guide::Line { point, direction } => {
                point.is_finite()
                    && direction.is_finite()
                    && (direction.length() - 1.0).abs() <= 1e-9
            }
            Guide::Point { point } => point.is_finite(),
        }
    }

    /// The guide's anchor: a guide line's stored point, or the guide point.
    pub fn anchor(&self) -> DVec3 {
        match self {
            Guide::Line { point, .. } | Guide::Point { point } => *point,
        }
    }

    /// The point of the guide nearest to `p`.
    pub fn closest(&self, p: DVec3) -> DVec3 {
        match self {
            Guide::Line { point, direction } => *point + *direction * (p - *point).dot(*direction),
            Guide::Point { point } => *point,
        }
    }

    /// Distance from `p` to the guide, mm.
    pub fn distance(&self, p: DVec3) -> f64 {
        self.closest(p).distance(p)
    }

    /// The guide moved by transform `m`. A line whose direction `m`
    /// collapses (a zero scale) becomes a guide point at its anchor.
    pub fn transformed(&self, m: &DMat4) -> Self {
        match self {
            Guide::Line { point, direction } => {
                let p = m.transform_point3(*point);
                Guide::line(p, m.transform_vector3(*direction)).unwrap_or(Guide::Point { point: p })
            }
            Guide::Point { point } => Guide::Point {
                point: m.transform_point3(*point),
            },
        }
    }
}

impl Entities {
    /// Number of guides.
    pub fn guide_count(&self) -> usize {
        self.guides.len()
    }

    /// All guide keys, in creation order.
    pub fn guide_ids(&self) -> impl Iterator<Item = GuideId> + '_ {
        self.guides.keys()
    }

    /// A guide by key.
    pub fn guide(&self, id: GuideId) -> Option<&Guide> {
        self.guides.get(id)
    }

    /// Every guide with its key, in creation order.
    pub fn guides(&self) -> impl Iterator<Item = (GuideId, &Guide)> + '_ {
        self.guides.iter()
    }

    /// Adds a guide.
    ///
    /// # Errors
    /// [`ModelError::InvalidData`] for a malformed guide.
    pub fn add_guide(&mut self, guide: Guide) -> Result<GuideId, ModelError> {
        if !guide.is_valid() {
            return Err(ModelError::InvalidData("malformed guide".into()));
        }
        Ok(self.guides.insert(guide))
    }

    /// Erases guide `id` and returns it.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn remove_guide(&mut self, id: GuideId) -> Result<Guide, ModelError> {
        self.guides.remove(id).ok_or(ModelError::NoSuchEntity)
    }

    /// Erases every guide; returns how many there were.
    pub fn clear_guides(&mut self) -> usize {
        let ids: Vec<GuideId> = self.guides.keys().collect();
        for id in &ids {
            self.guides.remove(*id);
        }
        ids.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    #[test]
    fn line_direction_is_normalised_and_zero_refused() {
        let g = Guide::line(DVec3::ZERO, DVec3::new(0.0, 3.0, 0.0)).unwrap();
        assert_eq!(
            g,
            Guide::Line {
                point: DVec3::ZERO,
                direction: DVec3::Y
            }
        );
        assert!(Guide::line(DVec3::ZERO, DVec3::ZERO).is_none());
        assert!(Guide::point(DVec3::new(f64::NAN, 0.0, 0.0)).is_none());
    }

    #[test]
    fn distance_to_a_line_is_perpendicular() {
        let g = Guide::line(DVec3::new(0.0, 2.5, 0.0), DVec3::X).unwrap();
        assert_eq!(g.distance(DVec3::new(40.0, 0.0, 0.0)), 2.5);
        assert_eq!(
            g.closest(DVec3::new(7.0, 9.0, 0.0)),
            DVec3::new(7.0, 2.5, 0.0)
        );
    }

    #[test]
    fn malformed_guides_are_refused() {
        let mut e = Entities::new();
        let bad = Guide::Line {
            point: DVec3::ZERO,
            direction: DVec3::new(2.0, 0.0, 0.0),
        };
        assert!(e.add_guide(bad).is_err());
        assert_eq!(e.guide_count(), 0);
    }

    #[test]
    fn adding_and_erasing_guides_undo() {
        let mut doc = Document::new();
        doc.transaction("Guide", |d| {
            d.root_mut()
                .add_guide(Guide::point(DVec3::new(1.0, 2.0, 3.0)).unwrap())
                .map(|_| ())
        })
        .unwrap();
        assert_eq!(doc.root().guide_count(), 1);
        doc.transaction("Delete Guides", |d| {
            d.root_mut().clear_guides();
            Ok(())
        })
        .unwrap();
        assert_eq!(doc.root().guide_count(), 0);
        assert!(doc.undo().is_some());
        assert_eq!(doc.root().guide_count(), 1);
        assert!(doc.undo().is_some());
        assert_eq!(doc.root().guide_count(), 0);
        assert!(doc.redo().is_some());
        assert_eq!(doc.root().guide_count(), 1);
    }

    #[test]
    fn a_groups_guides_are_laid_into_the_snap_view_but_not_erasable() {
        use crate::document::DefinitionKind;
        use crate::selection::Pickable;
        use smith_geom::DMat4;
        let mut doc = Document::new();
        doc.transaction("Group", |d| {
            let def = d.create_definition("Jig", DefinitionKind::Group);
            let g = d.context_entities_mut(Some(def)).expect("def");
            g.add_guide(Guide::point(DVec3::new(1.0, 0.0, 0.0)).expect("point"))?;
            d.add_instance(
                None,
                def,
                DMat4::from_translation(DVec3::new(10.0, 0.0, 0.0)),
                "",
            )?;
            Ok(())
        })
        .unwrap();
        let view = doc.snap_view(None).unwrap();
        let (id, g) = view.entities.guides().next().unwrap();
        assert_eq!(*g, Guide::point(DVec3::new(11.0, 0.0, 0.0)).unwrap());
        assert!(view.is_outside(Pickable::Guide(id)));
    }

    #[test]
    fn transformed_line_keeps_a_unit_direction() {
        let g = Guide::line(DVec3::ZERO, DVec3::X).unwrap();
        let m = DMat4::from_scale(DVec3::new(3.0, 1.0, 1.0));
        assert!(g.transformed(&m).is_valid());
        let flat = DMat4::from_scale(DVec3::new(0.0, 1.0, 1.0));
        assert!(matches!(g.transformed(&flat), Guide::Point { .. }));
    }
}
