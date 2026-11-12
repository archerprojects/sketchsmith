//! SketchSmith — scenes | v1.0.0 | Thursday 12 November 2026
//!
//! A scene is a saved way of looking at the model: the camera, the mode
//! (Edit, or Present with its look), that mode's display style, which
//! layers are shown, and which of the root's section planes is active. On
//! screen scenes are called renders: Save Render adds one, and each is one
//! saved version of how the item is shown. Restoring
//! a scene puts all three back. Scenes belong to the document, in the order
//! they were made; adding, updating, renaming and removing them are undo
//! steps like any other change.
//!
//! A scene lists the layers it hides rather than those it shows, so a layer
//! added after the scene was made is shown when it is restored. A removed
//! layer simply drops out of the list.

use smith_geom::DVec3;

use crate::document::Document;
use crate::error::ModelError;
use crate::ids::{LayerId, SceneId, SectionId};

/// A scene's camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneView {
    /// Point looked at and orbited around.
    pub target: DVec3,
    /// Eye-to-target distance, mm.
    pub distance: f64,
    /// Rotation about +Z, radians.
    pub yaw: f64,
    /// Elevation above the XY plane, radians.
    pub pitch: f64,
    /// Perspective (`true`) or parallel projection.
    pub perspective: bool,
    /// Vertical field of view (perspective), radians.
    pub fov_y: f64,
}

impl SceneView {
    /// Whether every number is finite and the distance and field of view
    /// positive.
    pub fn is_valid(&self) -> bool {
        self.target.is_finite()
            && self.distance.is_finite()
            && self.distance > 0.0
            && self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.fov_y.is_finite()
            && self.fov_y > 0.0
    }
}

/// How faces are drawn, as a scene records it (the display styles both
/// modes share).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FaceDisplay {
    /// Edges only.
    Wireframe,
    /// Faces in the background colour, hiding what is behind them.
    HiddenLine,
    /// Front and back colours, lit.
    #[default]
    Shaded,
    /// One neutral colour, lit.
    Monochrome,
    /// See-through faces.
    XRay,
}

/// A scene's display style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SceneStyle {
    /// How faces are drawn.
    pub faces: FaceDisplay,
    /// Edges behind faces drawn dashed.
    pub back_edges: bool,
}

/// A saved view.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene<L = LayerId, S = SectionId> {
    /// Its name, as listed.
    pub name: String,
    /// The camera.
    pub view: SceneView,
    /// Present mode (`false`: Edit mode).
    pub present: bool,
    /// That mode's display style.
    pub style: SceneStyle,
    /// The layers it hides.
    pub hidden_layers: Vec<L>,
    /// The root's section plane it makes active (`None`: none).
    pub section: Option<S>,
}

/// A scene as plain data: hidden layers are indices into the document's
/// layers.
pub type SceneData = Scene<u32, u32>;

