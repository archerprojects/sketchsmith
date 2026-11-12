//! SketchSmith — file errors | v1.0.0 | Thursday 12 November 2026
//!
//! Errors from reading and writing files. Messages are written to be shown to
//! the user as they are.

use smith_model::ModelError;

/// A file could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    /// The operating system refused (missing file, no permission, disk full…).
    #[error("{0}")]
    Os(#[from] std::io::Error),
    /// The file is not a binary glTF (`.glb`) file.
    #[error("this is not a .glb file")]
    NotGlb,
    /// A `.glb` file made by another application (no SketchSmith model data).
    #[error(
        "this .glb file was made by another application; importing other \
         applications' files arrives with File → Import"
    )]
    Foreign,
    /// The file is a SketchSmith file but its contents are inconsistent.
    #[error("the file is damaged: {0}")]
    Damaged(String),
    /// The model is too large for the format (over 4 GiB).
    #[error("the model is too large to save as .glb (over 4 GiB)")]
    TooLarge,
    /// An image that cannot be used as a texture (the message says why).
    #[error("{0}")]
    Image(String),
    /// A file opened as SKP is not one.
    #[error("Not an SKP File")]
    NotSkp,
    /// An SKP file from SketchUp 2021 or later.
    #[error("SKP 2021 or Later Not Read Yet")]
    SkpNewer,
    /// A 2013–2020 SKP file the reader could not get through.
    #[error("Couldn't Read This SKP File")]
    SkpUnreadable,
}

impl From<ModelError> for IoError {
    fn from(e: ModelError) -> Self {
        IoError::Damaged(e.to_string())
    }
}

impl From<smith_geom::GeomError> for IoError {
    fn from(e: smith_geom::GeomError) -> Self {
        IoError::Damaged(format!("invalid curve ({e})"))
    }
}

impl From<serde_json::Error> for IoError {
    fn from(e: serde_json::Error) -> Self {
        IoError::Damaged(format!("unreadable JSON ({e})"))
    }
}
