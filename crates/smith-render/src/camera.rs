//! SketchSmith — canvas camera | v1.0.0 | Thursday 12 November 2026
//!
//! An orbit camera in `f64`, Z-up. The camera looks at `target`
//! from `distance` away, along a direction set by `yaw` (about Z) and `pitch`
//! (elevation). Projection is perspective or parallel; parallel uses the same
//! visible height at the target as perspective, so toggling keeps the scale.
//!
//! **Zoom to cursor.** [`Camera::zoom_toward`] scales the camera about the
//! 3D point under the cursor: eye and target both move toward it by the same
//! factor. That point therefore stays exactly under the cursor, and each wheel
//! step is proportional to the distance to it — fine steps on a small part,
//! large steps on a whole model.
//!
//! **Precision.** The renderer subtracts a render origin from both the scene and
//! the eye in `f64` before converting to `f32` (see [`Camera::view_proj`]), so
//! sub-millimetre detail stays sharp far from the world origin. Depth uses a
//! reversed-Z infinite projection, which keeps depth precision for jewelry-scale
//! detail in metre-scale scenes.

use std::f64::consts::FRAC_PI_2;

use smith_geom::{DMat4, DVec2, DVec3};

/// Perspective or parallel projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Projection {
    /// Perspective, with the camera's vertical field of view.
    Perspective,
    /// Parallel (orthographic).
    Parallel,
}

/// The standard views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardView {
    /// Looking down −Z.
    Top,
    /// Looking up +Z.
    Bottom,
    /// Looking along +Y (from the front).
    Front,
    /// Looking along −Y.
    Back,
    /// Looking along +X (from the left).
    Left,
    /// Looking along −X (from the right).
    Right,
    /// Isometric-style view from front-left, above.
    Iso,
}

/// Closest a camera may get to its target, in mm.
const MIN_DISTANCE: f64 = 1e-3;
/// Furthest, in mm (10 km).
const MAX_DISTANCE: f64 = 1e7;

/// The canvas camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// The point orbited around and looked at.
    pub target: DVec3,
    /// Distance from eye to target, in mm.
    pub distance: f64,
    /// Rotation about +Z, radians. −π/2 views from −Y (the front).
    pub yaw: f64,
    /// Elevation above the XY plane, radians, in [−π/2, π/2].
    pub pitch: f64,
    /// Current projection.
    pub projection: Projection,
    /// Vertical field of view (perspective), radians.
    pub fov_y: f64,
}

impl Default for Camera {
    fn default() -> Self {
        let mut c = Self {
            target: DVec3::ZERO,
            distance: 300.0,
            yaw: 0.0,
            pitch: 0.0,
            projection: Projection::Perspective,
            fov_y: 35f64.to_radians(),
        };
        // Opens looking straight down on the red–green plane (agreed default).
        c.set_view(StandardView::Top);
        c
    }
}

impl Camera {
    /// Unit vector from the target toward the eye.
    fn back(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        DVec3::new(cp * cy, cp * sy, sp)
    }

    /// Eye position.
    pub fn eye(&self) -> DVec3 {
        self.target + self.back() * self.distance
    }

    /// Unit viewing direction (eye → target).
    pub fn forward(&self) -> DVec3 {
        -self.back()
    }

    /// Unit screen-right direction. Derived from yaw alone, so it is defined
    /// even looking straight down.
    pub fn right(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        DVec3::new(-sy, cy, 0.0)
    }

    /// Unit screen-up direction.
    pub fn up(&self) -> DVec3 {
        self.right().cross(self.forward())
    }

    /// Half the visible height at the target distance, in mm.
    pub fn half_height(&self) -> f64 {
        self.distance * (self.fov_y * 0.5).tan()
    }

    /// World size of one pixel at the target, for a canvas `height_px` tall.
    pub fn world_per_pixel(&self, height_px: f64) -> f64 {
        2.0 * self.half_height() / height_px.max(1.0)
    }

