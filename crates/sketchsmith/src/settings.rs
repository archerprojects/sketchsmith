//! SketchSmith — user settings | v1.0.0 | Thursday 12 November 2026
//!
//! Persistent user settings, stored as `settings.toml` in the config folder.
//! A missing or unreadable file falls back to defaults; saving never panics.
//! A setting missing from an older file takes its default (autosave every
//! 5 minutes, added in v0.1.20).

use std::path::Path;

use serde::{Deserialize, Serialize};

use smith_infer::infer::{OnlyKind, SnapFilter};
use smith_infer::units::UnitSettings;
use smith_io::export::{ExportOptions, Format};
use smith_io::import::{ImportFormat, ImportOptions};

use crate::keymap::Preset;
use crate::panel_layout::PanelLayout;

/// File name inside the config folder.
const FILE: &str = "settings.toml";

/// Which side of the window the Operations Bar (its tabs, with
/// Operation Inputs below their panels) sits on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelSide {
    /// Right side (default).
    #[default]
    Right,
    /// Left side.
    Left,
}

/// Default autosave interval in minutes.
pub const DEFAULT_AUTOSAVE_MINUTES: u32 = 5;

fn default_autosave() -> u32 {
    DEFAULT_AUTOSAVE_MINUTES
}

/// Default number of backups kept beside a saved file, as in Blender.
pub const DEFAULT_SAVE_VERSIONS: u32 = 1;

fn default_save_versions() -> u32 {
    DEFAULT_SAVE_VERSIONS
}

fn default_proportional_radius() -> f64 {
    10.0
}

/// The export dialog's last choices, one set per format.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportMemory {
    /// OBJ.
    pub obj: ExportOptions,
    /// STL.
    pub stl: ExportOptions,
    /// glTF.
    pub gltf: ExportOptions,
    /// 3MF.
    pub three_mf: ExportOptions,
    /// STEP.
    pub step: ExportOptions,
    /// DXF.
    pub dxf: ExportOptions,
    /// PDF.
    pub pdf: ExportOptions,
    /// SVG.
    pub svg: ExportOptions,
    /// PNG.
    pub png: ExportOptions,
    /// Export Render (its format and that format's choices).
    pub render: ExportOptions,
}

impl Default for ExportMemory {
    fn default() -> Self {
        Self {
            obj: ExportOptions::for_format(Format::Obj),
            stl: ExportOptions::for_format(Format::Stl),
            gltf: ExportOptions::for_format(Format::Gltf),
            three_mf: ExportOptions::for_format(Format::ThreeMf),
            step: ExportOptions::for_format(Format::Step),
            dxf: ExportOptions::for_format(Format::Dxf),
            pdf: ExportOptions::for_format(Format::Pdf),
            svg: ExportOptions::for_format(Format::Svg),
            png: ExportOptions::for_format(Format::Png),
            render: ExportOptions::for_format(Format::Png),
        }
    }
}

impl ExportMemory {
    /// The choices for `format`.
    pub fn get(&self, format: Format) -> ExportOptions {
        match format {
            Format::Obj => self.obj,
            Format::Stl => self.stl,
            Format::Gltf => self.gltf,
            Format::ThreeMf => self.three_mf,
            Format::Step => self.step,
            Format::Dxf => self.dxf,
            Format::Pdf => self.pdf,
            Format::Svg => self.svg,
            Format::Png => self.png,
        }
    }

    /// Remembers the choices for `format`.
    pub fn set(&mut self, format: Format, options: ExportOptions) {
        match format {
            Format::Obj => self.obj = options,
            Format::Stl => self.stl = options,
            Format::Gltf => self.gltf = options,
            Format::ThreeMf => self.three_mf = options,
            Format::Step => self.step = options,
            Format::Dxf => self.dxf = options,
            Format::Pdf => self.pdf = options,
            Format::Svg => self.svg = options,
            Format::Png => self.png = options,
        }
    }
}

/// The import window's last choices, one set per format.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportMemory {
    /// OBJ.
    pub obj: ImportOptions,
    /// STL.
    pub stl: ImportOptions,
    /// DAE.
    pub dae: ImportOptions,
    /// SVG.
    pub svg: ImportOptions,
    /// glTF.
    pub gltf: ImportOptions,
    /// 3MF.
    pub threemf: ImportOptions,
    /// SKP.
    pub skp: ImportOptions,
}

