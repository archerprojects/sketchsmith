//! SketchSmith — Geometry panel layout | v1.0.0 | Thursday 12 November 2026
//!
//! The customisable arrangement of the Geometry panel:
//! which groups exist, their order, the buttons in each and their order, the
//! buttons the user has hidden, and the groups floating over the canvas as
//! SketchUp-style toolbars. Each mode (Edit, Present) has its own layout, saved
//! in `settings.toml`.
//!
//! The layout refers to tools by name only; the shell's tool catalog supplies
//! everything else (icons and actions). [`PanelLayout::reconcile`] keeps
//! a saved layout in step with the catalog of the running version: tools that
//! no longer exist are dropped, and tools new to this version appear in their
//! default group unless the user hid them.
//!
//! This module has no UI code, so every rearrangement is unit-tested.

use serde::{Deserialize, Serialize};

/// A default group: its title and tool names, in order.
pub type DefaultGroup<'a> = (&'a str, &'a [&'a str]);

/// One panel group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupLayout {
    /// Group title, e.g. "Draw".
    pub title: String,
    /// Tool names, in button order.
    pub tools: Vec<String>,
    /// Where the group floats over the window (top-left, points), or `None`
    /// while it is in the Geometry panel.
    #[serde(default)]
    pub floating: Option<[f32; 2]>,
}

/// The arrangement of one mode's Geometry panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelLayout {
    /// Groups, in panel order (floating groups keep their place for returning to the panel).
    pub groups: Vec<GroupLayout>,
    /// Tools the user has removed from every group.
    #[serde(default)]
    pub hidden: Vec<String>,
}

