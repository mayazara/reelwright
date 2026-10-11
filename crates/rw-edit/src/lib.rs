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
}

/// Undo and redo stacks. Unlimited depth.
#[derive(Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    next_seq: u64,
    saved_state: u64,
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

    /// Applies `command` and records it. Clears the redo stack.
    pub fn execute(
        &mut self,
        project: &mut Project,
        mut command: Box<dyn Command>,
    ) -> Result<(), EditError> {
        command.apply(project)?;
        let seq = self.next_seq;
        self.next_seq += 1;
        self.undo.push(Entry { seq, command });
        self.redo.clear();
        Ok(())
    }

    /// Undoes the last command. Returns its label.
    pub fn undo(&mut self, project: &mut Project) -> Option<String> {
        let mut entry = self.undo.pop()?;
        entry.command.revert(project);
        let label = entry.command.label();
        self.redo.push(entry);
        Some(label)
    }

    /// Re-applies the last undone command. Returns its label.
    pub fn redo(&mut self, project: &mut Project) -> Option<String> {
        let mut entry = self.redo.pop()?;
        match entry.command.apply(project) {
            Ok(()) => {
                let label = entry.command.label();
                self.undo.push(entry);
                Some(label)
            }
            // A command that applied once should apply again; if it does not,
            // drop the redo stack rather than leave it inconsistent.
            Err(_) => {
                self.redo.clear();
                None
            }
        }
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
