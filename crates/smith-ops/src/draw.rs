//! SketchSmith — draw operations | v1.0.0 | Thursday 12 November 2026
//!
//! The geometry behind the draw tools. A tool works
//! out a [`DrawOp`] from clicks, snapping and typed values; [`draw`] builds it
//! with full sticky behaviour (splitting, auto-face, holes). The caller wraps the
//! call in a `Document::transaction`, so every draw is one undo step.
//!
//! Construction helpers used by the tools:
//! * [`rectangle_corners`] — four corners from a corner, two in-plane axes and
//!   two signed side lengths (exact: corner + axis × length).
//! * [`circumcircle`] — the circle through three points (circumcentre formula
//!   `A + (|b|² (n × a) + |a|² (b × n)) / (2 |n|²)`, with `a = B − A`,
//!   `b = C − A`, `n = a × b`).
//! * [`arc_from_bulge`] — the arc through a chord's ends with a given sagitta
//!   (bulge height): radius `R = (c²/4 + s²) / 2|s|`.
//! * [`tangent_bulge`] — the sagitta that makes an arc start tangent to a given
//!   direction: `s = (c/2) · tan(φ/2)`, where φ is the angle between the chord
//!   and the tangent.
//! * [`smooth_through`] — Bézier control points for a smooth curve through
//!   clicked points (Catmull–Rom tangents: each interior point's tangent is half
//!   the vector between its neighbours; each span's inner control points sit a
//!   third of a tangent from its ends).
//!
//! [`DrawOp::Guide`], [`DrawOp::EraseGuide`] and [`DrawOp::DeleteGuides`]
//! add and erase guides, and [`DrawOp::Annotate`] and
//! [`DrawOp::EraseAnnotation`] dimensions and text, and
//! [`DrawOp::Section`], [`DrawOp::ActivateSection`] and
//! [`DrawOp::EraseSection`] section planes; they never touch geometry.
//!
//! [`DrawOp::EditCurve`] replaces an existing curve's shape in place (Edit
//! Curve).

use smith_geom::curve::{Arc, Bezier, Circle, Curve, CurveFrame, Helix, Polygon};
use smith_geom::tolerance::TOL_MERGE;
use smith_geom::{DVec3, GeomError};
use smith_model::{
    Annotation, AnnotationId, CurveId, EdgeId, Entities, FaceId, Guide, GuideId, ModelError,
    Pickable, SectionId, SectionPlane, VertexId,
};

/// A circle or arc placement: centre, the direction of its first point, and
/// its normal (the curve runs counter-clockwise about the normal).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Centre point.
    pub center: DVec3,
    /// Direction from the centre to the curve's first point.
    pub x_axis: DVec3,
    /// Plane normal.
    pub normal: DVec3,
}

impl Placement {
    fn frame(&self) -> Result<CurveFrame, GeomError> {
        CurveFrame::new(self.center, self.x_axis, self.normal)
    }
}