impl PanelLayout {
    /// The default layout from the catalog's default groups.
    pub fn from_defaults(defaults: &[DefaultGroup<'_>]) -> Self {
        Self {
            groups: defaults
                .iter()
                .map(|(title, tools)| GroupLayout {
                    title: (*title).to_owned(),
                    tools: tools.iter().map(|t| (*t).to_owned()).collect(),
                    floating: None,
                })
                .collect(),
            hidden: Vec::new(),
        }
    }

    /// Brings a saved layout in step with the catalog: unknown tools and
    /// duplicates are dropped, and so is a group the catalog no longer has
    /// once it is empty; catalog tools that are neither placed nor hidden
    /// are added to their default group (re-created at the end if the user's
    /// layout has no group of that title).
    pub fn reconcile(&mut self, defaults: &[DefaultGroup<'_>]) {
        let known = |name: &str| defaults.iter().any(|(_, tools)| tools.contains(&name));
        let mut seen: Vec<String> = Vec::new();
        for g in &mut self.groups {
            g.tools.retain(|t| {
                let keep = known(t) && !seen.contains(t);
                if keep {
                    seen.push(t.clone());
                }
                keep
            });
        }
        // A group the catalog no longer has, left empty, goes too.
        self.groups
            .retain(|g| !g.tools.is_empty() || defaults.iter().any(|(t, _)| *t == g.title));
        self.hidden.retain(|t| known(t) && !seen.contains(t));
        seen.extend(self.hidden.iter().cloned());
        for (title, tools) in defaults {
            for tool in tools.iter() {
                if seen.iter().any(|s| s == tool) {
                    continue;
                }
                let index = match self.groups.iter().position(|g| g.title == *title) {
                    Some(i) => i,
                    None => {
                        self.groups.push(GroupLayout {
                            title: (*title).to_owned(),
                            tools: Vec::new(),
                            floating: None,
                        });
                        self.groups.len() - 1
                    }
                };
                self.groups[index].tools.push((*tool).to_owned());
                seen.push((*tool).to_owned());
            }
        }
    }

    /// Removes `tool` from wherever it is (a group or the hidden list).
    fn take(&mut self, tool: &str) -> Option<String> {
        for g in &mut self.groups {
            if let Some(i) = g.tools.iter().position(|t| t == tool) {
                return Some(g.tools.remove(i));
            }
        }
        let i = self.hidden.iter().position(|t| t == tool)?;
        Some(self.hidden.remove(i))
    }

    /// Moves `tool` into group `group`, before the button at `before` (or at
    /// the end). Works for placed and hidden tools alike. Indices refer to the
    /// layout as it is before the move.
    pub fn move_tool(&mut self, tool: &str, group: usize, before: Option<usize>) {
        if group >= self.groups.len() {
            return;
        }
        // Account for the tool leaving the same group ahead of the target slot.
        let shift = self.groups[group]
            .tools
            .iter()
            .position(|t| t == tool)
            .zip(before)
            .is_some_and(|(from, to)| from < to);
        let Some(name) = self.take(tool) else {
            return;
        };
        let tools = &mut self.groups[group].tools;
        let at = before
            .map(|b| if shift { b - 1 } else { b })
            .unwrap_or(tools.len())
            .min(tools.len());
        tools.insert(at, name);
    }

    /// Hides `tool`: it leaves its group and is listed as hidden.
    pub fn hide(&mut self, tool: &str) {
        if self.hidden.iter().any(|t| t == tool) {
            return;
        }
        if let Some(name) = self.take(tool) {
            self.hidden.push(name);
        }
    }

    /// Moves group `from` to position `to` in the Geometry panel.
    pub fn move_group(&mut self, from: usize, to: usize) {
        if from < self.groups.len() && to < self.groups.len() {
            let g = self.groups.remove(from);
            self.groups.insert(to, g);
        }
    }

    /// Floats group `group` at `pos`, or docks it back with `None`.
    pub fn set_floating(&mut self, group: usize, pos: Option<[f32; 2]>) {
        if let Some(g) = self.groups.get_mut(group) {
            g.floating = pos;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULTS: &[DefaultGroup<'_>] = &[
        ("Draw", &["Line", "Rectangle", "Circle"]),
        ("Camera", &["Orbit", "Pan"]),
    ];

    fn names(l: &PanelLayout, g: usize) -> Vec<&str> {
        l.groups[g].tools.iter().map(String::as_str).collect()
    }

    #[test]
    fn defaults_follow_the_catalog() {
        let l = PanelLayout::from_defaults(DEFAULTS);
        assert_eq!(names(&l, 0), ["Line", "Rectangle", "Circle"]);
        assert_eq!(l.groups[1].title, "Camera");
        assert!(l.hidden.is_empty());
    }

    #[test]
    fn reorder_within_and_across_groups() {
        let mut l = PanelLayout::from_defaults(DEFAULTS);
        l.move_tool("Circle", 0, Some(0));
        assert_eq!(names(&l, 0), ["Circle", "Line", "Rectangle"]);
        // Moving forward within a group lands before the target button.
        l.move_tool("Circle", 0, Some(2));
        assert_eq!(names(&l, 0), ["Line", "Circle", "Rectangle"]);
        l.move_tool("Pan", 0, None);
        assert_eq!(names(&l, 0), ["Line", "Circle", "Rectangle", "Pan"]);
        assert_eq!(names(&l, 1), ["Orbit"]);
    }

    #[test]
    fn hide_and_add_back() {
        let mut l = PanelLayout::from_defaults(DEFAULTS);
        l.hide("Rectangle");
        assert_eq!(names(&l, 0), ["Line", "Circle"]);
        assert_eq!(l.hidden, ["Rectangle"]);
        l.move_tool("Rectangle", 1, Some(1));
        assert_eq!(names(&l, 1), ["Orbit", "Rectangle", "Pan"]);
        assert!(l.hidden.is_empty());
    }

    #[test]
    fn groups_move_and_float() {
        let mut l = PanelLayout::from_defaults(DEFAULTS);
        l.move_group(1, 0);
        assert_eq!(l.groups[0].title, "Camera");
        l.set_floating(0, Some([300.0, 120.0]));
        assert_eq!(l.groups[0].floating, Some([300.0, 120.0]));
        l.set_floating(0, None);
        assert_eq!(l.groups[0].floating, None);
    }

    #[test]
    fn reconcile_keeps_choices_and_adds_new_tools() {
        let mut l = PanelLayout::from_defaults(DEFAULTS);
        l.hide("Pan");
        l.move_tool("Circle", 0, Some(0));
        l.groups[0].tools.push("Retired Tool".into());
        let newer: &[DefaultGroup<'_>] = &[
            ("Draw", &["Line", "Rectangle", "Circle", "Arc"]),
            ("Camera", &["Orbit", "Pan"]),
            ("Measure", &["Tape Measure"]),
        ];
        l.reconcile(newer);
        assert_eq!(names(&l, 0), ["Circle", "Line", "Rectangle", "Arc"]);
        assert_eq!(l.hidden, ["Pan"], "hidden stays hidden");
        assert_eq!(l.groups[2].title, "Measure");
        assert_eq!(names(&l, 2), ["Tape Measure"]);
    }

    #[test]
    fn layout_survives_a_settings_round_trip() {
        let mut l = PanelLayout::from_defaults(DEFAULTS);
        l.hide("Line");
        l.set_floating(1, Some([10.5, 20.0]));
        let text = toml::to_string(&l).unwrap();
        let back: PanelLayout = toml::from_str(&text).unwrap();
        assert_eq!(back, l);
    }

    #[test]
    fn a_group_the_catalog_dropped_goes_once_empty() {
        let mut l = PanelLayout::from_defaults(DEFAULTS);
        l.groups.push(GroupLayout {
            title: "Retired".into(),
            tools: vec!["No Such Tool".into()],
            floating: None,
        });
        l.reconcile(DEFAULTS);
        assert!(l.groups.iter().all(|g| g.title != "Retired"));
    }
}