    /// The combined view-projection matrix, relative to `origin` (the renderer's
    /// render origin), with reversed-Z depth (1 = near, 0 = infinitely far).
    pub fn view_proj(&self, aspect: f64, origin: DVec3) -> DMat4 {
        let eye = self.eye() - origin;
        let target = self.target - origin;
        let view = look_at_rh(eye, target, self.up());
        let near = (self.distance * 1e-3).max(1e-5);
        let proj = match self.projection {
            Projection::Perspective => perspective_infinite_reverse_rh(self.fov_y, aspect, near),
            Projection::Parallel => {
                let h = self.half_height();
                let w = h * aspect;
                // Reversed depth: far plane passed as "near" and vice versa.
                let depth = self.distance * 1e4 + 1e4;
                orthographic_rh(-w, w, -h, h, depth, -depth)
            }
        };
        proj * view
    }

    /// The world-space ray through a point in normalised device coordinates
    /// (x, y in [−1, 1], y up). Returns `(origin, unit direction)`.
    pub fn ray(&self, ndc: DVec2, aspect: f64) -> (DVec3, DVec3) {
        let h = self.half_height();
        match self.projection {
            Projection::Perspective => {
                let t = (self.fov_y * 0.5).tan();
                let dir =
                    self.forward() + self.right() * (ndc.x * t * aspect) + self.up() * (ndc.y * t);
                (self.eye(), dir.normalize())
            }
            Projection::Parallel => {
                let origin =
                    self.eye() + self.right() * (ndc.x * h * aspect) + self.up() * (ndc.y * h);
                (origin, self.forward())
            }
        }
    }

    /// Projects a world point to NDC. `None` if it is behind a perspective eye.
    pub fn project(&self, p: DVec3, aspect: f64) -> Option<DVec2> {
        let clip = self.view_proj(aspect, DVec3::ZERO) * p.extend(1.0);
        (clip.w > 0.0).then(|| DVec2::new(clip.x / clip.w, clip.y / clip.w))
    }

    /// The point the cursor ray meets the plane through the target facing the
    /// camera. Used when nothing is under the cursor.
    pub fn point_on_target_plane(&self, ndc: DVec2, aspect: f64) -> DVec3 {
        let (o, d) = self.ray(ndc, aspect);
        let n = self.forward();
        let denom = d.dot(n);
        if denom.abs() < 1e-12 {
            return self.target;
        }
        o + d * ((self.target - o).dot(n) / denom)
    }

