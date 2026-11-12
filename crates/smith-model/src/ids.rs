//! SketchSmith — entity keys | v1.0.0 | Thursday 12 November 2026
//!
//! Every entity is identified by a typed key wrapping a `u64`. Keys are issued
//! from a per-arena counter that only ever increases, so a key from an erased
//! entity is never reused for a new one: stale references fail safely instead
//! of pointing at the wrong geometry. Undo can restore an erased entity under
//! its original key (see `arena.rs`), which is why keys are not slot indices.

use std::fmt;

/// Behaviour shared by all entity keys.
pub(crate) trait Key: Copy + Ord + Eq + std::hash::Hash + fmt::Debug {
    /// Wraps a raw counter value.
    fn from_raw(raw: u64) -> Self;
}

macro_rules! key {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl Key for $name {
            fn from_raw(raw: u64) -> Self {
                Self(raw)
            }
        }

        impl $name {
            /// The raw key value (stable for the life of the document).
            pub fn raw(self) -> u64 {
                self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

key!(
    /// Key of a [`crate::Vertex`].
    VertexId
);
key!(
    /// Key of an [`crate::Edge`].
    EdgeId
);
key!(
    /// Key of a [`crate::Face`].
    FaceId
);
key!(
    /// Key of a [`crate::CurveRecord`].
    CurveId
);
key!(
    /// Key of an [`crate::Instance`] (a placed group or component).
    InstanceId
);
key!(
    /// Key of a [`crate::Definition`] (the geometry a group or component holds).
    DefinitionId
);

key!(
    /// Key of a [`crate::Layer`].
    LayerId
);
key!(
    /// Key of a [`crate::Guide`].
    GuideId
);
key!(
    /// Key of an [`crate::Annotation`].
    AnnotationId
);
key!(
    /// Key of a [`crate::Scene`].
    SceneId
);
key!(
    /// Key of a [`crate::SectionPlane`].
    SectionId
);
key!(
    /// Key of a [`crate::Material`].
    MaterialId
);
key!(
    /// Key of a [`crate::Wrap`].
    WrapId
);
