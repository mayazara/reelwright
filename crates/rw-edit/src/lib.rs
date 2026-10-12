//! Command pattern: every edit is an undoable [`Command`] applied to the
//! project model, recorded in a [`History`].

pub mod commands;
pub mod timeline;

use rw_model::Project;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("{0}")]
    Invalid(String),
}

/// One reversible change to a project.
///
/// `apply` must leave the project unchanged when it returns an error, and
/// `revert` must restore exactly the state `apply` started from.
pub trait Command: fmt::Debug + Send {
    /// Short description for the Edit menu, e.g. "Import 3 files".
    fn label(&self) -> String;
    fn apply(&mut self, project: &mut Project) -> Result<(), EditError>;
    fn revert(&mut self, project: &mut Project);
    /// Edits with the same key made in quick succession (dragging in a
    /// colour picker, typing) undo as one step.
    fn coalesce_key(&self) -> Option<String> {
        None
    }
}

/// Several commands applied and undone as one step.
#[derive(Debug)]
pub struct Compound {
    label: String,
    commands: Vec<Box<dyn Command>>,
}

impl Compound {
    pub fn new(label: impl Into<String>, commands: Vec<Box<dyn Command>>) -> Self {
        Compound {
            label: label.into(),
            commands,
        }
    }
}

impl Command for Compound {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn apply(&mut self, project: &mut Project) -> Result<(), EditError> {
        for i in 0..self.commands.len() {
            if let Err(e) = self.commands[i].apply(project) {
                for done in self.commands[..i].iter_mut().rev() {
                    done.revert(project);
                }
                return Err(e);
            }
        }
        Ok(())
    }

    fn revert(&mut self, project: &mut Project) {
        for c in self.commands.iter_mut().rev() {
            c.revert(project);
        }
    }
}

struct Entry {
    seq: u64,
    command: Box<dyn Command>,
    /// Entries sharing a group undo and redo together.
    group: u64,
    key: Option<String>,
    at: std::time::Instant,
}

/// Edits with the same coalesce key closer together than this merge.
pub const COALESCE_WINDOW: std::time::Duration = std::time::Duration::from_millis(1500);

/// Undo and redo stacks. Unlimited depth.
#[derive(Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    next_seq: u64,
    saved_state: u64,
    /// Merge quick same-key edits into one step (on by default).
    no_coalescing: bool,
}

impl fmt::Debug for History {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("History")
            .field("undo", &self.undo.len())
            .field("redo", &self.redo.len())
            .field("state", &self.state())
            .finish()
    }
}

impl History {
    pub fn new() -> Self {
        History {
            next_seq: 1,
            ..Default::default()
        }
    }

    /// Turns merging of quick same-key edits on or off.
    pub fn set_coalescing(&mut self, on: bool) {
        self.no_coalescing = !on;
    }

    /// Applies `command` and records it. Clears the redo stack.
    pub fn execute(
        &mut self,
        project: &mut Project,
        mut command: Box<dyn Command>,
    ) -> Result<(), EditError> {
        command.apply(project)?;
        let seq = self.next_seq;
        self.next_seq += 1;
        let key = command.coalesce_key();
        let now = std::time::Instant::now();
        let group = match self.undo.last() {
            Some(top)
                if !self.no_coalescing
                    && key.is_some()
                    && top.key == key
                    && now.duration_since(top.at) < COALESCE_WINDOW
                    && self.saved_state != top.seq =>
            {
                top.group
            }
            _ => seq,
        };
        self.undo.push(Entry {
            seq,
            command,
            group,
            key,
            at: now,
        });
        self.redo.clear();
        Ok(())
    }

    /// Undoes the last step (a command, or a run of merged ones). Returns
    /// its label.
    pub fn undo(&mut self, project: &mut Project) -> Option<String> {
        let group = self.undo.last()?.group;
        let mut label = None;
        while self.undo.last().is_some_and(|e| e.group == group) {
            let mut entry = self.undo.pop().expect("checked");
            entry.command.revert(project);
            label = Some(entry.command.label());
            self.redo.push(entry);
        }
        label
    }

    /// Re-applies the last undone step. Returns its label.
    pub fn redo(&mut self, project: &mut Project) -> Option<String> {
        let group = self.redo.last()?.group;
        let mut label = None;
        while self.redo.last().is_some_and(|e| e.group == group) {
            let mut entry = self.redo.pop().expect("checked");
            match entry.command.apply(project) {
                Ok(()) => {
                    label = Some(entry.command.label());
                    self.undo.push(entry);
                }
                // A command that applied once should apply again; if it does
                // not, drop the redo stack rather than leave it inconsistent.
                Err(_) => {
                    self.redo.clear();
                    return None;
                }
            }
        }
        label
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<String> {
        self.undo.last().map(|e| e.command.label())
    }

    pub fn redo_label(&self) -> Option<String> {
        self.redo.last().map(|e| e.command.label())
    }

    /// Identifies the current position in history; equal values mean the
    /// project content is the same.
    pub fn state(&self) -> u64 {
        self.undo.last().map_or(0, |e| e.seq)
    }

    /// Records that the current state has been written to disk.
    pub fn mark_saved(&mut self) {
        self.saved_state = self.state();
    }

    /// Whether the project differs from what was last saved.
    pub fn is_dirty(&self) -> bool {
        self.state() != self.saved_state
    }
}

#[cfg(test)]
mod tests;