/// One draw operation, fully specified.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawOp {
    /// A single line segment.
    Line {
        /// Start.
        a: DVec3,
        /// End.
        b: DVec3,
    },
    /// A closed four-sided outline (Rectangle and Rotated Rectangle).
    Rectangle {
        /// Corners in order.
        corners: [DVec3; 4],
    },
    /// A circle curve.
    Circle {
        /// Placement; the first segment starts at `center + x_axis × radius`.
        at: Placement,
        /// Radius, mm.
        radius: f64,
        /// Segment count.
        segments: u32,
    },
    /// An arc curve, from angle 0 (along `x_axis`) to `sweep`.
    Arc {
        /// Placement; the arc starts at `center + x_axis × radius`.
        at: Placement,
        /// Radius, mm.
        radius: f64,
        /// Swept angle, radians, in (0, 2π).
        sweep: f64,
        /// Segment count.
        segments: u32,
    },
    /// A regular polygon curve; the first corner lies along `x_axis`.
    Polygon {
        /// Placement.
        at: Placement,
        /// Centre-to-corner radius, mm.
        radius: f64,
        /// Number of sides (at least 3).
        sides: u32,
    },
    /// A Bézier spline from its control points (`3n + 1` of them).
    Bezier {
        /// Control points.
        control_points: Vec<DVec3>,
        /// Segments per cubic span.
        segments: u32,
    },
    /// A Curve Maker helix, conical helix or flat spiral.
    Helix {
        /// Axis start (`center`), start direction (`x_axis`), axis (`normal`).
        at: Placement,
        /// Radius at the start, mm.
        start_radius: f64,
        /// Radius at the end, mm.
        end_radius: f64,
        /// Rise per turn, mm (0 for a flat spiral).
        pitch: f64,
        /// Number of turns.
        turns: f64,
        /// Turns clockwise about the axis.
        left_handed: bool,
        /// Segments along the whole curve.
        segments: u32,
    },
    /// Replaces an existing curve's shape and segment count (Edit Curve).
    EditCurve {
        /// The curve to change.
        id: CurveId,
        /// Its new shape.
        curve: Curve,
        /// Its new segment count.
        segments: u32,
    },
    /// Extrude a face (see [`crate::modify::extrude`]).
    Extrude {
        /// The face.
        face: FaceId,
        /// Distance along its normal, mm (negative pushes back).
        distance: f64,
        /// Keep the original face (the Ctrl option).
        new_start: bool,
    },
    /// Move or copy edges and faces (see [`crate::modify::move_items`]).
    Move {
        /// What moves.
        items: Vec<Pickable>,
        /// The move, mm.
        delta: DVec3,
        /// Copy instead of moving (the Ctrl option).
        copy: bool,
        /// Number of copies (`5x`, or `/5` with `divide`).
        copies: u32,
        /// Spread the copies evenly up to `delta` (`/N`).
        divide: bool,
        /// Proportional editing (moves only, not copies).
        proportional: Option<crate::modify::Proportional>,
    },
    /// Rotate, scale or mirror edges and faces, or copy them (see
    /// [`crate::modify::transform_items`]).
    Transform {
        /// What changes.
        items: Vec<Pickable>,
        /// The rotation, scale or mirror.
        transform: crate::modify::Transform,
        /// Copy instead of changing in place.
        copy: bool,
        /// Number of copies.
        copies: u32,
        /// Spread the copies within one step (`/N`).
        divide: bool,
    },
    /// Offset (Inset) a face's outline (see [`crate::shape::offset_face`]).
    Offset {
        /// The face.
        face: FaceId,
        /// Distance inward, mm (negative: outward).
        distance: f64,
        /// How many offsets, one step apart.
        count: u32,
    },
    /// Offset the chain of edges through an edge (see
    /// [`crate::shape::offset_chain`]).
    OffsetChain {
        /// An edge of the chain.
        edge: EdgeId,
        /// Distance to the chain's left about `normal`, mm (negative: right).
        distance: f64,
        /// How many offsets, one step apart.
        count: u32,
        /// The plane's normal when the chain is a straight line.
        normal: DVec3,
    },
    /// Divide an edge (see [`crate::shape::divide_edge`]).
    Divide {
        /// The edge.
        edge: EdgeId,
        /// Measure from the edge's start (else its end).
        from_start: bool,
        /// Where the points go.
        division: crate::shape::Division,
    },
    /// Loop Cut the ring of four-sided faces through an edge.
    LoopCut {
        /// An edge the cuts cross.
        edge: EdgeId,
        /// Number of cuts.
        cuts: u32,
        /// Where they go (even, or measured along the side edges).
        spacing: crate::cut::Spacing,
    },
    /// Cut along a line drawn in the view (see [`crate::cut::knife`]).
    Knife {
        /// The points drawn, in order.
        points: Vec<DVec3>,
        /// How the view looked when they were drawn.
        view: crate::cut::View,
        /// Also cut faces turned away from the view.
        through: bool,
    },
    /// Align the end points of edges and faces to one X, Y or Z value.
    Align {
        /// What aligns.
        items: Vec<Pickable>,
        /// Unit axis.
        axis: DVec3,
        /// The shared value along it, mm.
        value: f64,
    },
    /// Slide a point or an edge (see [`crate::shape::slide`]).
    Slide {
        /// What slides.
        target: crate::shape::SlideTarget,
        /// How far, mm.
        distance: f64,
    },
    /// Remove a point between two edges in a straight line.
    Dissolve {
        /// The point.
        vertex: smith_model::VertexId,
    },
    /// Reverse faces.
    ReverseFaces {
        /// The faces.
        faces: Vec<FaceId>,
    },
    /// Turn every face connected to one face the same way as it.
    OrientFaces {
        /// The face the others follow.
        face: FaceId,
    },
    /// Erase edges, healing coplanar faces, or hide them.
    Erase {
        /// The edges.
        edges: Vec<EdgeId>,
        /// Hide instead of erasing.
        hide: bool,
    },
    /// Add a guide line or guide point (Tape Measure, Protractor, Find
    /// Center).
    Guide {
        /// The guide.
        guide: Guide,
    },
    /// Erase one guide.
    EraseGuide {
        /// The guide.
        guide: GuideId,
    },
    /// Erase every guide in the context.
    DeleteGuides,
    /// Add a dimension or text (Dimensions, Text).
    Annotate {
        /// The dimension or text.
        annotation: Annotation,
    },
    /// Erase one dimension or text.
    EraseAnnotation {
        /// The dimension or text.
        annotation: AnnotationId,
    },
    /// Add a section plane and make it the active one.
    Section {
        /// The plane.
        plane: SectionPlane,
    },
    /// Make a section plane the active one (`None`: none active).
    ActivateSection {
        /// The plane.
        section: Option<SectionId>,
    },
    /// Erase one section plane.
    EraseSection {
        /// The plane.
        section: SectionId,
    },
    /// Sweep a profile along a path (see [`crate::sweep`]). The canvas makes
    /// it a group that keeps its recipe; here, on loose geometry, it is
    /// built in place.
    Sweep {
        /// The path's points, in order.
        path: Vec<DVec3>,
        /// The path closes on itself.
        closed: bool,
        /// A second path (open, as the first) the profile keeps spanning.
        path2: Option<Vec<DVec3>>,
        /// The profile.
        profile: crate::sweep::SweepProfile,
        /// Sets a typed tube's first frame axis (made square to the path).
        up: DVec3,
        /// Taper, Twist, Upright, Align to Path.
        options: crate::sweep::SweepOptions,
    },
    /// Place a shape (see [`crate::shapes`]). The canvas makes it a group;
    /// here, on loose geometry, it is built in place.
    Shape {
        /// The shape and its sizes.
        spec: crate::shapes::ShapeSpec,
        /// Where it goes: its origin, X, Y and Z (Z up out of the face).
        transform: smith_geom::DMat4,
    },
    /// Rebuild a shape's group with new sizes (Edit Shape; the canvas runs
    /// it on the group's definition).
    EditShape {
        /// The group's definition.
        definition: smith_model::DefinitionId,
        /// The new sizes.
        spec: crate::shapes::ShapeSpec,
    },
    /// Loft profile faces along a path (see [`crate::sweep::loft`]) or by
    /// spline ([`crate::sweep::loft_spline`]); a group on the canvas, as
    /// Sweep.
    Loft {
        /// The path's points, start first; `None` for Loft.
        path: Option<Vec<DVec3>>,
        /// The path closes on itself.
        closed: bool,
        /// The profile faces: across the path, or in loft order.
        profiles: Vec<FaceId>,
        /// The drawing axes' Z.
        up: DVec3,
        /// Vertices, Start Shift, Segments, Twist and Edges Only.
        options: smith_model::LoftOptions,
        /// A second path (with a first) the profiles keep spanning.
        path2: Option<Vec<DVec3>>,
        /// Each profile's own turn, radians, in `profiles` order.
        twists: Vec<f64>,
    },
    /// Skin a frame of lines (Skinning): each cell of three or four sides
    /// covered. A group on the canvas, as the extrusions.
    Skin {
        /// The frame's segments.
        segments: Vec<[DVec3; 2]>,
        /// Pieces each way across a cell.
        sampling: u32,
        /// The lines only, no faces.
        edges_only: bool,
    },
    /// Extrude bare edges along a path (Extrude Edges along Path): a
    /// surface. One profile is carried along; two are blended between, as a
    /// loft. A group on the canvas, as the other extrusions.
    ExtrudeEdges {
        /// The path's points, start first.
        path: Vec<DVec3>,
        /// The path closes on itself (one profile only).
        closed: bool,
        /// A second path the profiles keep spanning.
        path2: Option<Vec<DVec3>>,
        /// The edge chains' points: one, or a start and an end.
        profiles: Vec<Vec<DVec3>>,
        /// The chains close on themselves.
        loops: bool,
        /// The drawing axes' Z.
        up: DVec3,
        /// Vertices, Segments and Twist (two profiles; Twist alone for one).
        options: smith_model::LoftOptions,
    },
    /// Copy one group or component along a path (Copy Along Path): copies
    /// of it, as instances of its definition, carried and turned by the
    /// path's frames. Only the canvas runs it (instances belong to the
    /// document).
    CopyAlongPath {
        /// The group or component copied.
        instance: smith_model::InstanceId,
        /// The path's points, start first.
        path: Vec<DVec3>,
        /// The path closes on itself.
        closed: bool,
        /// Copies spread evenly to the path's end (when `spacing` is 0).
        count: u32,
        /// A copy every this far along the path (mm; 0 uses `count`).
        spacing: f64,
        /// The drawing axes' Z (the frames' first axis).
        up: DVec3,
    },
    /// Trim or extend two edges to where their lines meet (see
    /// [`crate::extend::trim_extend`]).
    TrimExtend {
        /// The first edge, by its ends.
        a: (DVec3, DVec3),
        /// The second.
        b: (DVec3, DVec3),
    },
    /// Extend an edge's end to a face's plane.
    ExtendToFace {
        /// The edge, by its ends.
        edge: (DVec3, DVec3),
        /// Near the end to extend.
        end: DVec3,
        /// A point on the face.
        point: DVec3,
        /// The face's normal.
        normal: DVec3,
    },
    /// Join two open loops with a band of faces (see
    /// [`crate::extend::bridge`]).
    Bridge {
        /// The first loop's points.
        a: Vec<DVec3>,
        /// The second's.
        b: Vec<DVec3>,
        /// Points round each (at least the most either has).
        vertices: u32,
        /// Rings across the band.
        segments: u32,
    },
    /// Move a chain's or selection's points: Space, Circle, Flatten, Smooth
    /// or Relax (see [`crate::tweak`]).
    Tweak(crate::tweak::Tweak),
    /// Clean Up the context's geometry (see [`crate::cleanup`]).
    CleanUp(crate::cleanup::CleanUp),
    /// Intersect Faces (see [`crate::intersect`]).
    Intersect(crate::intersect::IntersectFaces),
    /// Close Gaps (see [`crate::gaps`]).
    CloseGaps(crate::gaps::CloseGaps),
    /// Inspect Faces: finds only, changes nothing (see [`crate::inspect`]).
    Inspect(crate::inspect::InspectFaces),
    /// Inspect Manifold: finds only, changes nothing (see [`crate::solid`]).
    InspectSolid(crate::solid::InspectSolid),
    /// Make Manifold (see [`crate::manifold`]).
    MakeManifold(crate::manifold::MakeManifold),
    /// Fuse: run by the canvas with the document (see [`crate::fuse`]);
    /// here, only two faces.
    Fuse(crate::fuse::Fuse),
    /// Fill: faces for the flat loops of `edges`, or `vertices` joined in
    /// order (see `smith_model::Entities::fill`).
    Fill {
        /// The selected vertices, in pick order.
        vertices: Vec<VertexId>,
        /// The selected edges (they win over the vertices).
        edges: Vec<EdgeId>,
    },
    /// Weld a chain of edges into one curve (see [`crate::weld`]).
    Weld {
        /// The chain's points, in order.
        points: Vec<DVec3>,
        /// The chain closes on itself.
        closed: bool,
        /// Points the curve has (at least the chain's own).
        vertices: u32,
    },
    /// Bevel straight edges between flat faces (see [`crate::bevel`]).
    Bevel {
        /// The edges, by their ends.
        edges: Vec<(DVec3, DVec3)>,
        /// Corners of flat outlines, rounded by `width` as a radius.
        corners: Vec<DVec3>,
        /// How far the bevel cuts back along each face (edges), or the
        /// radius (corners), mm.
        width: f64,
        /// 1 a flat chamfer, more a round fillet.
        segments: u32,
    },
    /// Rebuild a Sweep or Revolve group from a changed recipe (Edit Sweep;
    /// the canvas runs it on the group's definition).
    EditSweep {
        /// The group's definition.
        definition: smith_model::DefinitionId,
        /// The new recipe, in the group's own coordinates.
        recipe: smith_model::SweepRecipe,
    },
    /// Revolve a face about an axis (see [`crate::sweep::revolve`]); a
    /// group on the canvas, as Sweep.
    Revolve {
        /// The profile face.
        face: FaceId,
        /// A point on the axis.
        axis_point: DVec3,
        /// The axis direction.
        axis_dir: DVec3,
        /// Angle, radians (a full turn closes).
        angle: f64,
        /// Steps round.
        steps: u32,
    },
}