    /// Orbits by a mouse drag of `dx`, `dy` pixels (SketchUp-like sensitivity).
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        const RAD_PER_PX: f64 = 0.008;
        self.yaw -= dx * RAD_PER_PX;
        self.pitch = (self.pitch + dy * RAD_PER_PX).clamp(-FRAC_PI_2, FRAC_PI_2);
    }

    /// Pans so the scene follows a mouse drag of `dx`, `dy` pixels in a canvas
    /// `height_px` tall.
    pub fn pan(&mut self, dx: f64, dy: f64, height_px: f64) {
        let wpp = self.world_per_pixel(height_px);
        self.target += -self.right() * dx * wpp + self.up() * dy * wpp;
    }

    /// Zooms by `factor` (below 1 = closer) about `point`, keeping `point` fixed
    /// on screen. See the module notes.
    pub fn zoom_toward(&mut self, point: DVec3, factor: f64) {
        let new_distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
        let f = new_distance / self.distance;
        self.target = point + (self.target - point) * f;
        self.distance = new_distance;
    }

    /// Switches to a standard view, keeping target and distance.
    pub fn set_view(&mut self, view: StandardView) {
        use std::f64::consts::PI;
        let (yaw, pitch) = match view {
            StandardView::Top => (-FRAC_PI_2, FRAC_PI_2),
            StandardView::Bottom => (-FRAC_PI_2, -FRAC_PI_2),
            StandardView::Front => (-FRAC_PI_2, 0.0),
            StandardView::Back => (FRAC_PI_2, 0.0),
            StandardView::Left => (PI, 0.0),
            StandardView::Right => (0.0, 0.0),
            StandardView::Iso => (-3.0 * PI / 4.0, 35.264f64.to_radians()),
        };
        self.yaw = yaw;
        self.pitch = pitch;
    }

    /// Points the camera so it looks *against* `back` (the eye sits on the `back`
    /// side of the target), keeping target and distance. Used by Align View.
    pub fn look_from(&mut self, back: DVec3) {
        let Some(b) = back.try_normalize() else {
            return;
        };
        self.pitch = b.z.clamp(-1.0, 1.0).asin();
        // Straight up or down: keep the current yaw so "up" on screen stays stable.
        if b.x.abs() > 1e-12 || b.y.abs() > 1e-12 {
            self.yaw = b.y.atan2(b.x);
        }
    }

    /// Toggles perspective and parallel projection.
    pub fn toggle_projection(&mut self) {
        self.projection = match self.projection {
            Projection::Perspective => Projection::Parallel,
            Projection::Parallel => Projection::Perspective,
        };
    }

    /// Frames axis-aligned bounds `(min, max)` with a small margin.
    pub fn zoom_extents(&mut self, (min, max): (DVec3, DVec3), aspect: f64) {
        let center = (min + max) * 0.5;
        let radius = ((max - min).length() * 0.5).max(1.0);
        let half_fov =
            (self.fov_y * 0.5).min((self.fov_y * 0.5).tan().atan2(1.0 / aspect.max(1e-3)));
        self.target = center;
        self.distance = (radius / half_fov.sin() * 1.1).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }
}

// Projection helpers. glam 0.34 no longer ships projection constructors, so the
// standard right-handed, depth-0-to-1 (wgpu) forms are written out here.

/// Right-handed view matrix looking from `eye` toward `center`.
fn look_at_rh(eye: DVec3, center: DVec3, up: DVec3) -> DMat4 {
    let f = (center - eye).normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    DMat4::from_cols_array(&[
        s.x,
        u.x,
        -f.x,
        0.0, //
        s.y,
        u.y,
        -f.y,
        0.0, //
        s.z,
        u.z,
        -f.z,
        0.0, //
        -s.dot(eye),
        -u.dot(eye),
        f.dot(eye),
        1.0,
    ])
}

/// Right-handed infinite perspective with reversed depth: the near plane maps
/// to depth 1 and infinity to 0.
fn perspective_infinite_reverse_rh(fov_y: f64, aspect: f64, near: f64) -> DMat4 {
    let f = 1.0 / (0.5 * fov_y).tan();
    DMat4::from_cols_array(&[
        f / aspect,
        0.0,
        0.0,
        0.0, //
        0.0,
        f,
        0.0,
        0.0, //
        0.0,
        0.0,
        0.0,
        -1.0, //
        0.0,
        0.0,
        near,
        0.0,
    ])
}

