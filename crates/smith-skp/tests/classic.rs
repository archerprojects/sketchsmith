//! SketchSmith — SKP reader tests on real files | v1.0.0 | Thursday 12 November 2026
//!
//! The files in `fixtures/` come from OpenSKP's own test suite (MIT, see
//! `THIRD_PARTY_NOTICES.md`), and the counts checked here are the ones
//! OpenSKP's tests expect of them.

use std::path::Path;

use smith_skp::{Container, SkpError, SkpModel, container, read};

fn bytes(name: &str) -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(p).unwrap()
}

fn model(name: &str) -> SkpModel {
    read(&bytes(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn layer_names(m: &SkpModel) -> Vec<&str> {
    m.layers.iter().map(|l| l.name.as_str()).collect()
}

#[test]
fn a_2020_model_reads_whole() {
    let m = model("gondola_v20.skp");
    assert_eq!(m.version, "{20.1.235}");
    assert_eq!(m.definitions.len(), 20);
    assert_eq!(m.materials.len(), 24);
    assert_eq!(layer_names(&m), ["Layer0", "Gondulas Laterais"]);
    let sum = |f: fn(&smith_skp::Entities) -> usize| -> usize {
        m.definitions.iter().map(|d| f(&d.entities)).sum()
    };
    assert_eq!(sum(|e| e.faces.len()), 1887);
    assert_eq!(sum(|e| e.edges.len()), 9174);
    assert_eq!(sum(|e| e.vertices.len()), 6543);
    assert_eq!(m.root.instances.len(), 23);
    // Every placed definition carries geometry, itself or further down.
    fn carries(m: &SkpModel, id: usize, depth: usize) -> bool {
        depth < 64
            && m.definition(id).is_some_and(|d| {
                !d.entities.faces.is_empty()
                    || d.entities
                        .instances
                        .iter()
                        .any(|i| carries(m, i.definition, depth + 1))
            })
    }
    assert!(
        m.root
            .instances
            .iter()
            .all(|i| carries(&m, i.definition, 0))
    );
}

#[test]
fn a_2019_model_keeps_its_definitions_nesting_and_layers() {
    let m = model("legacy_v2019_synthetic.skp");
    assert_eq!(m.version, "{19.0.1}");
    let mut names: Vec<&str> = m.definitions.iter().map(|d| d.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["Frame", "InnerFrame", "Panel", "Post"]);
    for d in &m.definitions {
        let e = &d.entities;
        assert_eq!((e.faces.len(), e.edges.len(), e.vertices.len()), (1, 4, 4));
    }
    assert_eq!(m.root.instances.len(), 10);
    assert_eq!(m.materials.len(), 24);
    let layers = layer_names(&m);
    assert!(layers.contains(&"Layer0") && layers.contains(&"Roof"));
    let frame = m.definitions.iter().find(|d| d.name == "Frame").unwrap();
    let painted = frame.entities.faces[0].material.unwrap();
    let red = m.materials.iter().find(|x| x.id == painted).unwrap();
    assert_eq!(red.name, "Red");
    let panel = m.definitions.iter().find(|d| d.name == "Panel").unwrap();
    let inside: Vec<&str> = panel
        .entities
        .instances
        .iter()
        .map(|i| m.definition(i.definition).unwrap().name.as_str())
        .collect();
    assert_eq!(inside, ["InnerFrame"]);
}

#[test]
fn a_2014_model_reads_as_its_later_twin() {
    let m = model("legacy_v2014_synthetic.skp");
    assert_eq!(m.definitions.len(), 4);
    assert_eq!(m.layers.len(), 2);
    assert_eq!(m.root.instances.len(), 10);
}

#[test]
fn a_2020_model_without_definitions_keeps_its_root_and_texture() {
    let m = model("legacy_v2020_textured.skp");
    assert_eq!(m.root.faces.len(), 1);
    assert_eq!(m.root.edges.len(), 4);
    assert_eq!(m.root.vertices.len(), 4);
    assert!(m.materials.iter().any(|x| {
        x.texture
            .as_ref()
            .and_then(|t| t.data.as_ref())
            .is_some_and(|d| !d.is_empty())
    }));
}

#[test]
fn models_with_no_or_one_material_read() {
    let blank = model("blank_v17.skp");
    assert_eq!(blank.version, "{17.0.1}");
    assert!(blank.materials.is_empty());
    assert_eq!(layer_names(&blank), ["Layer0"]);
    assert!(blank.definitions.is_empty());
    assert!(blank.root.instances.is_empty());

    let one = model("single_material_v17.skp");
    assert_eq!(one.materials.len(), 1);
    assert_eq!(one.materials[0].name, "RedMat");
    assert_eq!(layer_names(&one), ["Layer0"]);

    let guides = model("zero_material_custom_layer_v18.skp");
    assert_eq!(guides.version, "{18.0.16975}");
    assert!(guides.materials.is_empty());
    assert!(guides.layers.len() >= 2);
    assert!(guides.root.faces.is_empty());
    assert_eq!(guides.root.guide_lines.len(), 2);
}

#[test]
fn a_2021_file_is_recognised_and_refused() {
    let data = bytes("instance_scale_term_2021.skp");
    assert_eq!(container(&data), Container::Vff);
    assert_eq!(read(&data), Err(SkpError::Newer));
}

#[test]
fn a_file_that_is_not_skp_is_refused() {
    assert_eq!(read(b"solid cube\nendsolid\n"), Err(SkpError::NotSkp));
    assert_eq!(read(&[]), Err(SkpError::NotSkp));
}

#[test]
fn cut_short_and_damaged_files_fail_with_a_message_never_a_panic() {
    for name in [
        "gondola_v20.skp",
        "legacy_v2019_synthetic.skp",
        "legacy_v2020_textured.skp",
        "zero_material_custom_layer_v18.skp",
    ] {
        let data = bytes(name);
        // Cut short at many lengths.
        let step = (data.len() / 97).max(1);
        for len in (0..data.len()).step_by(step) {
            let _ = read(&data[..len]);
        }
        // Bytes flipped through the body, a few at a time.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..60 {
            let mut d = data.clone();
            for _ in 0..8 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let at = 0x200 + (seed as usize) % (d.len() - 0x200);
                d[at] ^= (seed >> 32) as u8 | 1;
            }
            if let Err(e) = read(&d) {
                assert!(matches!(
                    e,
                    SkpError::Unreadable(_) | SkpError::NotSkp | SkpError::Newer
                ));
            }
        }
    }
}