impl DrawOp {
    /// The undo-history name of the operation.
    pub fn name(&self) -> &'static str {
        match self {
            DrawOp::Line { .. } => "Line",
            DrawOp::Rectangle { .. } => "Rectangle",
            DrawOp::Circle { .. } => "Circle",
            DrawOp::Arc { .. } => "Arc",
            DrawOp::Polygon { .. } => "Polygon",
            DrawOp::Bezier { .. } => "Bézier Spline",
            DrawOp::Helix { .. } => "Curve Maker",
            DrawOp::EditCurve { .. } => "Edit Curve",
            DrawOp::Extrude { .. } => "Extrude",
            DrawOp::Move { copy: false, .. } => "Move",
            DrawOp::Move { copy: true, .. } => "Copy",
            DrawOp::Transform { transform, .. } => match transform {
                crate::modify::Transform::Rotate { .. } => "Rotate",
                crate::modify::Transform::Scale { .. } => "Scale",
                crate::modify::Transform::Mirror { .. } => "Mirror",
                crate::modify::Transform::Matrix { .. } => "Repeat Transform",
            },
            DrawOp::Offset { .. } | DrawOp::OffsetChain { .. } => "Offset",
            DrawOp::Divide { .. } => "Divide",
            DrawOp::LoopCut { .. } => "Loop Cut",
            DrawOp::Align { .. } => "Align",
            DrawOp::Knife { .. } => "Knife",
            DrawOp::Slide { .. } => "Slide",
            DrawOp::Dissolve { .. } => "Dissolve",
            DrawOp::ReverseFaces { .. } => "Reverse Faces",
            DrawOp::OrientFaces { .. } => "Orient Faces",
            DrawOp::Erase { hide: false, .. } => "Erase",
            DrawOp::Erase { hide: true, .. } => "Hide",
            DrawOp::Guide { .. } => "Guide",
            DrawOp::EraseGuide { .. } => "Erase Guide",
            DrawOp::DeleteGuides => "Delete Guides",
            DrawOp::Annotate {
                annotation: Annotation::Leader { .. } | Annotation::Screen { .. },
            } => "Text",
            DrawOp::Annotate { .. } => "Dimension",
            DrawOp::EraseAnnotation { .. } | DrawOp::EraseSection { .. } => "Erase",
            DrawOp::Section { .. } => "Section Plane",
            DrawOp::Sweep { .. } => "Extrude along Path",
            DrawOp::Revolve { .. } => "Revolve",
            DrawOp::Shape { spec, .. } => spec.name(),
            DrawOp::EditShape { .. } => "Edit Shape",
            DrawOp::EditSweep { .. } => "Edit",
            DrawOp::Loft { path: Some(_), .. } => "Loft along Path",
            DrawOp::Loft { path: None, .. } => "Loft",
            DrawOp::ExtrudeEdges { .. } => "Extrude Edges along Path",
            DrawOp::Skin { .. } => "Skinning",
            DrawOp::Bevel { .. } => "Bevel",
            DrawOp::Weld { .. } => "Weld",
            DrawOp::Fill { .. } => "Fill",
            DrawOp::CleanUp(_) => "Clean Up",
            DrawOp::Intersect(_) => "Intersect Faces",
            DrawOp::CloseGaps(_) => "Close Gaps",
            DrawOp::Inspect(_) => "Inspect Faces",
            DrawOp::InspectSolid(_) => "Inspect Manifold",
            DrawOp::MakeManifold(_) => "Make Manifold",
            DrawOp::Fuse(_) => "Fuse",
            DrawOp::TrimExtend { .. } => "Trim / Extend",
            DrawOp::ExtendToFace { .. } => "Extend to Face",
            DrawOp::Bridge { .. } => "Bridge",
            DrawOp::Tweak(t) => match t.kind {
                crate::tweak::TweakKind::Space => "Space",
                crate::tweak::TweakKind::Circle => "Circle Loop",
                crate::tweak::TweakKind::Flatten => "Flatten",
                crate::tweak::TweakKind::Smooth => "Smooth",
                crate::tweak::TweakKind::Relax => "Relax",
            },
            DrawOp::CopyAlongPath { .. } => "Copy Along Path",
            DrawOp::ActivateSection { section: Some(_) } => "Activate Section",
            DrawOp::ActivateSection { section: None } => "Section Off",
        }
    }
}

