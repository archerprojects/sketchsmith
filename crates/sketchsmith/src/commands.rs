//! SketchSmith — commands and where they appear | v1.0.0 | Thursday 12 November 2026
//!
//! Each command that acts on the selection or the model is defined once
//! here, as a [`Command`]. What it does, whether it applies to the current
//! selection, and its label live in the canvas (`Canvas::run_command`,
//! `Canvas::command_applies`, `Canvas::command_label`).
//!
//! Where a command appears is only a placement list: [`CONTEXT_MENU`] (the
//! right-click menu, with something selected), [`CONTEXT_MENU_EMPTY`]
//! (right-click with nothing selected), and the Items panel's
//! [`ITEMS_PANEL`]. Moving a command from one place to another is a change to these
//! lists only.

/// A command on the selection or the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Erase the selection.
    Erase,
    /// Hide the selection.
    Hide,
    /// Show the selection again (selected from the Items panel).
    Unhide,
    /// Hide everything but the selection.
    HideOthers,
    /// Show what was hidden last.
    UnhideLast,
    /// Show everything hidden.
    UnhideAll,
    /// Open the selected group or component for editing.
    EditGroup,
    /// Edit Shape: change a shape group's sizes.
    EditShape,
    /// Edit Extrude, Edit Loft or Edit Revolve (named for the selected
    /// group): change what it was made from.
    EditSweep,
    /// Reopen the selected curve's values.
    EditCurve,
    /// Add everything connected to the selection.
    SelectConnected,
    /// Add the edges bounding the selected faces.
    SelectBoundingEdges,
    /// Add the faces bounded by the selected edges.
    SelectConnectedFaces,
    /// Select everything.
    SelectAll,
    /// Put the Pivot where the menu was opened.
    SetPivotHere,
    /// Put the Pivot at the drawing axes' origin.
    PivotToOrigin,
    /// Tie the Pivot to the selected item, so it follows it.
    PivotToSelection,
    /// Give the selected points one shared red, green or blue value.
    AlignTo(u8),
    /// Flip the selection along red, green or blue.
    FlipAlong(u8),
    /// Swap the front and back of the selected faces.
    ReverseFaces,
    /// Turn connected faces the way of the selected one.
    OrientFaces,
    /// Divide the selected edge.
    Divide,
    /// Look straight at the selected face.
    AlignView,
    /// Frame the selection.
    ZoomSelection,
    /// Frame the whole model.
    ZoomExtents,
    /// Paste the clipboard.
    Paste,
    /// Make a group of the selection.
    MakeGroup,
    /// Make a component of the selection.
    MakeComponent,
    /// Put a group's or component's geometry back into the model.
    Explode,
    /// Give a group or component its own copy of its definition.
    MakeUnique,
    /// Lock or unlock a group or component.
    Lock,
    /// Remove scaling and mirroring from a group or component.
    ResetScale,
    /// Move a group's or component's origin and axes to the drawing axes.
    ChangeAxes,
}

/// One line of a menu or sidebar section.
#[derive(Debug, Clone, Copy)]
pub enum Entry {
    /// A command, shown when it applies.
    Item(Command),
    /// A submenu of commands, shown when any of them applies.
    Sub(&'static str, &'static [Command]),
    /// A separator (dropped where it would be first, last or doubled).
    Separator,
}

/// The right-click menu with something selected: the operations done over
/// the geometry.
pub const CONTEXT_MENU: &[Entry] = &[
    Entry::Item(Command::Erase),
    Entry::Separator,
    Entry::Item(Command::EditGroup),
    Entry::Item(Command::EditShape),
    Entry::Item(Command::EditSweep),
    Entry::Item(Command::EditCurve),
    Entry::Separator,
    Entry::Sub(
        "Select",
        &[
            Command::SelectConnected,
            Command::SelectBoundingEdges,
            Command::SelectConnectedFaces,
        ],
    ),
    Entry::Sub(
        "Transform",
        &[
            Command::SetPivotHere,
            Command::PivotToOrigin,
            Command::PivotToSelection,
            Command::AlignTo(0),
            Command::AlignTo(1),
            Command::AlignTo(2),
            Command::FlipAlong(0),
            Command::FlipAlong(1),
            Command::FlipAlong(2),
        ],
    ),
    Entry::Separator,
    Entry::Item(Command::ReverseFaces),
    Entry::Item(Command::OrientFaces),
    Entry::Item(Command::Divide),
    Entry::Separator,
    Entry::Item(Command::AlignView),
    Entry::Item(Command::ZoomSelection),
];

/// The right-click menu with nothing selected.
pub const CONTEXT_MENU_EMPTY: &[Entry] = &[
    Entry::Item(Command::Paste),
    Entry::Item(Command::SelectAll),
    Entry::Separator,
    Entry::Item(Command::UnhideLast),
    Entry::Item(Command::UnhideAll),
    Entry::Separator,
    Entry::Item(Command::SetPivotHere),
    Entry::Item(Command::PivotToOrigin),
    Entry::Separator,
    Entry::Item(Command::ZoomExtents),
];

/// The Items panel's commands, top to bottom.
pub const ITEMS_PANEL: &[Command] = &[
    Command::EditShape,
    Command::EditSweep,
    Command::MakeGroup,
    Command::MakeComponent,
    Command::Explode,
    Command::MakeUnique,
    Command::Lock,
    Command::ResetScale,
    Command::ChangeAxes,
    Command::Hide,
    Command::Unhide,
    Command::HideOthers,
    Command::UnhideAll,
];