/// Right-handed orthographic projection to depth 0..1. Passing `near > far`
/// (both signed view distances) gives reversed depth.
fn orthographic_rh(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64) -> DMat4 {
    let rw = 1.0 / (right - left);
    let rh = 1.0 / (top - bottom);
    let r = 1.0 / (near - far);
    DMat4::from_cols_array(&[
        2.0 * rw,
        0.0,
        0.0,
        0.0, //
        0.0,
        2.0 * rh,
        0.0,
        0.0, //
        0.0,
        0.0,
        r,
        0.0, //
        -(left + right) * rw,
        -(top + bottom) * rh,
        r * near,
        1.0,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: DVec3, b: DVec3) -> bool {
        (a - b).length() < 1e-9
    }

    #[test]
    fn standard_views_look_the_right_way() {
        let mut c = Camera::default();
        c.set_view(StandardView::Top);
        assert!(near(c.forward(), -DVec3::Z));
        assert!(near(c.up(), DVec3::Y));
        assert!(near(c.right(), DVec3::X));
        c.set_view(StandardView::Front);
        assert!(near(c.forward(), DVec3::Y));
        assert!(near(c.up(), DVec3::Z));
        c.set_view(StandardView::Right);
        assert!(near(c.forward(), -DVec3::X));
        c.set_view(StandardView::Bottom);
        assert!(near(c.forward(), DVec3::Z));
    }

    #[test]
    fn zoom_toward_keeps_the_point_under_the_cursor() {
        for projection in [Projection::Perspective, Projection::Parallel] {
            let mut c = Camera {
                projection,
                ..Camera::default()
            };
            let aspect = 1.6;
            let point = DVec3::new(37.0, -12.0, 4.0);
            let before = c.project(point, aspect).unwrap();
            c.zoom_toward(point, 0.5);
            let after = c.project(point, aspect).unwrap();
            assert!((before - after).length() < 1e-9, "{projection:?}");
        }
    }

    #[test]
    fn zoom_steps_scale_with_distance_to_the_point() {
        let mut c = Camera::default();
        let near_point = c.target + c.back() * (c.distance * 0.9);
        let before = c.eye();
        c.zoom_toward(near_point, 0.8);
        // The eye moved 20 % of the way to the point, not 20 % of the full distance.
        let moved = (c.eye() - before).length();
        assert!((moved - 0.2 * near_point.distance(before)).abs() < 1e-9);
    }

    #[test]
    fn ray_through_center_hits_target_and_matches_projection() {
        for projection in [Projection::Perspective, Projection::Parallel] {
            let c = Camera {
                projection,
                ..Camera::default()
            };
            let (o, d) = c.ray(DVec2::ZERO, 1.5);
            let to_target = c.target - o;
            assert!(to_target.cross(d).length() < 1e-9 * to_target.length());
            // A point along an off-centre ray projects back to the same NDC.
            let ndc = DVec2::new(0.3, -0.6);
            let (o, d) = c.ray(ndc, 1.5);
            let p = o + d * 50.0;
            assert!((c.project(p, 1.5).unwrap() - ndc).length() < 1e-9);
        }
    }

    #[test]
    fn pan_moves_scene_with_the_cursor() {
        let mut c = Camera::default();
        let aspect = 1.0;
        let height = 800.0;
        let p = c.target;
        let before = c.project(p, aspect).unwrap();
        c.pan(100.0, 0.0, height);
        let after = c.project(p, aspect).unwrap();
        // 100 px right in an 800 px tall square canvas = +0.25 NDC.
        assert!((after.x - before.x - 0.25).abs() < 1e-9);
    }

    #[test]
    fn look_from_points_the_camera() {
        let mut c = Camera::default();
        let n = DVec3::new(1.0, -2.0, 0.5).normalize();
        c.look_from(n);
        assert!(near(c.forward(), -n));
        c.look_from(DVec3::Z);
        assert!(near(c.forward(), -DVec3::Z));
    }

    #[test]
    fn zoom_extents_frames_bounds() {
        let mut c = Camera::default();
        c.zoom_extents((DVec3::splat(-5.0), DVec3::splat(5.0)), 1.0);
        assert!(near(c.target, DVec3::ZERO));
        for corner in [DVec3::splat(5.0), DVec3::splat(-5.0)] {
            let ndc = c.project(corner, 1.0).unwrap();
            assert!(ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0);
        }
    }

    #[test]
    fn reversed_depth_nearer_is_larger_in_both_projections() {
        for projection in [Projection::Perspective, Projection::Parallel] {
            let c = Camera {
                projection,
                ..Camera::default()
            };
            let m = c.view_proj(1.0, DVec3::ZERO);
            let depth = |d: f64| {
                let clip = m * (c.eye() + c.forward() * d).extend(1.0);
                clip.z / clip.w
            };
            let (a, b) = (depth(c.distance * 0.5), depth(c.distance * 2.0));
            assert!(a > b, "{projection:?}: {a} vs {b}");
            assert!(
                (0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b),
                "{projection:?}"
            );
        }
    }
}