/// What a draw created.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DrawReport {
    /// Edges created (after splitting).
    pub edges: Vec<EdgeId>,
    /// The curve entity, for circles, arcs and polygons.
    pub curve: Option<CurveId>,
    /// What a repair tool found or did, one line each (Clean Up).
    pub report: Vec<crate::cleanup::ReportLine>,
    /// A hint to show after the run, if any.
    pub hint: Option<&'static str>,
    /// The run left something only Auto-Tri can do (Make Manifold's bent
    /// holes): offer it for this run.
    pub offer_auto_tri: bool,
}

/// What an extrusion or loft op is made from, as
/// the recipe its group keeps (picked faces read as their outlines). `None`
/// for any other op.
///
/// # Errors
/// As [`crate::sweep::profile_outline`] for a picked face.
pub fn recipe_of(
    entities: &smith_model::Entities,
    op: &DrawOp,
) -> Option<Result<smith_model::SweepRecipe, ModelError>> {
    Some(match op {
        DrawOp::Skin {
            segments,
            sampling,
            edges_only,
        } => Ok(smith_model::SweepRecipe::Skin {
            segments: segments.clone(),
            sampling: *sampling,
            edges_only: *edges_only,
        }),
        DrawOp::Sweep {
            path,
            closed,
            path2,
            profile,
            up,
            options,
        } => crate::sweep::sweep_recipe(
            entities,
            path,
            *closed,
            path2.as_deref(),
            *profile,
            *up,
            *options,
        ),
        DrawOp::Loft {
            path,
            closed,
            profiles,
            up,
            options,
            path2,
            twists,
        } => profiles
            .iter()
            .map(|f| crate::sweep::profile_outline(entities, *f))
            .collect::<Result<Vec<_>, _>>()
            .map(|profiles| smith_model::SweepRecipe::Loft {
                path: path.clone(),
                profiles,
                up: *up,
                options: *options,
                path2: path2.clone(),
                edges: None,
                closed: *closed,
                twists: twists.clone(),
            }),
        DrawOp::ExtrudeEdges {
            path,
            closed,
            path2,
            profiles,
            loops,
            up,
            options,
        } => Ok(match profiles.as_slice() {
            [one] => smith_model::SweepRecipe::Sweep {
                path: path.clone(),
                closed: *closed,
                path2: path2.clone(),
                section: smith_model::SweepSection::Edges {
                    points: one.clone(),
                    closed: *loops,
                },
                up: *up,
                taper: 1.0,
                twist: options.twist,
                upright: false,
                align: false,
            },
            _ => smith_model::SweepRecipe::Loft {
                path: Some(path.clone()),
                profiles: profiles.clone(),
                up: *up,
                options: *options,
                path2: path2.clone(),
                edges: Some(*loops),
                closed: false,
                twists: Vec::new(),
            },
        }),
        _ => return None,
    })
}

