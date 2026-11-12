//! SketchSmith — document, definitions and undo | v1.0.0 | Thursday 12 November 2026
//!
//! A [`Document`] holds the model root context, the definitions that groups
//! and components use, and the undo/redo history.
//!
//! **Definitions and instances.** A [`Definition`] owns its own [`Entities`];
//! an [`Instance`] placed in a context refers to a definition with a `DMat4`
//! transform. A group is a definition flagged [`DefinitionKind::Group`].
//! Instances are placed with [`Document::add_instance`] and removed with
//! [`Document::remove_instance`]; [`Document::context`] and
//! [`Document::context_entities_mut`] reach the root (`None`) or a
//! definition's geometry alike.
//!
//! **Undo.** Every user operation runs inside one transaction
//! ([`Document::transaction`]). While a transaction is open, every touched entity
//! in every context saves its prior state once (copy-on-write). Commit turns
//! that into one undo step; history is unlimited for the session and is not
//! saved to disk. A failed operation is rolled back completely.
//!
//! Definition geometry is journaled in two layers: entity-level changes inside
//! definitions that existed when the transaction began, and whole-definition
//! snapshots only for definitions created or removed during it.
//!
//! **Materials** belong to the document, listed in creation order, and are
//! undoable like layers. The model's **smoothing angle** (Present mode) is
//! document data too, and an undo step when it changes.
//!
//! **Revision.** Every committed step gets a serial number that is never
//! reused. [`Document::revision`] is the serial of the step on top of the undo
//! stack (0 when it is empty), so the application can tell whether the model
//! differs from the last save even after undo and redo.

use std::collections::BTreeMap;

use smith_geom::{DMat4, DVec3};

use crate::arena::{Arena, Change, SharedChange};
use crate::entities::Entities;
use crate::error::ModelError;
use crate::ids::{DefinitionId, EdgeId, FaceId, InstanceId, LayerId, MaterialId, SceneId};
use crate::journal::EntitiesChange;
use crate::look::LineStyle;
use crate::paint::Material;
use crate::scenes::Scene;
use crate::shapes::{ShapeSpec, SweepRecipe};

/// Whether a definition backs a group or a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionKind {
    /// A group: one instance, made unique when copied and edited.
    Group,
    /// A named component: every instance shares the geometry.
    Component,
}

/// A definition's descriptive data. Its geometry is reached through
/// [`Document::definition_entities`].
#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    /// Display name.
    pub name: String,
    /// Group or component.
    pub kind: DefinitionKind,
    /// The shape it was made as (Shapes), while its geometry has not been
    /// edited by hand.
    pub shape: Option<ShapeSpec>,
    /// What it was made from (Sweep, Revolve), while its geometry has not
    /// been edited by hand.
    pub sweep: Option<SweepRecipe>,
}

/// A placed group or component.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// The definition whose geometry this instance shows.
    pub definition: DefinitionId,
    /// Placement: definition space → containing context.
    pub transform: DMat4,
    /// Instance name (may be empty).
    pub name: String,
    /// The layer it is on; `None` is the Base layer.
    pub layer: Option<LayerId>,
    /// Locked: it cannot be selected for editing, moved or erased.
    pub locked: bool,
    /// Hidden by the user.
    pub hidden: bool,
}

/// A layer: a named set of geometry, groups and components that can be
/// shown or hidden together. Every document has the Base layer, which
/// cannot be removed; entities on no other layer are on Base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    /// Display name.
    pub name: String,
    /// The number it was created with (Base is 0). It stays through
    /// renames; a new layer takes the highest number held plus one, so the
    /// newest layer's number comes back when it is removed, an older one's
    /// does not.
    pub number: u32,
    /// Display colour (sRGB).
    pub color: [u8; 3],
    /// Shown in the canvas.
    pub visible: bool,
    /// The Present-mode line style of the edges on it (`None`: plain).
    pub style: Option<LineStyle>,
    /// Locked: what is on it cannot be selected or changed.
    pub locked: bool,
    /// Its place in the stack, bottom up: Base is 0, the others above it.
    /// Set by the document (a new layer goes on top; see
    /// [`Document::reorder_layer`]); [`Document::set_layer`] keeps it.
    pub order: u32,
}

/// The refusal of a step that would change something on a locked layer.
pub const ON_LOCKED_LAYER: &str = "On a Locked Layer: Unlock It First";

/// The refusal of a layer command on a locked layer.
pub const UNLOCK_LAYER: &str = "Unlock the Layer First";

impl Layer {
    /// The name of the layer every document has.
    pub const BASE: &'static str = "Base";

    /// The Base layer as a new document has it.
    pub fn base() -> Self {
        Self {
            name: Self::BASE.into(),
            number: 0,
            color: [160, 160, 160],
            visible: true,
            style: None,
            locked: false,
            order: 0,
        }
    }
}

/// Document-wide Present-mode settings that are part of the model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelLook {
    /// Faces meeting at less than this angle (degrees) shade as one surface,
    /// unless the edge between them is marked.
    pub smoothing_angle_deg: f64,
}

impl Default for ModelLook {
    fn default() -> Self {
        Self {
            smoothing_angle_deg: 30.0,
        }
    }
}

/// One undo step.
#[derive(Debug, Clone)]
struct Transaction {
    name: String,
    /// Unique, increasing serial (see [`Document::revision`]).
    serial: u64,
    root: EntitiesChange,
    definitions: Vec<SharedChange<DefinitionId, Definition>>,
    layers: Vec<SharedChange<LayerId, Layer>>,
    scenes: Vec<SharedChange<SceneId, Scene>>,
    materials: Vec<SharedChange<MaterialId, Material>>,
    /// The model look before and after, when it changed.
    look: Option<(ModelLook, ModelLook)>,
    /// Geometry of definitions created (before `None`) or removed (after `None`).
    created_removed: Vec<Change<DefinitionId, Entities>>,
    /// Entity-level changes inside definitions that already existed.
    edited: Vec<(DefinitionId, EntitiesChange)>,
}

impl Transaction {
    fn is_empty(&self) -> bool {
        self.root.is_empty()
            && self.definitions.is_empty()
            && self.layers.is_empty()
            && self.scenes.is_empty()
            && self.materials.is_empty()
            && self.look.is_none()
            && self.created_removed.is_empty()
            && self.edited.is_empty()
    }
}

