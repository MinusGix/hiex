use std::{
    fmt::Debug,
    io::{Read, Seek, Write},
};

// TODO: make this more generic
pub trait Action<F, E>: MemoryUsage + Debug
where
    F: Read + Seek,
{
    /// Perform an action
    fn apply(&mut self, data: &mut F, _other: E) -> std::io::Result<()>;
    /// Undo this action.
    /// One can assume that the action has already been applied.
    fn unapply(&mut self, data: &mut F, _other: E) -> std::io::Result<()>;

    // TODO: can_undo / can_redo?
}

/// Used to measure how much memory something uses.
pub trait MemoryUsage {
    /// About how much memory this structure uses.
    fn memory_usage(&self) -> usize;
}

#[derive(Debug)]
pub enum ActionError {
    /// Unrecoverable. Action is removed from list.
    IoError(std::io::Error),
}
impl From<std::io::Error> for ActionError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

pub struct ActionList<F, E>
where
    F: Read + Write + Seek,
{
    actions: Vec<Box<dyn Action<F, E>>>,
    /// Index into actions.
    /// All values in positions < `index` are 'active' actions.
    index: usize,
}
impl<F, E> ActionList<F, E>
where
    F: Read + Write + Seek,
{
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
            index: 0,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            actions: Vec::with_capacity(capacity),
            index: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.actions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// The amount of entries that are 'active'.
    pub fn past_len(&self) -> usize {
        self.index
    }

    /// The amount of entries that are 'inactive'
    pub fn future_len(&self) -> usize {
        self.actions.len() - self.index
    }

    pub fn is_past_empty(&self) -> bool {
        self.past_len() == 0
    }

    pub fn is_future_empty(&self) -> bool {
        self.future_len() == 0
    }

    pub fn clear_future(&mut self) {
        let mut length = self.actions.len();
        while length > self.index {
            self.actions.pop();
            length = self.actions.len()
        }

        debug_assert!(self.is_future_empty());
    }

    /// Get the mots recently performed action's index, if one exists
    fn latest_action_index(&self) -> Option<usize> {
        if self.index == 0 {
            // There is no action that we recently performed
            None
        } else {
            // There must be one entry at least
            debug_assert!(!self.is_past_empty());
            Some(self.index - 1)
        }
    }

    /// Get the most recently performed action, if one exists.
    fn latest_action_mut(&mut self) -> Option<&mut Box<dyn Action<F, E>>> {
        let index = self.latest_action_index();
        if let Some(index) = index {
            Some(&mut self.actions[index])
        } else {
            None
        }
    }

    /// Returns `None` if there was no actions to undo.
    pub fn undo(&mut self, reader: &mut F, other: E) -> Result<Option<()>, ActionError> {
        if self.is_past_empty() {
            // No actions to undo
            Ok(None)
        } else {
            debug_assert!(self.index > 0);
            if let Err(err) = self
                .latest_action_mut()
                .expect("Expected action as the past was not empty.")
                .unapply(reader, other)
            {
                // Failure. Editor is in a somewhat indeterminate state now.
                Err(err.into())
            } else {
                // Move back a space
                // We do this here rather than before the action, because repeated undoes have a
                // slightly higher chance of fixing reality...somewhat.
                self.index -= 1;
                // We succeeded
                Ok(Some(()))
            }
        }
    }

    pub fn redo(&mut self, reader: &mut F, other: E) -> Result<Option<()>, ActionError> {
        if self.is_future_empty() {
            // No actions to redo
            Ok(None)
        } else if let Err(err) = self.actions[self.index].apply(reader, other) {
            // Failure. Editor is in a somewhat indeterminate state now.
            Err(err.into())
        } else {
            // Move forward a space
            self.index = self.index.checked_add(1).expect("Failed to do next action, as there was too many actions (which should probably be impossible)!");
            Ok(Some(()))
        }
    }

    pub fn add<A>(
        &mut self,
        mut action: A,
        reader: &mut F,
        other: E,
    ) -> Result<(), (A, ActionError)>
    where
        A: 'static + Action<F, E>,
    {
        if let Err(err) = action.apply(reader, other) {
            Err((action, err.into()))
        } else {
            self.clear_future();
            // We've applied the action correctly, so add it to the vector.
            self.actions.push(Box::new(action));
            self.index += 1;
            Ok(())
        }
    }
}
impl<F, E> MemoryUsage for ActionList<F, E>
where
    F: Read + Write + Seek,
{
    fn memory_usage(&self) -> usize {
        self.actions
            .iter()
            .fold(0usize, |acc, action| acc + action.memory_usage())
    }
}
impl<F, E> Default for ActionList<F, E>
where
    F: Read + Write + Seek,
{
    fn default() -> Self {
        Self::new()
    }
}