/// Builds `op` into `entities` with full sticky behaviour.
///
/// # Errors
/// [`ModelError::Geometry`] for degenerate input (zero radius, too few
/// segments or sides, an invalid sweep); otherwise as `Entities::add_line`.
pub fn draw(entities: &mut Entities, op: &DrawOp) -> Result<DrawReport, ModelError> {
    match op {
        DrawOp::Line { a, b } => Ok(DrawReport {
            edges: entities.add_line(*a, *b)?,
            ..DrawReport::default()
        }),
        DrawOp::Rectangle { corners } => Ok(DrawReport {
            edges: entities.add_polyline(corners, true)?,
            ..DrawReport::default()
        }),
        DrawOp::Circle {
            at,
            radius,
            segments,
        } => {
            let curve = Curve::Circle(Circle::new(at.frame()?, *radius)?);
            curve_report(entities, curve, *segments)
        }
        DrawOp::Arc {
            at,
            radius,
            sweep,
            segments,
        } => {
            let curve = Curve::Arc(Arc::new(at.frame()?, *radius, 0.0, *sweep)?);
            curve_report(entities, curve, *segments)
        }
        DrawOp::Polygon { at, radius, sides } => {
            let curve = Curve::Polygon(Polygon::new(at.frame()?, *radius, *sides)?);
            curve_report(entities, curve, *sides)
        }
        DrawOp::Bezier {
            control_points,
            segments,
        } => {
            let curve = Curve::Bezier(Bezier::new(control_points.clone())?);
            curve_report(entities, curve, *segments)
        }
        DrawOp::Helix {
            at,
            start_radius,
            end_radius,
            pitch,
            turns,
            left_handed,
            segments,
        } => {
            let helix = Helix::new(
                at.frame()?,
                *start_radius,
                *end_radius,
                *pitch,
                *turns,
                *left_handed,
            )?;
            curve_report(entities, Curve::Helix(helix), *segments)
        }
        DrawOp::EditCurve {
            id,
            curve,
            segments,
        } => {
            entities.set_curve(*id, curve.clone(), *segments)?;
            let edges = entities
                .curve(*id)
                .map(|r| r.edges().to_vec())
                .unwrap_or_default();
            Ok(DrawReport {
                edges,
                curve: Some(*id),
                ..DrawReport::default()
            })
        }
        DrawOp::Extrude {
            face,
            distance,
            new_start,
        } => {
            crate::modify::extrude(entities, *face, *distance, *new_start)?;
            Ok(DrawReport::default())
        }
        DrawOp::Move {
            items,
            delta,
            copy,
            copies,
            divide,
            proportional,
        } => {
            match proportional {
                Some(p) if !*copy => crate::modify::move_proportional(entities, items, *delta, *p)?,
                _ => crate::modify::move_items(entities, items, *delta, *copy, *copies, *divide)?,
            }
            Ok(DrawReport::default())
        }
        DrawOp::Transform {
            items,
            transform,
            copy,
            copies,
            divide,
        } => {
            crate::modify::transform_items(entities, items, transform, *copy, *copies, *divide)?;
            Ok(DrawReport::default())
        }
        DrawOp::Offset {
            face,
            distance,
            count,
        } => {
            crate::shape::offset_face(entities, *face, *distance, *count)?;
            Ok(DrawReport::default())
        }
        DrawOp::OffsetChain {
            edge,
            distance,
            count,
            normal,
        } => {
            crate::shape::offset_chain(entities, *edge, *distance, *count, *normal)?;
            Ok(DrawReport::default())
        }
        DrawOp::Divide {
            edge,
            from_start,
            division,
        } => {
            crate::shape::divide_edge(entities, *edge, *from_start, *division)?;
            Ok(DrawReport::default())
        }
        DrawOp::LoopCut {
            edge,
            cuts,
            spacing,
        } => {
            crate::cut::loop_cut(entities, *edge, *cuts, *spacing)?;
            Ok(DrawReport::default())
        }
        DrawOp::Knife {
            points,
            view,
            through,
        } => {
            crate::cut::knife(entities, points, view, *through)?;
            Ok(DrawReport::default())
        }
        DrawOp::Align { items, axis, value } => {
            crate::cut::align_items(entities, items, *axis, *value)?;
            Ok(DrawReport::default())
        }
        DrawOp::Slide { target, distance } => {
            crate::shape::slide(entities, *target, *distance)?;
            Ok(DrawReport::default())
        }
        DrawOp::Dissolve { vertex } => {
            entities.dissolve_vertex(*vertex)?;
            Ok(DrawReport::default())
        }
        DrawOp::ReverseFaces { faces } => {
            crate::shape::reverse_faces(entities, faces)?;
            Ok(DrawReport::default())
        }
        DrawOp::OrientFaces { face } => {
            entities.orient_faces_from(*face)?;
            Ok(DrawReport::default())
        }
        DrawOp::Erase { edges, hide } => {
            crate::modify::erase(entities, edges, *hide)?;
            Ok(DrawReport::default())
        }
        DrawOp::Guide { guide } => {
            entities.add_guide(*guide)?;
            Ok(DrawReport::default())
        }
        DrawOp::EraseGuide { guide } => {
            entities.remove_guide(*guide)?;
            Ok(DrawReport::default())
        }
        DrawOp::DeleteGuides => {
            if entities.clear_guides() == 0 {
                return Err(ModelError::NoSuchEntity);
            }
            Ok(DrawReport::default())
        }
        DrawOp::Annotate { annotation } => {
            entities.add_annotation(annotation.clone())?;
            Ok(DrawReport::default())
        }
        DrawOp::EraseAnnotation { annotation } => {
            entities.remove_annotation(*annotation)?;
            Ok(DrawReport::default())
        }
        DrawOp::Sweep { .. }
        | DrawOp::Loft { .. }
        | DrawOp::ExtrudeEdges { .. }
        | DrawOp::Skin { .. } => {
            let recipe = recipe_of(entities, op).ok_or(ModelError::NoSuchEntity)??;
            crate::sweep::build_sweep(entities, &recipe)?;
            Ok(DrawReport::default())
        }
        DrawOp::Shape { spec, transform } => {
            let before: std::collections::HashSet<smith_model::VertexId> =
                entities.vertex_ids().collect();
            crate::shapes::build_shape(entities, *spec)?;
            let made: Vec<smith_model::VertexId> = entities
                .vertex_ids()
                .filter(|v| !before.contains(v))
                .collect();
            entities.transform_vertices(&made, transform)?;
            Ok(DrawReport::default())
        }
        // Only meaningful on a definition or the document; the canvas
        // handles them there.
        DrawOp::EditShape { .. } | DrawOp::EditSweep { .. } | DrawOp::CopyAlongPath { .. } => {
            Err(ModelError::NoSuchEntity)
        }
        DrawOp::Revolve {
            face,
            axis_point,
            axis_dir,
            angle,
            steps,
        } => {
            crate::sweep::revolve(entities, *face, *axis_point, *axis_dir, *angle, *steps)?;
            Ok(DrawReport::default())
        }
        DrawOp::TrimExtend { a, b } => {
            crate::extend::trim_extend(entities, *a, *b)?;
            Ok(DrawReport::default())
        }
        DrawOp::ExtendToFace {
            edge,
            end,
            point,
            normal,
        } => {
            crate::extend::extend_to_face(entities, *edge, *end, *point, *normal)?;
            Ok(DrawReport::default())
        }
        DrawOp::Bridge {
            a,
            b,
            vertices,
            segments,
        } => {
            crate::extend::bridge(entities, a, b, *vertices, *segments)?;
            Ok(DrawReport::default())
        }
        DrawOp::Tweak(t) => {
            crate::tweak::tweak(entities, t)?;
            Ok(DrawReport::default())
        }
        DrawOp::CleanUp(c) => Ok(DrawReport {
            report: crate::cleanup::clean_up(entities, c)?,
            ..DrawReport::default()
        }),
        DrawOp::Fuse(f) => match (f.base, f.target) {
            (Pickable::Face(b), Pickable::Face(t)) => {
                entities.fuse_faces(b, t, f.op)?;
                Ok(DrawReport::default())
            }
            _ => Err(ModelError::Refused(crate::fuse::PICK_TWO.into())),
        },
        DrawOp::MakeManifold(m) => {
            let (report, bent) = crate::manifold::make_manifold(entities, m)?;
            Ok(DrawReport {
                report,
                offer_auto_tri: bent,
                ..DrawReport::default()
            })
        }
        DrawOp::InspectSolid(i) => Ok(DrawReport {
            report: crate::solid::inspect_solid(entities, i)?,
            ..DrawReport::default()
        }),
        DrawOp::Inspect(i) => Ok(DrawReport {
            report: crate::inspect::inspect_faces(entities, i)?,
            ..DrawReport::default()
        }),
        DrawOp::CloseGaps(g) => Ok(DrawReport {
            report: crate::gaps::close_gaps(entities, g)?,
            ..DrawReport::default()
        }),
        DrawOp::Intersect(i) => {
            let against = crate::intersect::against_here(entities, i);
            let (report, hint) = crate::intersect::intersect(entities, i, &against)?;
            Ok(DrawReport {
                report,
                hint,
                ..DrawReport::default()
            })
        }
        DrawOp::Fill { vertices, edges } => {
            entities.fill(vertices, edges)?;
            Ok(DrawReport::default())
        }
        DrawOp::Weld {
            points,
            closed,
            vertices,
        } => {
            crate::weld::weld(entities, points, *closed, *vertices)?;
            Ok(DrawReport::default())
        }
        DrawOp::Bevel {
            edges,
            corners,
            width,
            segments,
        } => {
            if !edges.is_empty() {
                crate::bevel::bevel(entities, edges, *width, *segments)?;
            }
            if !corners.is_empty() {
                crate::bevel::round_corners(entities, corners, *width, *segments)?;
            }
            Ok(DrawReport::default())
        }
        DrawOp::Section { plane } => {
            entities.add_section(*plane)?;
            Ok(DrawReport::default())
        }
        DrawOp::ActivateSection { section } => {
            entities.set_active_section(*section)?;
            Ok(DrawReport::default())
        }
        DrawOp::EraseSection { section } => {
            entities.remove_section(*section)?;
            Ok(DrawReport::default())
        }
    }
}