/// The model: root geometry, definitions, and history.
#[derive(Debug, Clone, Default)]
pub struct Document {
    root: Entities,
    definitions: Arena<DefinitionId, Definition>,
    layers: Arena<LayerId, Layer>,
    /// Saved views, in the order they were made.
    pub(crate) scenes: Arena<SceneId, Scene>,
    /// Materials, in the order they were made.
    pub(crate) materials: Arena<MaterialId, Material>,
    /// Present-mode settings saved with the model.
    pub(crate) look: ModelLook,
    /// The look when the open transaction began.
    look_before: Option<ModelLook>,
    /// Definitions whose shape or sweep recipe was set in the open
    /// transaction (kept through its hand-edit check).
    recipe_set: std::collections::BTreeSet<DefinitionId>,
    /// The layer new geometry goes on (`None`: Base). A setting, saved with
    /// the file but not an undo step.
    active_layer: Option<LayerId>,
    geometry: BTreeMap<DefinitionId, Entities>,
    /// Geometry snapshots of definitions created/removed in the open transaction.
    geometry_journal: Option<BTreeMap<DefinitionId, Option<Entities>>>,
    open: Option<String>,
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    /// Serial given to the last committed step.
    last_serial: u64,
}

impl Document {
    /// An empty document, with the Base layer.
    pub fn new() -> Self {
        let mut d = Self::default();
        d.layers.insert(Layer::base());
        d
    }

    // ----- materials and look ----------------------------------------------

