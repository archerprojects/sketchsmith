//! SketchSmith — model errors | v1.0.0 | Thursday 12 November 2026

use smith_geom::GeomError;
use thiserror::Error;

/// Errors from model operations. Model code never panics on user data.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ModelError {
    /// An input coordinate was NaN or infinite.
    #[error("non-finite coordinate")]
    NonFinite,
    /// Both ends of a line merged to the same vertex (shorter than `TOL_MERGE`).
    #[error("line is shorter than the merge tolerance")]
    ZeroLength,
    /// A key referred to an entity that does not exist (already erased).
    #[error("entity does not exist")]
    NoSuchEntity,
    /// The geometry layer rejected the input (invalid curve, segment count, …).
    #[error(transparent)]
    Geometry(#[from] GeomError),
    /// The curve bounds out-of-plane faces, so its segment count cannot change.
    #[error("the curve has been extruded; its segment count is locked")]
    CurveLocked,
    /// A transaction is already open.
    #[error("a transaction is already open")]
    TransactionOpen,
    /// No transaction is open.
    #[error("no transaction is open")]
    NoTransaction,
    /// The placement would make a definition contain itself.
    #[error("a group or component cannot contain itself")]
    RecursiveInstance,
    /// A group or component involved is locked.
    #[error("it is locked")]
    Locked,
    /// The change would bend these faces out of flat.
    #[error("the change would bend {} face(s) out of flat", .0.len())]
    NotFlat(Vec<crate::ids::FaceId>),
    /// An offset or division does not fit the shape.
    #[error("the {0} does not fit the shape")]
    DoesNotFit(&'static str),
    /// A moved vertex would land on one that stays.
    #[error("a moved point would land on another point")]
    Collides,
    /// The operation does not apply here; the text says where and why, as
    /// shown on screen (a corner it names, say).
    #[error("{0}")]
    Refused(String),
    /// Stored model data (a file being read) is inconsistent.
    #[error("damaged model data: {0}")]
    InvalidData(String),
}