/// Bézier control points for a smooth curve through `points` (at least two):
/// `3(n − 1) + 1` control points, passing through every given point.
pub fn smooth_through(points: &[DVec3]) -> Vec<DVec3> {
    let n = points.len();
    if n < 2 {
        return points.to_vec();
    }
    let tangent = |i: usize| match i {
        0 => points[1] - points[0],
        i if i == n - 1 => points[n - 1] - points[n - 2],
        i => (points[i + 1] - points[i - 1]) * 0.5,
    };
    let mut out = Vec::with_capacity(3 * (n - 1) + 1);
    out.push(points[0]);
    for i in 0..n - 1 {
        out.push(points[i] + tangent(i) / 3.0);
        out.push(points[i + 1] - tangent(i + 1) / 3.0);
        out.push(points[i + 1]);
    }
    out
}

fn curve_report(
    entities: &mut Entities,
    curve: Curve,
    segments: u32,
) -> Result<DrawReport, ModelError> {
    let id = entities.add_curve(curve, segments)?;
    let edges = entities
        .curve(id)
        .map(|r| r.edges().to_vec())
        .unwrap_or_default();
    Ok(DrawReport {
        edges,
        curve: Some(id),
        ..DrawReport::default()
    })
}

/// The four corners of a rectangle: `p0`, then along `u` by `du`, then along
/// `v` by `dv`. `u` and `v` are unit, perpendicular, in-plane directions.
pub fn rectangle_corners(p0: DVec3, u: DVec3, v: DVec3, du: f64, dv: f64) -> [DVec3; 4] {
    [p0, p0 + u * du, p0 + u * du + v * dv, p0 + v * dv]
}