impl Default for ImportMemory {
    fn default() -> Self {
        Self {
            obj: ImportOptions::for_format(ImportFormat::Obj),
            stl: ImportOptions::for_format(ImportFormat::Stl),
            dae: ImportOptions::for_format(ImportFormat::Dae),
            svg: ImportOptions::for_format(ImportFormat::Svg),
            gltf: ImportOptions::for_format(ImportFormat::Gltf),
            threemf: ImportOptions::for_format(ImportFormat::ThreeMf),
            skp: ImportOptions::for_format(ImportFormat::Skp),
        }
    }
}

impl ImportMemory {
    /// The choices for `format`.
    pub fn get(&self, format: ImportFormat) -> ImportOptions {
        match format {
            ImportFormat::Obj => self.obj,
            ImportFormat::Stl => self.stl,
            ImportFormat::Dae => self.dae,
            ImportFormat::Svg => self.svg,
            ImportFormat::Gltf => self.gltf,
            ImportFormat::ThreeMf => self.threemf,
            ImportFormat::Skp => self.skp,
        }
    }

    /// Remembers the choices for `format`.
    pub fn set(&mut self, format: ImportFormat, options: ImportOptions) {
        match format {
            ImportFormat::Obj => self.obj = options,
            ImportFormat::Stl => self.stl = options,
            ImportFormat::Dae => self.dae = options,
            ImportFormat::Svg => self.svg = options,
            ImportFormat::Gltf => self.gltf = options,
            ImportFormat::ThreeMf => self.threemf = options,
            ImportFormat::Skp => self.skp = options,
        }
    }
}

/// User settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Active keymap preset.
    #[serde(default)]
    pub keymap: Preset,
    /// Operations Bar Side (read from `dock_side` in files before 0.1.23).
    #[serde(default, alias = "dock_side")]
    pub panel_side: PanelSide,
    /// Display units and precision.
    #[serde(default)]
    pub units: UnitSettings,
    /// Enabled snap types.
    #[serde(default)]
    pub snaps: SnapFilter,
    /// Type the snap-only key restricts to.
    #[serde(default)]
    pub snap_only: OnlyKind,
    /// The Edit-mode panel arrangement, once customised.
    #[serde(default, alias = "dock_edit")]
    pub panel_edit: Option<PanelLayout>,
    /// The Present-mode panel arrangement, once customised.
    #[serde(default, alias = "dock_present")]
    pub panel_present: Option<PanelLayout>,
    /// Minutes between autosaves of unsaved work; 0 turns autosave off.
    #[serde(default = "default_autosave")]
    pub autosave_minutes: u32,
    /// Backups kept beside a file when saving over it (`.glb1`, `.glb2` …);
    /// 0 keeps none.
    #[serde(default = "default_save_versions")]
    pub save_versions: u32,
    /// Proportional editing on.
    #[serde(default)]
    pub proportional: bool,
    /// Proportional editing reach, mm.
    #[serde(default = "default_proportional_radius")]
    pub proportional_radius: f64,
    /// Proportional falloff: 0 smooth, 1 linear, 2 sharp.
    #[serde(default)]
    pub proportional_falloff: u8,
    /// The Select tool's mode in Edit mode: 0 Vertices, 1 Edges, 2 Faces.
    #[serde(default = "edges_mode")]
    pub select_mode: u8,
    /// The Statistics overlay in the canvas's corner.
    #[serde(default = "yes")]
    pub show_statistics: bool,
    /// Guides are shown (and snapped to).
    #[serde(default = "yes")]
    pub show_guides: bool,
    /// Section planes are drawn.
    #[serde(default = "yes")]
    pub show_section_planes: bool,
    /// The active section plane cuts the view.
    #[serde(default = "yes")]
    pub section_cuts: bool,
    /// The sidebar (N) is open.
    #[serde(default = "yes")]
    pub sidebar_open: bool,
    /// The open tab in Edit mode: 0 Geometry, 1 Items, 2 Layers.
    #[serde(default)]
    pub sidebar_tab: u8,
    /// The open tab in Present mode: 0 Styles, 1 Render.
    #[serde(default)]
    pub present_tab: u8,
    /// The export dialog's last choices.
    #[serde(default)]
    pub exports: ExportMemory,
    /// The import window's last choices.
    #[serde(default)]
    pub imports: ImportMemory,
}