impl Document {
    /// Every scene with its key, in the order they were made.
    pub fn scenes(&self) -> impl Iterator<Item = (SceneId, &Scene)> + '_ {
        self.scenes.iter()
    }

    /// A scene by key.
    pub fn scene(&self, id: SceneId) -> Option<&Scene> {
        self.scenes.get(id)
    }

    /// Adds a scene (call within a transaction for undo).
    ///
    /// # Errors
    /// [`ModelError::InvalidData`] for an empty name or a bad camera.
    pub fn add_scene(&mut self, scene: Scene) -> Result<SceneId, ModelError> {
        check(&scene)?;
        Ok(self.scenes.insert(scene))
    }

    /// Replaces scene `id` (update or rename).
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`], or as [`Document::add_scene`].
    pub fn set_scene(&mut self, id: SceneId, scene: Scene) -> Result<(), ModelError> {
        check(&scene)?;
        *self.scenes.get_mut(id).ok_or(ModelError::NoSuchEntity)? = scene;
        Ok(())
    }

    /// Removes scene `id`.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn remove_scene(&mut self, id: SceneId) -> Result<Scene, ModelError> {
        self.scenes.remove(id).ok_or(ModelError::NoSuchEntity)
    }

    /// The next free scene name: "Render 1", "Render 2", …
    pub fn next_scene_name(&self) -> String {
        let n = (1..)
            .find(|n| {
                let name = format!("Render {n}");
                !self.scenes.iter().any(|(_, s)| s.name == name)
            })
            .unwrap_or(1);
        format!("Render {n}")
    }

    /// Shows and hides layers as scene `id` says (the layers it lists are
    /// hidden, every other layer shown) and makes its section plane the
    /// root's active one. Call within a transaction.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn apply_scene_layers(&mut self, id: SceneId) -> Result<(), ModelError> {
        let hidden = self
            .scenes
            .get(id)
            .ok_or(ModelError::NoSuchEntity)?
            .hidden_layers
            .clone();
        let ids: Vec<LayerId> = self.layer_ids().collect();
        for l in ids {
            if let Some(mut layer) = self.layer(l).cloned() {
                let visible = !hidden.contains(&l);
                if layer.visible != visible {
                    layer.visible = visible;
                    self.set_layer(l, layer)?;
                }
            }
        }
        let section = self.scenes.get(id).and_then(|s| s.section);
        let section = section.filter(|k| self.root().section(*k).is_some());
        if self.root().active_section().map(|(k, _)| k) != section {
            self.root_mut().set_active_section(section)?;
        }
        Ok(())
    }

    /// The layers hidden now, for a new scene.
    pub fn hidden_layers(&self) -> Vec<LayerId> {
        self.layer_ids()
            .filter(|l| self.layer(*l).is_some_and(|x| !x.visible))
            .collect()
    }
}

fn check<L, S>(scene: &Scene<L, S>) -> Result<(), ModelError> {
    if scene.name.trim().is_empty() || !scene.view.is_valid() {
        return Err(ModelError::InvalidData("malformed scene".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> SceneView {
        SceneView {
            target: DVec3::ZERO,
            distance: 100.0,
            yaw: 0.3,
            pitch: 0.4,
            perspective: true,
            fov_y: 0.8,
        }
    }

    #[test]
    fn a_scene_restores_its_layers_and_adding_it_undoes() {
        let mut doc = Document::new();
        let red = doc
            .transaction("Layer", |d| Ok(d.add_layer("Red", [200, 0, 0])))
            .unwrap();
        let id = doc
            .transaction("Scene", |d| {
                let name = d.next_scene_name();
                d.add_scene(Scene {
                    name,
                    view: view(),
                    present: false,
                    style: SceneStyle::default(),
                    hidden_layers: vec![red],
                    section: None,
                })
            })
            .unwrap();
        assert_eq!(doc.scene(id).unwrap().name, "Render 1");
        assert_eq!(doc.next_scene_name(), "Render 2");
        doc.transaction("Scene 1", |d| d.apply_scene_layers(id))
            .unwrap();
        assert!(!doc.layer_visible(Some(red)));
        assert!(doc.undo().is_some());
        assert!(doc.layer_visible(Some(red)));
        assert!(doc.undo().is_some());
        assert!(doc.scene(id).is_none());
    }

    #[test]
    fn a_scene_needs_a_name_and_a_good_camera() {
        let mut doc = Document::new();
        let bad = Scene {
            name: " ".into(),
            view: view(),
            present: false,
            style: SceneStyle::default(),
            hidden_layers: Vec::new(),
            section: None,
        };
        assert!(doc.add_scene(bad).is_err());
        let flat = Scene {
            name: "Top".into(),
            view: SceneView {
                distance: 0.0,
                ..view()
            },
            present: false,
            style: SceneStyle::default(),
            hidden_layers: Vec::new(),
            section: None,
        };
        assert!(doc.add_scene(flat).is_err());
    }
}