/// The circle through three points: (centre, unit normal, radius). `None` if
/// the points are collinear or coincident.
pub fn circumcircle(p1: DVec3, p2: DVec3, p3: DVec3) -> Option<(DVec3, DVec3, f64)> {
    let a = p2 - p1;
    let b = p3 - p1;
    let n = a.cross(b);
    let n2 = n.length_squared();
    let scale = a.length_squared().max(b.length_squared());
    if n2 <= 1e-18 * scale * scale || scale < TOL_MERGE * TOL_MERGE {
        return None;
    }
    let center =
        p1 + (n.cross(a) * b.length_squared() + b.cross(n) * a.length_squared()) / (2.0 * n2);
    let radius = (p1 - center).length();
    Some((center, n / n2.sqrt(), radius))
}

/// The arc from `a` to `b` bulging `sagitta` along `normal × (b − a)`
/// (negative bulges the other way). Returns the placement, radius and sweep
/// for a [`DrawOp::Arc`]. `None` for a zero chord or a flat bulge.
pub fn arc_from_bulge(
    a: DVec3,
    b: DVec3,
    sagitta: f64,
    normal: DVec3,
) -> Option<(Placement, f64, f64)> {
    let chord = b - a;
    let c = chord.length();
    if c < TOL_MERGE || sagitta.abs() < TOL_MERGE || !sagitta.is_finite() {
        return None;
    }
    let e = chord / c;
    let n = normal.try_normalize()?;
    let u = n.cross(e).try_normalize()?;
    let s = sagitta.abs();
    let radius = (c * c / 4.0 + s * s) / (2.0 * s);
    let side = u * sagitta.signum();
    let mid = (a + b) * 0.5;
    let center = mid - side * (radius - s);
    let bulge = mid + side * s;
    let x = (a - center) / radius;
    // Choose the normal so the counter-clockwise sweep from `a` meets the
    // bulge point before `b`.
    for nn in [n, -n] {
        let y = nn.cross(x);
        let ang = |p: DVec3| {
            let w = p - center;
            w.dot(y).atan2(w.dot(x)).rem_euclid(std::f64::consts::TAU)
        };
        let (ab, ap) = (ang(b), ang(bulge));
        if ap < ab {
            let at = Placement {
                center,
                x_axis: x,
                normal: nn,
            };
            return Some((at, radius, ab));
        }
    }
    None
}