fn yes() -> bool {
    true
}

/// The Select tool's mode until one is chosen: Edges.
fn edges_mode() -> u8 {
    1
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            keymap: Preset::default(),
            panel_side: PanelSide::default(),
            units: UnitSettings::default(),
            snaps: SnapFilter::default(),
            snap_only: OnlyKind::default(),
            panel_edit: None,
            panel_present: None,
            autosave_minutes: DEFAULT_AUTOSAVE_MINUTES,
            save_versions: DEFAULT_SAVE_VERSIONS,
            proportional: false,
            proportional_radius: default_proportional_radius(),
            proportional_falloff: 0,
            select_mode: edges_mode(),
            show_statistics: true,
            show_guides: true,
            show_section_planes: true,
            section_cuts: true,
            sidebar_open: true,
            sidebar_tab: 0,
            present_tab: 0,
            exports: ExportMemory::default(),
            imports: ImportMemory::default(),
        }
    }
}

impl Settings {
    /// Loads settings from `config_dir`, or defaults if absent or invalid.
    pub fn load(config_dir: &Path) -> Self {
        std::fs::read_to_string(config_dir.join(FILE))
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Saves settings to `config_dir`. Failures are logged, not fatal.
    pub fn save(&self, config_dir: &Path) {
        let result = toml::to_string(self)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                std::fs::write(config_dir.join(FILE), text).map_err(|e| e.to_string())
            });
        if let Err(e) = result {
            log::warn!("could not save settings: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let dir = std::env::temp_dir().join(format!("sketchsmith-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(Settings::load(&dir), Settings::default());
        let s = Settings {
            keymap: Preset::Blender,
            panel_side: PanelSide::Left,
            units: UnitSettings {
                unit: smith_infer::units::LengthUnit::FractionalInch,
                ..UnitSettings::default()
            },
            snaps: SnapFilter {
                quadrant: false,
                ..SnapFilter::default()
            },
            snap_only: OnlyKind::Endpoint,
            panel_edit: Some(PanelLayout::from_defaults(&[("Draw", &["Line", "Arc"])])),
            panel_present: None,
            autosave_minutes: 2,
            save_versions: 3,
            proportional: true,
            proportional_radius: 2.5,
            proportional_falloff: 2,
            select_mode: 0,
            show_statistics: false,
            show_guides: false,
            show_section_planes: false,
            section_cuts: false,
            sidebar_open: false,
            sidebar_tab: 2,
            present_tab: 1,
            exports: ExportMemory {
                obj: ExportOptions {
                    units: smith_io::export::UnitPreset::Custom,
                    unit: smith_io::export::FileUnit::In,
                    scale: 100.0,
                    selection_only: true,
                    ..ExportOptions::default()
                },
                pdf: ExportOptions {
                    paper: smith_io::export::Paper::A3,
                    orientation: smith_io::export::Orientation::Portrait,
                    hidden_lines: smith_io::export::HiddenLines::Dashed,
                    scale: 0.5,
                    ..ExportOptions::default()
                },
                ..ExportMemory::default()
            },
            imports: ImportMemory {
                obj: ImportOptions {
                    units: smith_io::export::UnitPreset::Print,
                    separate_shells: true,
                    ..ImportOptions::default()
                },
                ..ImportMemory::default()
            },
        };
        s.save(&dir);
        assert_eq!(Settings::load(&dir), s);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Settings files written before 0.1.23 used `dock_*` keys; they still load.
    #[test]
    fn reads_the_old_dock_keys() {
        let text = "dock_side = \"left\"\n";
        let s: Settings = toml::from_str(text).unwrap();
        assert_eq!(s.panel_side, PanelSide::Left);
        let layout = PanelLayout::from_defaults(&[("Draw", &["Line"])]);
        let new = toml::to_string(&Settings {
            panel_edit: Some(layout.clone()),
            ..Settings::default()
        })
        .unwrap();
        let old = new.replace("panel_edit", "dock_edit");
        let back: Settings = toml::from_str(&old).unwrap();
        assert_eq!(back.panel_edit, Some(layout));
    }
}
