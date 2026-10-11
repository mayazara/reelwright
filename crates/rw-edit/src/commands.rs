//! Project-level and media-bin commands.

use crate::{Command, EditError};
use rw_model::{Canvas, MediaId, MediaRef, Project};

/// Adds media to the bin.
#[derive(Debug)]
pub struct AddMedia {
    media: MediaRef,
}

impl AddMedia {
    pub fn new(media: MediaRef) -> Self {
        AddMedia { media }
    }
}

impl Command for AddMedia {
    fn label(&self) -> String {
        format!("Import {}", self.media.name)
    }

    fn apply(&mut self, project: &mut Project) -> Result<(), EditError> {
        if project.media_index(self.media.id).is_some() {
            return Err(EditError::Invalid("media is already in the project".into()));
        }
        project.media.push(self.media.clone());
        Ok(())
    }

    fn revert(&mut self, project: &mut Project) {
        project.media.retain(|m| m.id != self.media.id);
    }
}

/// Removes unused media from the bin.
#[derive(Debug)]
pub struct RemoveMedia {
    id: MediaId,
    removed: Option<(usize, MediaRef)>,
}

impl RemoveMedia {
    pub fn new(id: MediaId) -> Self {
        RemoveMedia { id, removed: None }
    }
}

impl Command for RemoveMedia {
    fn label(&self) -> String {
        match &self.removed {
            Some((_, m)) => format!("Remove {}", m.name),
            None => "Remove media".into(),
        }
    }

    fn apply(&mut self, project: &mut Project) -> Result<(), EditError> {
        let index = project
            .media_index(self.id)
            .ok_or(EditError::NotFound("media"))?;
        if project.is_media_used(self.id) {
            return Err(EditError::Invalid(
                "media is used on the timeline; remove those clips first".into(),
            ));
        }
        self.removed = Some((index, project.media.remove(index)));
        Ok(())
    }

    fn revert(&mut self, project: &mut Project) {
        if let Some((index, media)) = self.removed.take() {
            project.media.insert(index.min(project.media.len()), media);
        }
    }
}

/// Renames a media item (display name only; the file is untouched).
#[derive(Debug)]
pub struct RenameMedia {
    id: MediaId,
    name: String,
}

impl RenameMedia {
    pub fn new(id: MediaId, name: impl Into<String>) -> Self {
        RenameMedia {
            id,
            name: name.into(),
        }
    }
}

impl Command for RenameMedia {
    fn label(&self) -> String {
        "Rename media".into()
    }

    fn apply(&mut self, project: &mut Project) -> Result<(), EditError> {
        let index = project
            .media_index(self.id)
            .ok_or(EditError::NotFound("media"))?;
        std::mem::swap(&mut project.media[index].name, &mut self.name);
        Ok(())
    }

    fn revert(&mut self, project: &mut Project) {
        if let Some(index) = project.media_index(self.id) {
            std::mem::swap(&mut project.media[index].name, &mut self.name);
        }
    }
}

/// Renames the project.
#[derive(Debug)]
pub struct RenameProject {
    name: String,
}

impl RenameProject {
    pub fn new(name: impl Into<String>) -> Self {
        RenameProject { name: name.into() }
    }
}

impl Command for RenameProject {
    fn label(&self) -> String {
        "Rename project".into()
    }

    fn apply(&mut self, project: &mut Project) -> Result<(), EditError> {
        if self.name.trim().is_empty() {
            return Err(EditError::Invalid("project name cannot be empty".into()));
        }
        std::mem::swap(&mut project.name, &mut self.name);
        Ok(())
    }

    fn revert(&mut self, project: &mut Project) {
        std::mem::swap(&mut project.name, &mut self.name);
    }
}

/// Changes canvas size, frame rate or background.
///
/// Items have no transform until M3, so there is nothing to re-fit yet; the
/// renderer fits media to the canvas at draw time.
#[derive(Debug)]
pub struct SetCanvas {
    canvas: Canvas,
}

impl SetCanvas {
    pub fn new(canvas: Canvas) -> Self {
        SetCanvas { canvas }
    }
}

impl Command for SetCanvas {
    fn label(&self) -> String {
        "Change canvas".into()
    }

    fn apply(&mut self, project: &mut Project) -> Result<(), EditError> {
        let c = &self.canvas;
        if !(Canvas::MIN_SIDE..=Canvas::MAX_SIDE).contains(&c.width)
            || !(Canvas::MIN_SIDE..=Canvas::MAX_SIDE).contains(&c.height)
            || c.frame_rate.num == 0
            || c.frame_rate.den == 0
        {
            return Err(EditError::Invalid("canvas settings out of range".into()));
        }
        std::mem::swap(&mut project.canvas, &mut self.canvas);
        Ok(())
    }

    fn revert(&mut self, project: &mut Project) {
        std::mem::swap(&mut project.canvas, &mut self.canvas);
    }
}