/// The sagitta of the arc from `a` to `b` that leaves `a` heading along
/// `tangent`, signed for [`arc_from_bulge`] with the same `normal`. `None` if
/// the tangent is not in the arc's plane or points along the chord.
pub fn tangent_bulge(a: DVec3, b: DVec3, tangent: DVec3, normal: DVec3) -> Option<f64> {
    let chord = b - a;
    let c = chord.length();
    let n = normal.try_normalize()?;
    let t = tangent.try_normalize()?;
    if c < TOL_MERGE || t.dot(n).abs() > 1e-6 {
        return None;
    }
    let e = chord / c;
    let u = n.cross(e);
    let (tc, tu) = (t.dot(e), t.dot(u));
    if tu.abs() < 1e-9 {
        return None;
    }
    let phi = tu.abs().atan2(tc);
    Some((c / 2.0) * (phi / 2.0).tan() * tu.signum())
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith_geom::tolerance::coincident;
    use smith_model::Document;

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    fn flat(center: DVec3) -> Placement {
        Placement {
            center,
            x_axis: DVec3::X,
            normal: DVec3::Z,
        }
    }

    #[test]
    fn typed_rectangle_is_exactly_600_square_mm() {
        // Acceptance: `20,30` gives a face of exactly 600 mm² (exact f64).
        let mut m = Entities::new();
        let corners = rectangle_corners(DVec3::ZERO, DVec3::X, DVec3::Y, 20.0, 30.0);
        draw(&mut m, &DrawOp::Rectangle { corners }).unwrap();
        assert_eq!(m.face_count(), 1);
        let f = m.face_ids().next().unwrap();
        assert_eq!(m.face_area(f), Some(600.0));
    }

    #[test]
    fn small_circle_keeps_48_editable_segments() {
        // Acceptance: r 0.5 mm with `48s` is a 48-segment curve, still editable.
        let mut m = Entities::new();
        let op = DrawOp::Circle {
            at: flat(DVec3::ZERO),
            radius: 0.5,
            segments: 48,
        };
        let r = draw(&mut m, &op).unwrap();
        let id = r.curve.unwrap();
        assert_eq!(m.curve(id).unwrap().segments(), 48);
        assert_eq!(r.edges.len(), 48);
        assert_eq!(m.face_count(), 1);
        m.set_curve_segments(id, 24).unwrap();
        assert_eq!(m.curve(id).unwrap().edges().len(), 24);
        m.validate().unwrap();
    }

    #[test]
    fn closed_line_chain_makes_a_face() {
        // Acceptance: a Line chain that closes on its start auto-faces.
        let mut m = Entities::new();
        let pts = [v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(5.0, 8.0, 0.0)];
        for i in 0..3 {
            let op = DrawOp::Line {
                a: pts[i],
                b: pts[(i + 1) % 3],
            };
            draw(&mut m, &op).unwrap();
            assert_eq!(m.face_count(), usize::from(i == 2));
        }
    }

    #[test]
    fn six_sided_polygon_is_hexagonal() {
        // Acceptance: a polygon typed `6s` is hexagonal.
        let mut m = Entities::new();
        let op = DrawOp::Polygon {
            at: flat(DVec3::ZERO),
            radius: 10.0,
            sides: 6,
        };
        let r = draw(&mut m, &op).unwrap();
        assert_eq!(r.edges.len(), 6);
        assert_eq!(m.face_count(), 1);
        for e in &r.edges {
            assert!(
                (m.edge_length(*e).unwrap() - 10.0).abs() < 1e-12,
                "side = radius"
            );
        }
    }

    #[test]
    fn circle_through_three_points_passes_through_all_three() {
        // Acceptance: within TOL_MERGE of all three picked points.
        let (p1, p2, p3) = (v(1.3, 0.2, 0.0), v(7.9, 3.1, 0.5), v(-2.0, 6.4, 1.1));
        let (c, n, r) = circumcircle(p1, p2, p3).unwrap();
        for p in [p1, p2, p3] {
            assert!(((p - c).length() - r).abs() < TOL_MERGE);
            assert!((p - c).dot(n).abs() < 1e-9, "in the circle's plane");
        }
        assert!(circumcircle(p1, p1 * 2.0, p1 * 3.0).is_none(), "collinear");
    }

    #[test]
    fn arc_from_bulge_hits_both_ends_and_the_bulge() {
        let (a, b) = (v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
        for s in [2.0, -2.0, 5.0, 8.0] {
            let (at, r, sweep) = arc_from_bulge(a, b, s, DVec3::Z).unwrap();
            let arc = Arc::new(at.frame().unwrap(), r, 0.0, sweep).unwrap();
            assert!(coincident(arc.start_point(), a));
            assert!(coincident(arc.end_point(), b));
            let mid = arc.frame().point_at(r, sweep / 2.0);
            assert!((mid - v(5.0, s, 0.0)).length() < 1e-9, "bulge {s}");
        }
        assert!(arc_from_bulge(a, b, 0.0, DVec3::Z).is_none());
    }

    #[test]
    fn tangent_bulge_continues_the_incoming_direction() {
        // Leaving (0,0) along +x and ending at (5,5) is a quarter circle about
        // (0,5) with radius 5.
        let (a, b) = (v(0.0, 0.0, 0.0), v(5.0, 5.0, 0.0));
        let s = tangent_bulge(a, b, DVec3::X, DVec3::Z).unwrap();
        let (at, r, sweep) = arc_from_bulge(a, b, s, DVec3::Z).unwrap();
        assert!((r - 5.0).abs() < 1e-9);
        assert!((sweep - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        let arc = Arc::new(at.frame().unwrap(), r, 0.0, sweep).unwrap();
        let f = arc.frame();
        let start_dir = f.y_axis();
        assert!((start_dir - DVec3::X).length() < 1e-9, "leaves along +x");
    }

    #[test]
    fn each_draw_is_one_undo_step() {
        // Acceptance: each completed draw is a single undo step.
        let mut doc = Document::new();
        let empty = doc.clone();
        let op = DrawOp::Rectangle {
            corners: rectangle_corners(DVec3::ZERO, DVec3::X, DVec3::Y, 4.0, 3.0),
        };
        doc.transaction(op.name(), |d| draw(d.root_mut(), &op))
            .unwrap();
        let op2 = DrawOp::Line {
            a: v(0.0, 0.0, 0.0),
            b: v(4.0, 3.0, 0.0),
        };
        doc.transaction(op2.name(), |d| draw(d.root_mut(), &op2))
            .unwrap();
        assert_eq!(doc.root().face_count(), 2, "the diagonal splits the face");
        assert_eq!(doc.undo().as_deref(), Some("Line"));
        assert_eq!(doc.root().face_count(), 1);
        assert_eq!(doc.undo().as_deref(), Some("Rectangle"));
        assert!(doc.same_content(&empty));
    }

    #[test]
    fn degenerate_input_is_refused_cleanly() {
        let mut m = Entities::new();
        let bad = DrawOp::Circle {
            at: flat(DVec3::ZERO),
            radius: 0.0,
            segments: 24,
        };
        assert!(draw(&mut m, &bad).is_err());
        let bad = DrawOp::Polygon {
            at: flat(DVec3::ZERO),
            radius: 3.0,
            sides: 2,
        };
        assert!(draw(&mut m, &bad).is_err());
        assert_eq!(m.edge_count(), 0);
    }

    #[test]
    fn smooth_bezier_passes_through_its_points() {
        let pts = [
            v(0.0, 0.0, 0.0),
            v(4.0, 3.0, 0.0),
            v(8.0, 0.0, 0.0),
            v(12.0, 4.0, 0.0),
        ];
        let cps = smooth_through(&pts);
        assert_eq!(cps.len(), 10);
        let mut m = Entities::new();
        let op = DrawOp::Bezier {
            control_points: cps,
            segments: 8,
        };
        let r = draw(&mut m, &op).unwrap();
        assert_eq!(r.edges.len(), 24);
        for p in pts {
            assert!(m.find_vertex(p).is_some(), "passes through {p}");
        }
    }

    #[test]
    fn edit_curve_changes_shape_in_place_and_undoes() {
        let mut doc = Document::new();
        let op = DrawOp::Circle {
            at: flat(DVec3::ZERO),
            radius: 5.0,
            segments: 24,
        };
        let id = doc
            .transaction("Circle", |d| draw(d.root_mut(), &op))
            .unwrap()
            .curve
            .unwrap();
        let drawn = doc.clone();
        let bigger =
            Curve::Circle(Circle::new(flat(DVec3::ZERO).frame().unwrap(), 12.3456).unwrap());
        let edit = DrawOp::EditCurve {
            id,
            curve: bigger,
            segments: 48,
        };
        doc.transaction(edit.name(), |d| draw(d.root_mut(), &edit))
            .unwrap();
        let rec = doc.root().curve(id).unwrap();
        assert_eq!(rec.segments(), 48);
        assert!(matches!(rec.curve(), Curve::Circle(c) if c.radius() == 12.3456));
        assert_eq!(doc.root().face_count(), 1);
        doc.undo();
        assert!(doc.same_content(&drawn));
    }
}