    /// Every material key, in creation order.
    pub fn material_ids(&self) -> impl Iterator<Item = MaterialId> + '_ {
        self.materials.keys()
    }

    /// A material by key.
    pub fn material(&self, id: MaterialId) -> Option<&Material> {
        self.materials.get(id)
    }

    /// Adds `material`, its name made unique with a number (`Material`,
    /// `Material 2`, …; an empty name becomes `Material`), and returns it.
    ///
    /// # Errors
    /// [`ModelError::NonFinite`] for values out of range.
    pub fn add_material(&mut self, mut material: Material) -> Result<MaterialId, ModelError> {
        if !material.is_valid() {
            return Err(ModelError::NonFinite);
        }
        material.name = self.unique_material_name(&material.name, None);
        Ok(self.materials.insert(material))
    }

    /// Changes material `id`; every face painted with it follows. A name
    /// another material has is made unique.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`]; [`ModelError::NonFinite`] for values
    /// out of range.
    pub fn set_material(
        &mut self,
        id: MaterialId,
        mut material: Material,
    ) -> Result<(), ModelError> {
        if !material.is_valid() {
            return Err(ModelError::NonFinite);
        }
        if self.materials.get(id).is_none() {
            return Err(ModelError::NoSuchEntity);
        }
        material.name = self.unique_material_name(&material.name, Some(id));
        if self.materials.get(id) != Some(&material) {
            self.materials[id] = material;
        }
        Ok(())
    }

    fn unique_material_name(&self, name: &str, except: Option<MaterialId>) -> String {
        let base = match name.trim() {
            "" => "Material",
            t => t,
        };
        let taken = |n: &str| {
            self.materials
                .iter()
                .any(|(k, m)| Some(k) != except && m.name == n)
        };
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|n| !taken(n))
            .unwrap_or_default()
    }

    /// Removes material `id`, unpainting every face side painted with it.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn remove_material(&mut self, id: MaterialId) -> Result<(), ModelError> {
        if self.materials.get(id).is_none() {
            return Err(ModelError::NoSuchEntity);
        }
        self.replace_material(id, None)?;
        self.materials.remove(id);
        Ok(())
    }

    /// Repaints every face side painted with `old`, in every context, with
    /// `new` (`None` unpaints). Returns how many sides changed.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] for a material that does not exist.
    pub fn replace_material(
        &mut self,
        old: MaterialId,
        new: Option<MaterialId>,
    ) -> Result<usize, ModelError> {
        if self.materials.get(old).is_none() || new.is_some_and(|m| self.materials.get(m).is_none())
        {
            return Err(ModelError::NoSuchEntity);
        }
        if new == Some(old) {
            return Ok(0);
        }
        let mut n = self.root.replace_material(old, new);
        for g in self.geometry.values_mut() {
            n += g.replace_material(old, new);
        }
        Ok(n)
    }

    /// The materials some face side, in any context, is painted with, each
    /// with the index it is saved under (unused materials are not saved).
    pub(crate) fn used_materials(&self) -> BTreeMap<MaterialId, u32> {
        let mut used = std::collections::BTreeSet::new();
        for g in std::iter::once(&self.root).chain(self.geometry.values()) {
            used.extend(g.painted_materials());
        }
        used.into_iter()
            .enumerate()
            .map(|(i, m)| (m, u32::try_from(i).unwrap_or(u32::MAX)))
            .collect()
    }

    /// Purge Unused, groups: removes every group and component definition
    /// no instance uses (anywhere, including inside other unused ones).
    /// Returns how many went. Materials are left alone
    /// ([`Document::remove_unused_materials`]).
    ///
    /// # Errors
    /// As [`Document::remove_definition`].
    pub fn purge_unused_groups(&mut self) -> Result<usize, ModelError> {
        let mut definitions = 0;
        loop {
            let unused: Vec<DefinitionId> = self
                .definitions
                .keys()
                .filter(|d| self.instance_users(*d) == 0)
                .collect();
            if unused.is_empty() {
                break;
            }
            for d in unused {
                self.remove_definition(d)?;
                definitions += 1;
            }
        }
        Ok(definitions)
    }

    /// The materials no face side, in any context, is painted with.
    fn unused_materials(&self) -> Vec<MaterialId> {
        let used = self.used_materials();
        self.materials
            .keys()
            .filter(|m| !used.contains_key(m))
            .collect()
    }

    /// How many materials no face side is painted with.
    pub fn unused_material_count(&self) -> usize {
        self.unused_materials().len()
    }

    /// Purge Unused, materials: removes every material no face side is
    /// painted with. Returns how many went.
    pub fn remove_unused_materials(&mut self) -> usize {
        let unused = self.unused_materials();
        for m in &unused {
            self.materials.remove(*m);
        }
        unused.len()
    }

    /// The model's Present-mode settings.
    pub fn look(&self) -> ModelLook {
        self.look
    }

    /// Sets the smoothing angle, 0–180 degrees.
    ///
    /// # Errors
    /// [`ModelError::NonFinite`] for an angle outside that range.
    pub fn set_smoothing_angle(&mut self, degrees: f64) -> Result<(), ModelError> {
        if !degrees.is_finite() || !(0.0..=180.0).contains(&degrees) {
            return Err(ModelError::NonFinite);
        }
        self.look.smoothing_angle_deg = degrees;
        Ok(())
    }

    /// The Base layer: the first layer, which every document has.
    pub fn base_layer(&self) -> Option<LayerId> {
        self.layers.keys().next()
    }

    /// Every layer in stack order, bottom up: Base first.
    pub fn layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        let mut ids: Vec<(u32, LayerId)> = self.layers.iter().map(|(k, l)| (l.order, k)).collect();
        ids.sort();
        ids.into_iter().map(|(_, k)| k)
    }

    /// The order a new layer on top of the stack takes.
    fn next_layer_order(&self) -> u32 {
        self.layers
            .iter()
            .map(|(_, l)| l.order)
            .max()
            .map_or(1, |n| n + 1)
    }

    /// Moves layer `id` to `position` in the stack, counted bottom up from
    /// the first place above Base (0); a position past the top puts it on
    /// top. Base stays at the bottom.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] for a missing layer or Base.
    pub fn reorder_layer(&mut self, id: LayerId, position: usize) -> Result<(), ModelError> {
        if Some(id) == self.base_layer() || self.layers.get(id).is_none() {
            return Err(ModelError::NoSuchEntity);
        }
        let base = self.base_layer();
        let mut stack: Vec<LayerId> = self
            .layer_ids()
            .filter(|l| Some(*l) != base && *l != id)
            .collect();
        stack.insert(position.min(stack.len()), id);
        for (i, l) in stack.into_iter().enumerate() {
            let order = i as u32 + 1;
            if self.layers.get(l).is_some_and(|x| x.order != order)
                && let Some(x) = self.layers.get_mut(l)
            {
                x.order = order;
            }
        }
        Ok(())
    }

    /// Layer `id`'s position in the stack above Base (0 just above it), as
    /// [`Document::reorder_layer`] counts; `None` for Base or a missing
    /// layer.
    pub fn layer_position(&self, id: LayerId) -> Option<usize> {
        let base = self.base_layer();
        self.layer_ids()
            .filter(|l| Some(*l) != base)
            .position(|l| l == id)
    }

    /// A layer by key.
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.get(id)
    }

    /// The layer new geometry goes on (`None`: Base).
    pub fn active_layer(&self) -> Option<LayerId> {
        self.active_layer
    }

    /// Makes `layer` (`None`: Base) the one new geometry goes on, in every
    /// context. A layer that does not exist means Base.
    pub fn set_active_layer(&mut self, layer: Option<LayerId>) {
        let layer =
            layer.filter(|l| self.layers.get(*l).is_some() && Some(*l) != self.base_layer());
        self.active_layer = layer;
        self.root.set_draw_layer(layer);
        for e in self.geometry.values_mut() {
            e.set_draw_layer(layer);
        }
    }

    /// Whether entities on `layer` (`None`: Base) are shown.
    pub fn layer_visible(&self, layer: Option<LayerId>) -> bool {
        layer
            .or_else(|| self.base_layer())
            .and_then(|l| self.layers.get(l))
            .is_none_or(|l| l.visible)
    }

    /// Whether entities on `layer` (`None`: Base) are locked.
    pub fn layer_locked(&self, layer: Option<LayerId>) -> bool {
        layer
            .or_else(|| self.base_layer())
            .and_then(|l| self.layers.get(l))
            .is_some_and(|l| l.locked)
    }

    /// Whether the open transaction changes anything on a locked layer.
    fn touches_locked_layer(&self) -> bool {
        if !self.layers.iter().any(|(_, l)| l.locked) {
            return false;
        }
        let locked = |l: Option<LayerId>| self.layer_locked(l);
        self.root.journal_touches(&locked)
            || self
                .geometry
                .values()
                .any(|g| g.journal_open() && g.journal_touches(&locked))
    }

    /// Adds a layer named `name` (made unique with a number), shown, and
    /// returns it.
    pub fn add_layer(&mut self, name: &str, color: [u8; 3]) -> LayerId {
        let taken: Vec<String> = self.layers.iter().map(|(_, l)| l.name.clone()).collect();
        let name = if taken.iter().any(|t| t == name) {
            (2..)
                .map(|n| format!("{name} {n}"))
                .find(|n| !taken.contains(n))
                .unwrap_or_default()
        } else {
            name.to_string()
        };
        let number = self.next_layer_number();
        let order = self.next_layer_order();
        self.layers.insert(Layer {
            name,
            number,
            color,
            visible: true,
            style: None,
            locked: false,
            order,
        })
    }

    /// Adds a layer named by its number (`1`, `2`, …), shown, and returns
    /// it. The number is the highest any layer holds plus one; a name already
    /// taken (typed by hand) is skipped, taking the next number up.
    pub fn add_numbered_layer(&mut self, color: [u8; 3]) -> LayerId {
        let taken: Vec<String> = self.layers.iter().map(|(_, l)| l.name.clone()).collect();
        let mut number = self.next_layer_number();
        while taken.contains(&number.to_string()) {
            number += 1;
        }
        let order = self.next_layer_order();
        self.layers.insert(Layer {
            name: number.to_string(),
            number,
            color,
            visible: true,
            style: None,
            locked: false,
            order,
        })
    }

    /// The highest number any layer holds, plus one.
    fn next_layer_number(&self) -> u32 {
        self.layers
            .iter()
            .map(|(_, l)| l.number)
            .max()
            .map_or(1, |n| n + 1)
    }

    /// Changes layer `id`: its name, number, colour, shown flag and line
    /// style. Its place in the stack is kept.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn set_layer(&mut self, id: LayerId, layer: Layer) -> Result<(), ModelError> {
        let x = self.layers.get_mut(id).ok_or(ModelError::NoSuchEntity)?;
        // Its place in the stack changes only through `reorder_layer`.
        *x = Layer {
            order: x.order,
            ..layer
        };
        Ok(())
    }

    /// The edges, faces and groups on layer `layer` (`None` or Base's key:
    /// Base) in context `ctx`, in creation order.
    pub fn layer_contents(
        &self,
        ctx: Option<DefinitionId>,
        layer: Option<LayerId>,
    ) -> (Vec<EdgeId>, Vec<FaceId>, Vec<InstanceId>) {
        let key = layer.filter(|l| Some(*l) != self.base_layer());
        let Some(e) = self.context(ctx) else {
            return Default::default();
        };
        (
            e.edges
                .iter()
                .filter(|(_, x)| x.flags.layer == key)
                .map(|(k, _)| k)
                .collect(),
            e.faces
                .iter()
                .filter(|(_, x)| x.attributes.layer == key)
                .map(|(k, _)| k)
                .collect(),
            e.instances
                .iter()
                .filter(|(_, x)| x.layer == key)
                .map(|(k, _)| k)
                .collect(),
        )
    }

    /// Duplicates layer `id`: a new layer just above it, named after it with
    /// "copy", with its colour, visibility and line style, unlocked; every
    /// group and component on it, in every context, gets a copy in the same
    /// place on the new layer (a component's copy stays that component).
    /// Loose geometry is not copied: in place it would merge into the
    /// original. Returns the new layer and how many loose edges and faces
    /// were left.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] for a missing layer.
    pub fn duplicate_layer(&mut self, id: LayerId) -> Result<(LayerId, usize), ModelError> {
        let source = self
            .layers
            .get(id)
            .cloned()
            .ok_or(ModelError::NoSuchEntity)?;
        let key = (Some(id) != self.base_layer()).then_some(id);
        let copy = self.add_layer(&format!("{} copy", source.name), source.color);
        if let Some(x) = self.layers.get_mut(copy) {
            x.visible = source.visible;
            x.style = source.style;
        }
        let above = self.layer_position(id).map_or(0, |p| p + 1);
        self.reorder_layer(copy, above)?;
        let contexts: Vec<Option<DefinitionId>> = std::iter::once(None)
            .chain(self.geometry.keys().map(|k| Some(*k)))
            .collect();
        let mut loose = 0;
        for ctx in contexts {
            let Some(e) = self.context_mut(ctx) else {
                continue;
            };
            loose += e.edges.iter().filter(|(_, x)| x.flags.layer == key).count()
                + e.faces
                    .iter()
                    .filter(|(_, x)| x.attributes.layer == key)
                    .count();
            let groups: Vec<InstanceId> = e
                .instances
                .iter()
                .filter(|(_, x)| x.layer == key)
                .map(|(k, _)| k)
                .collect();
            for g in groups {
                let c = e.copy_instance(g, &DMat4::IDENTITY)?;
                if let Some(x) = e.instances.get_mut(c) {
                    x.layer = Some(copy);
                }
            }
        }
        Ok((copy, loose))
    }

    /// Every context: the model root, then each definition.
    fn all_contexts(&self) -> Vec<Option<DefinitionId>> {
        std::iter::once(None)
            .chain(self.geometry.keys().map(|k| Some(*k)))
            .collect()
    }

    /// The key entities carry for layer `id`: `None` for Base.
    fn layer_key(&self, id: LayerId) -> Option<LayerId> {
        (Some(id) != self.base_layer()).then_some(id)
    }

    /// Deletes layer `id`'s record, the active layer falling back to `to`
    /// (`None`: Base) when it was the active one.
    fn drop_layer(&mut self, id: LayerId, to: Option<LayerId>) {
        self.layers.remove(id);
        if self.active_layer == Some(id) {
            self.set_active_layer(to);
        }
    }

    /// Refuses when any of `layers` is locked.
    fn refuse_locked(&self, layers: &[LayerId]) -> Result<(), ModelError> {
        if layers
            .iter()
            .any(|l| self.layers.get(*l).is_some_and(|x| x.locked))
        {
            return Err(ModelError::Refused(UNLOCK_LAYER.into()));
        }
        Ok(())
    }

    /// How many faces and groups (in every context) and loose edges (edges
    /// with no face) are on layer `id`: what Remove would delete.
    pub fn layer_content_count(&self, id: LayerId) -> (usize, usize, usize) {
        let key = self.layer_key(id);
        let mut n = (0, 0, 0);
        for ctx in self.all_contexts() {
            let Some(e) = self.context(ctx) else { continue };
            n.0 += e
                .faces
                .iter()
                .filter(|(_, f)| f.attributes.layer == key)
                .count();
            n.1 += e.instances.iter().filter(|(_, i)| i.layer == key).count();
            n.2 += e
                .edges
                .iter()
                .filter(|(_, x)| x.flags.layer == key && x.faces().is_empty())
                .count();
        }
        n
    }

    /// Removes layer `id` and everything on it, in every context: its
    /// groups, faces and edges are deleted. An edge on it that bounds a face
    /// on a layer that stays is kept, moved to that face's layer (the first
    /// face's, if several). Base cannot be removed, nor a locked layer.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] for a missing layer or Base;
    /// [`ModelError::Refused`] for a locked layer.
    pub fn remove_layer(&mut self, id: LayerId) -> Result<(), ModelError> {
        if Some(id) == self.base_layer() || self.layers.get(id).is_none() {
            return Err(ModelError::NoSuchEntity);
        }
        self.refuse_locked(&[id])?;
        let key = self.layer_key(id);
        for ctx in self.all_contexts() {
            let Some(e) = self.context_mut(ctx) else {
                continue;
            };
            let groups: Vec<InstanceId> = e
                .instances
                .iter()
                .filter(|(_, i)| i.layer == key)
                .map(|(k, _)| k)
                .collect();
            for g in groups {
                e.erase_instance(g)?;
            }
            let faces: Vec<FaceId> = e
                .faces
                .iter()
                .filter(|(_, f)| f.attributes.layer == key)
                .map(|(k, _)| k)
                .collect();
            for f in faces {
                if e.faces.contains_key(f) {
                    e.erase_face(f)?;
                }
            }
            let edges: Vec<EdgeId> = e
                .edges
                .iter()
                .filter(|(_, x)| x.flags.layer == key)
                .map(|(k, _)| k)
                .collect();
            for edge in edges {
                let Some(x) = e.edges.get(edge) else { continue };
                // A face that stays keeps its boundary, on its own layer.
                let keep = x
                    .faces()
                    .first()
                    .and_then(|f| e.faces.get(*f))
                    .map(|f| f.attributes.layer);
                match keep {
                    Some(layer) => {
                        if let Some(x) = e.edges.get_mut(edge) {
                            x.flags.layer = layer;
                        }
                    }
                    None => e.erase_edge(edge)?,
                }
            }
        }
        self.drop_layer(id, None);
        Ok(())
    }

    /// Anchor to Base: everything on layer `id`, in every context, moves to
    /// Base, and the layer is deleted. Base itself cannot be anchored, nor
    /// a locked layer.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] for a missing layer or Base;
    /// [`ModelError::Refused`] for a locked layer.
    pub fn anchor_layer(&mut self, id: LayerId) -> Result<(), ModelError> {
        if Some(id) == self.base_layer() || self.layers.get(id).is_none() {
            return Err(ModelError::NoSuchEntity);
        }
        self.refuse_locked(&[id])?;
        if let Some(base) = self.base_layer() {
            self.refuse_locked(&[base])?;
        }
        self.move_contents(id, None);
        self.drop_layer(id, None);
        Ok(())
    }

    /// Moves everything on layer `from`, in every context, to `to` (`None`:
    /// Base).
    fn move_contents(&mut self, from: LayerId, to: Option<LayerId>) {
        for ctx in self.all_contexts() {
            if let Some(e) = self.context_mut(ctx) {
                e.move_layer(from, to);
            }
        }
    }

    /// Merge Down: everything on layer `id` moves to the layer just below it
    /// in the stack, and `id` is deleted. Base has nothing below it.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] for a missing layer or Base;
    /// [`ModelError::Refused`] when either layer is locked.
    pub fn merge_layer_down(&mut self, id: LayerId) -> Result<(), ModelError> {
        if Some(id) == self.base_layer() || self.layers.get(id).is_none() {
            return Err(ModelError::NoSuchEntity);
        }
        let stack: Vec<LayerId> = self.layer_ids().collect();
        let at = stack
            .iter()
            .position(|l| *l == id)
            .ok_or(ModelError::NoSuchEntity)?;
        let below = stack[at.saturating_sub(1)];
        self.refuse_locked(&[id, below])?;
        let to = self.layer_key(below);
        self.move_contents(id, to);
        self.drop_layer(id, to);
        Ok(())
    }

    /// Merge Visible: everything on every shown layer moves to the lowest
    /// shown one, and the other shown layers are deleted; hidden layers are
    /// left as they are. Returns how many layers were merged in (0 with one
    /// layer or none shown).
    ///
    /// # Errors
    /// [`ModelError::Refused`] when a shown layer is locked.
    pub fn merge_visible_layers(&mut self) -> Result<usize, ModelError> {
        let shown: Vec<LayerId> = self
            .layer_ids()
            .filter(|l| self.layers.get(*l).is_some_and(|x| x.visible))
            .collect();
        let Some((&lowest, rest)) = shown.split_first() else {
            return Ok(0);
        };
        if rest.is_empty() {
            return Ok(0);
        }
        self.refuse_locked(&shown)?;
        let to = self.layer_key(lowest);
        for l in rest {
            self.move_contents(*l, to);
            self.drop_layer(*l, to);
        }
        Ok(rest.len())
    }

    /// Flatten: everything on every layer, hidden ones included, moves to
    /// Base, and every other layer is deleted. Returns how many layers went.
    ///
    /// # Errors
    /// [`ModelError::Refused`] when any layer, Base included, is locked.
    pub fn flatten_layers(&mut self) -> Result<usize, ModelError> {
        let all: Vec<LayerId> = self.layer_ids().collect();
        self.refuse_locked(&all)?;
        let base = self.base_layer();
        let others: Vec<LayerId> = all.into_iter().filter(|l| Some(*l) != base).collect();
        for l in &others {
            self.move_contents(*l, None);
            self.drop_layer(*l, None);
        }
        Ok(others.len())
    }

    /// The root context.
    pub fn root(&self) -> &Entities {
        &self.root
    }

    /// The root context, for editing. Changes are undoable only inside a transaction.
    pub fn root_mut(&mut self) -> &mut Entities {
        &mut self.root
    }

    /// A definition's descriptive data.
    pub fn definition(&self, id: DefinitionId) -> Option<&Definition> {
        self.definitions.get(id)
    }

    /// All definition keys.
    pub fn definition_ids(&self) -> impl Iterator<Item = DefinitionId> + '_ {
        self.definitions.keys()
    }

    /// A definition's geometry.
    pub fn definition_entities(&self, id: DefinitionId) -> Option<&Entities> {
        self.geometry.get(&id)
    }

    /// A definition's geometry, for editing.
    pub fn definition_entities_mut(&mut self, id: DefinitionId) -> Option<&mut Entities> {
        self.geometry.get_mut(&id)
    }

    /// The geometry of context `ctx` (`None` = root).
    pub fn context(&self, ctx: Option<DefinitionId>) -> Option<&Entities> {
        match ctx {
            None => Some(&self.root),
            Some(id) => self.geometry.get(&id),
        }
    }

    /// The geometry of context `ctx` (`None` = root), for editing.
    pub fn context_entities_mut(&mut self, ctx: Option<DefinitionId>) -> Option<&mut Entities> {
        self.context_mut(ctx)
    }

    /// Removes instance `id` from context `ctx` and returns it. Its definition
    /// stays, even when no instance uses it any more (see
    /// [`Document::instance_users`]).
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn remove_instance(
        &mut self,
        ctx: Option<DefinitionId>,
        id: InstanceId,
    ) -> Result<Instance, ModelError> {
        let entities = self.context_mut(ctx).ok_or(ModelError::NoSuchEntity)?;
        entities
            .instances
            .remove(id)
            .ok_or(ModelError::NoSuchEntity)
    }

    /// The bounds of `definition`'s geometry, nested groups and components
    /// included, in its own coordinates; `None` when it is empty.
    pub fn definition_bounds(&self, definition: DefinitionId) -> Option<(DVec3, DVec3)> {
        self.context_bounds(Some(definition), 0)
    }

    fn context_bounds(&self, ctx: Option<DefinitionId>, depth: u32) -> Option<(DVec3, DVec3)> {
        let entities = self.context(ctx)?;
        let mut acc = entities.bounds();
        if depth > 64 {
            return acc;
        }
        for (_, inst) in entities.instances.iter() {
            if let Some(corners) = self
                .context_bounds(Some(inst.definition), depth + 1)
                .map(|b| box_corners(b, &inst.transform))
            {
                for c in corners {
                    acc = Some(acc.map_or((c, c), |(lo, hi)| (lo.min(c), hi.max(c))));
                }
            }
        }
        acc
    }

    /// The bounds of the whole model: root geometry and every group and
    /// component placed in it.
    pub fn model_bounds(&self) -> Option<(DVec3, DVec3)> {
        self.context_bounds(None, 0)
    }

    /// The eight corners of each instance's box in context `ctx`, in the
    /// context's coordinates, for sizing and centring tools.
    pub fn instance_boxes(&self, ctx: Option<DefinitionId>) -> BTreeMap<InstanceId, [DVec3; 8]> {
        let Some(entities) = self.context(ctx) else {
            return BTreeMap::new();
        };
        entities
            .instances
            .iter()
            .filter_map(|(id, inst)| {
                let b = self.definition_bounds(inst.definition)?;
                Some((id, box_corners(b, &inst.transform)))
            })
            .collect()
    }

    /// Gives instance `id` of context `ctx` its own copy of its definition,
    /// so editing it no longer changes the other instances, and returns the
    /// new definition. The copy keeps the kind; its name gets the next free
    /// `#n` (`Wheel` → `Wheel#2`).
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn make_unique(
        &mut self,
        ctx: Option<DefinitionId>,
        id: InstanceId,
    ) -> Result<DefinitionId, ModelError> {
        let old = self
            .context(ctx)
            .and_then(|c| c.instance(id))
            .map(|i| i.definition)
            .ok_or(ModelError::NoSuchEntity)?;
        let def = self
            .definitions
            .get(old)
            .cloned()
            .ok_or(ModelError::NoSuchEntity)?;
        let mut geometry = self
            .geometry
            .get(&old)
            .cloned()
            .ok_or(ModelError::NoSuchEntity)?;
        // The copy starts as it is now: nothing of the open step to undo in it.
        if geometry.journal_open() {
            geometry.take_journal();
        }
        let base = def.name.split('#').next().unwrap_or("").to_string();
        let taken: Vec<String> = self
            .definitions
            .iter()
            .map(|(_, d)| d.name.clone())
            .collect();
        let name = (2..)
            .map(|n| format!("{base}#{n}"))
            .find(|n| !taken.contains(n))
            .unwrap_or_default();
        let new = self.create_definition(name, def.kind);
        self.geometry.insert(new, geometry);
        let entities = self.context_mut(ctx).ok_or(ModelError::NoSuchEntity)?;
        entities
            .instances
            .get_mut(id)
            .ok_or(ModelError::NoSuchEntity)?
            .definition = new;
        Ok(new)
    }

    /// Applies `m` after the transform of every instance, in every context,
    /// that uses `definition` (`T` becomes `T · m`).
    pub fn retransform_users(&mut self, definition: DefinitionId, m: &DMat4) {
        for entities in std::iter::once(&mut self.root).chain(self.geometry.values_mut()) {
            let ids: Vec<InstanceId> = entities
                .instances
                .iter()
                .filter(|(_, i)| i.definition == definition)
                .map(|(id, _)| id)
                .collect();
            for id in ids {
                if let Some(i) = entities.instances.get_mut(id) {
                    i.transform *= *m;
                }
            }
        }
    }

    /// Sets instance `id`'s transform.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`]; [`ModelError::NonFinite`].
    pub fn set_instance_transform(
        &mut self,
        ctx: Option<DefinitionId>,
        id: InstanceId,
        m: DMat4,
    ) -> Result<(), ModelError> {
        if !m.is_finite() {
            return Err(ModelError::NonFinite);
        }
        self.context_mut(ctx)
            .and_then(|c| c.instances.get_mut(id))
            .ok_or(ModelError::NoSuchEntity)?
            .transform = m;
        Ok(())
    }

    /// How many instances, in every context, use `definition`.
    pub fn instance_users(&self, definition: DefinitionId) -> usize {
        std::iter::once(&self.root)
            .chain(self.geometry.values())
            .map(|e| {
                e.instances
                    .iter()
                    .filter(|(_, i)| i.definition == definition)
                    .count()
            })
            .sum()
    }

    /// The context `ctx` (`None` = root), for editing.
    fn context_mut(&mut self, ctx: Option<DefinitionId>) -> Option<&mut Entities> {
        match ctx {
            None => Some(&mut self.root),
            Some(id) => self.geometry.get_mut(&id),
        }
    }

    /// Creates an empty definition.
    pub fn create_definition(
        &mut self,
        name: impl Into<String>,
        kind: DefinitionKind,
    ) -> DefinitionId {
        let id = self.definitions.insert(Definition {
            shape: None,
            sweep: None,
            name: name.into(),
            kind,
        });
        if let Some(j) = &mut self.geometry_journal {
            j.entry(id).or_insert(None);
        }
        let mut e = Entities::new();
        e.set_draw_layer(self.active_layer);
        self.geometry.insert(id, e);
        id
    }

    /// Sets (or clears) definition `id`'s shape recipe (call within a
    /// transaction). Set this way, it survives this step's own geometry
    /// changes; a later hand edit clears it.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn set_definition_shape(
        &mut self,
        id: DefinitionId,
        shape: Option<ShapeSpec>,
    ) -> Result<(), ModelError> {
        let d = self
            .definitions
            .get_mut(id)
            .ok_or(ModelError::NoSuchEntity)?;
        d.shape = shape;
        self.recipe_set.insert(id);
        Ok(())
    }

    /// Sets (or clears) definition `id`'s sweep recipe (call within a
    /// transaction). Like a shape recipe, it survives this step's own
    /// geometry changes; a later hand edit clears it.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn set_definition_sweep(
        &mut self,
        id: DefinitionId,
        sweep: Option<SweepRecipe>,
    ) -> Result<(), ModelError> {
        let d = self
            .definitions
            .get_mut(id)
            .ok_or(ModelError::NoSuchEntity)?;
        d.sweep = sweep;
        self.recipe_set.insert(id);
        Ok(())
    }

    /// Renames a definition.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn rename_definition(
        &mut self,
        id: DefinitionId,
        name: impl Into<String>,
    ) -> Result<(), ModelError> {
        self.definitions
            .get_mut(id)
            .ok_or(ModelError::NoSuchEntity)?
            .name = name.into();
        Ok(())
    }

    /// Removes a definition and its geometry. Instances that refer to it must be
    /// removed by the caller first (not yet enforced).
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn remove_definition(&mut self, id: DefinitionId) -> Result<(), ModelError> {
        let mut geometry = self.geometry.remove(&id).ok_or(ModelError::NoSuchEntity)?;
        // Snapshot the geometry as it was when the transaction began.
        if geometry.journal_open() {
            geometry.rollback_journal();
        }
        if let Some(j) = &mut self.geometry_journal {
            j.entry(id).or_insert(Some(geometry));
        }
        self.definitions.remove(id);
        Ok(())
    }

    /// Places an instance of `definition` in context `ctx` (`None` = root).
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] for a missing definition or context;
    /// [`ModelError::RecursiveInstance`] if the placement would make a
    /// definition contain itself.
    pub fn add_instance(
        &mut self,
        ctx: Option<DefinitionId>,
        definition: DefinitionId,
        transform: DMat4,
        name: impl Into<String>,
    ) -> Result<InstanceId, ModelError> {
        if !self.definitions.contains_key(definition) {
            return Err(ModelError::NoSuchEntity);
        }
        if let Some(c) = ctx
            && (c == definition || self.contains_definition(definition, c))
        {
            return Err(ModelError::RecursiveInstance);
        }
        let entities = self.context_mut(ctx).ok_or(ModelError::NoSuchEntity)?;
        Ok(entities.instances.insert(Instance {
            definition,
            transform,
            name: name.into(),
            layer: None,
            locked: false,
            hidden: false,
        }))
    }

    /// Whether `outer`'s geometry contains (at any depth) an instance of `target`.
    fn contains_definition(&self, outer: DefinitionId, target: DefinitionId) -> bool {
        let mut stack = vec![outer];
        let mut seen = Vec::new();
        while let Some(d) = stack.pop() {
            if seen.contains(&d) {
                continue;
            }
            seen.push(d);
            if let Some(ents) = self.geometry.get(&d) {
                for (_, inst) in ents.instances.iter() {
                    if inst.definition == target {
                        return true;
                    }
                    stack.push(inst.definition);
                }
            }
        }
        false
    }

    // ----- transactions ----------------------------------------------------

    /// Starts a transaction.
    ///
    /// # Errors
    /// [`ModelError::TransactionOpen`] if one is already open.
    pub fn begin(&mut self, name: impl Into<String>) -> Result<(), ModelError> {
        if self.open.is_some() {
            return Err(ModelError::TransactionOpen);
        }
        self.open = Some(name.into());
        self.root.begin_journal();
        self.definitions.begin_journal();
        self.layers.begin_journal();
        self.scenes.begin_journal();
        self.materials.begin_journal();
        self.look_before = Some(self.look);
        for g in self.geometry.values_mut() {
            g.begin_journal();
        }
        self.geometry_journal = Some(BTreeMap::new());
        Ok(())
    }

    /// Commits the open transaction as one undo step. Returns `false` if it
    /// changed nothing (no step is recorded). A step that changes anything
    /// on a locked layer is undone instead and refused.
    ///
    /// # Errors
    /// [`ModelError::NoTransaction`]; [`ModelError::Refused`]
    /// ([`ON_LOCKED_LAYER`]) for a change on a locked layer.
    pub fn commit(&mut self) -> Result<bool, ModelError> {
        // Nothing on a locked layer may change: the step is undone instead.
        if self.open.is_some() && self.touches_locked_layer() {
            self.abort()?;
            return Err(ModelError::Refused(ON_LOCKED_LAYER.into()));
        }
        let name = self.open.take().ok_or(ModelError::NoTransaction)?;
        // A shape or sweep edited by hand is no longer that shape or sweep:
        // its recipe goes, as part of this step. A recipe set in this step
        // is kept.
        let stale: Vec<DefinitionId> = self
            .geometry
            .iter()
            .filter(|(id, g)| g.geometry_touched() && !self.recipe_set.contains(id))
            .map(|(id, _)| *id)
            .filter(|id| {
                self.definitions
                    .get(*id)
                    .is_some_and(|d| d.shape.is_some() || d.sweep.is_some())
            })
            .collect();
        for id in stale {
            if let Some(d) = self.definitions.get_mut(id) {
                d.shape = None;
                d.sweep = None;
            }
        }
        self.recipe_set.clear();
        let root = self.root.take_journal();
        let definitions = self.definitions.take_journal();
        let layers = self.layers.take_journal();
        let scenes = self.scenes.take_journal();
        let materials = self.materials.take_journal();
        let look = self
            .look_before
            .take()
            .filter(|before| *before != self.look)
            .map(|before| (before, self.look));
        let mut edited = Vec::new();
        for (id, g) in &mut self.geometry {
            if g.journal_open() {
                let change = g.take_journal();
                if !change.is_empty() {
                    edited.push((*id, change));
                }
            }
        }
        let created_removed = self
            .geometry_journal
            .take()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(key, before)| {
                let after = self.geometry.get(&key).cloned();
                (before != after).then_some(Change { key, before, after })
            })
            .collect();
        let t = Transaction {
            name,
            serial: self.last_serial + 1,
            root,
            definitions,
            layers,
            scenes,
            materials,
            look,
            created_removed,
            edited,
        };
        if t.is_empty() {
            return Ok(false);
        }
        self.last_serial = t.serial;
        self.undo.push(t);
        self.redo.clear();
        Ok(true)
    }

    /// Abandons the open transaction, restoring the model exactly.
    ///
    /// # Errors
    /// [`ModelError::NoTransaction`].
    pub fn abort(&mut self) -> Result<(), ModelError> {
        self.open.take().ok_or(ModelError::NoTransaction)?;
        self.root.rollback_journal();
        for g in self.geometry.values_mut() {
            if g.journal_open() {
                g.rollback_journal();
            }
        }
        for (id, before) in self.geometry_journal.take().unwrap_or_default() {
            match before {
                Some(g) => {
                    self.geometry.insert(id, g);
                }
                None => {
                    self.geometry.remove(&id);
                }
            }
        }
        self.definitions.rollback_journal();
        self.layers.rollback_journal();
        self.scenes.rollback_journal();
        self.materials.rollback_journal();
        if let Some(before) = self.look_before.take() {
            self.look = before;
        }
        Ok(())
    }

    /// Runs `op` as one undoable step named `name`: committed if it returns
    /// `Ok`, rolled back completely if it returns `Err`.
    ///
    /// # Errors
    /// The operation's own error, or [`ModelError::TransactionOpen`].
    pub fn transaction<R>(
        &mut self,
        name: impl Into<String>,
        op: impl FnOnce(&mut Self) -> Result<R, ModelError>,
    ) -> Result<R, ModelError> {
        self.begin(name)?;
        match op(self) {
            Ok(r) => {
                self.commit()?;
                Ok(r)
            }
            Err(e) => {
                self.abort()?;
                Err(e)
            }
        }
    }

    /// Identifies the model state reached through the history: the serial of
    /// the step on top of the undo stack, or 0 with nothing to undo. Equal
    /// revisions mean equal content; a new step always gives a new revision.
    pub fn revision(&self) -> u64 {
        self.undo.last().map_or(0, |t| t.serial)
    }

    /// Whether there is a step to undo.
    pub fn can_undo(&self) -> bool {
        self.undo.last().is_some()
    }

    /// Whether there is a step to redo.
    pub fn can_redo(&self) -> bool {
        self.redo.last().is_some()
    }

    /// Name of the step `undo` would reverse.
    pub fn undo_name(&self) -> Option<&str> {
        self.undo.last().map(|t| t.name.as_str())
    }

    /// Name of the step `redo` would carry out again.
    pub fn redo_name(&self) -> Option<&str> {
        self.redo.last().map(|t| t.name.as_str())
    }

    /// Reverses the last committed step. Returns its name.
    pub fn undo(&mut self) -> Option<String> {
        if self.open.is_some() {
            return None;
        }
        let t = self.undo.pop()?;
        for (id, change) in &t.edited {
            if let Some(g) = self.geometry.get_mut(id) {
                g.apply_change(change, true);
            }
        }
        self.apply_geometry_snapshots(&t.created_removed, true);
        self.definitions.apply(&t.definitions, true);
        self.layers.apply(&t.layers, true);
        self.scenes.apply(&t.scenes, true);
        self.materials.apply(&t.materials, true);
        if let Some((before, _)) = t.look {
            self.look = before;
        }
        // Restored geometry and layers: new drawing still goes on the active
        // layer, or on Base if that layer no longer exists.
        self.set_active_layer(self.active_layer);
        self.root.apply_change(&t.root, true);
        let name = t.name.clone();
        self.redo.push(t);
        Some(name)
    }

    /// Re-applies the last undone step. Returns its name.
    pub fn redo(&mut self) -> Option<String> {
        if self.open.is_some() {
            return None;
        }
        let t = self.redo.pop()?;
        self.apply_geometry_snapshots(&t.created_removed, false);
        self.definitions.apply(&t.definitions, false);
        self.layers.apply(&t.layers, false);
        self.scenes.apply(&t.scenes, false);
        self.materials.apply(&t.materials, false);
        if let Some((_, after)) = t.look {
            self.look = after;
        }
        self.set_active_layer(self.active_layer);
        for (id, change) in &t.edited {
            if let Some(g) = self.geometry.get_mut(id) {
                g.apply_change(change, false);
            }
        }
        self.root.apply_change(&t.root, false);
        let name = t.name.clone();
        self.undo.push(t);
        Some(name)
    }

    fn apply_geometry_snapshots(&mut self, changes: &[Change<DefinitionId, Entities>], undo: bool) {
        for c in changes {
            let image = if undo { &c.before } else { &c.after };
            match image {
                Some(g) => {
                    self.geometry.insert(c.key, g.clone());
                }
                None => {
                    self.geometry.remove(&c.key);
                }
            }
        }
    }

    /// Builds a document from its parts with an empty history (file reading;
    /// see `snapshot.rs`).
    pub(crate) fn from_parts(
        root: Entities,
        definitions: Arena<DefinitionId, Definition>,
        layers: Arena<LayerId, Layer>,
        scenes: Arena<SceneId, Scene>,
        geometry: BTreeMap<DefinitionId, Entities>,
        active_layer: Option<LayerId>,
    ) -> Self {
        let mut d = Self {
            root,
            definitions,
            layers,
            scenes,
            recipe_set: Default::default(),
            geometry,
            ..Self::default()
        };
        d.set_active_layer(active_layer);
        d
    }

    /// Structural equality of content (root, definitions, geometry), ignoring history.
    pub fn same_content(&self, other: &Self) -> bool {
        self.root == other.root
            && self.definitions == other.definitions
            && self.layers == other.layers
            && self.scenes == other.scenes
            && self.materials == other.materials
            && self.look == other.look
            && self.active_layer == other.active_layer
            && self.geometry == other.geometry
    }
}

/// The corners of the box `b` carried by `m`.
fn box_corners((lo, hi): (DVec3, DVec3), m: &DMat4) -> [DVec3; 8] {
    std::array::from_fn(|i| {
        let p = DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        );
        m.transform_point3(p)
    })
}

impl Entities {
    /// Places a copy of instance `id` in this context, moved by `m` (applied
    /// after its own transform), and returns the copy.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`]; [`ModelError::NonFinite`].
    pub fn copy_instance(&mut self, id: InstanceId, m: &DMat4) -> Result<InstanceId, ModelError> {
        if !m.is_finite() {
            return Err(ModelError::NonFinite);
        }
        let mut copy = self
            .instances
            .get(id)
            .cloned()
            .ok_or(ModelError::NoSuchEntity)?;
        copy.transform = *m * copy.transform;
        Ok(self.instances.insert(copy))
    }

    /// Moves instance `id` by `m` (applied after its own transform).
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`]; [`ModelError::NonFinite`].
    pub fn transform_instance(&mut self, id: InstanceId, m: &DMat4) -> Result<(), ModelError> {
        if !m.is_finite() {
            return Err(ModelError::NonFinite);
        }
        let inst = self.instances.get_mut(id).ok_or(ModelError::NoSuchEntity)?;
        inst.transform = *m * inst.transform;
        Ok(())
    }

    /// Erases instance `id`. Its definition stays (the document removes
    /// definitions no instance uses when it is saved or purged).
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn erase_instance(&mut self, id: InstanceId) -> Result<(), ModelError> {
        self.instances
            .remove(id)
            .map(|_| ())
            .ok_or(ModelError::NoSuchEntity)
    }

    /// A placed instance by key.
    pub fn instance(&self, id: InstanceId) -> Option<&Instance> {
        self.instances.get(id)
    }

    /// All instance keys in this context.
    pub fn instance_ids(&self) -> impl Iterator<Item = InstanceId> + '_ {
        self.instances.keys()
    }

    /// Number of instances in this context.
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }
}
