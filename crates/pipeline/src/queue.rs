//! Question queue: at most one `Active` question, interruptions wait as `Pending`.

use meetcue_core::{FocusTarget, QuestionId, QuestionStatus};

/// How a newly detected question enters the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// Becomes active if nothing is active, otherwise waits.
    Queue,
    /// The user already answered the active question and the other side moved
    /// on: finish the active question first, then queue.
    AfterAnswer,
    /// Interruptions are not queued: the new question takes over and the
    /// previous active one goes back to waiting.
    Preempt,
}

#[derive(Debug, Default)]
pub struct QuestionQueue {
    items: Vec<(QuestionId, QuestionStatus)>,
    /// Explicit focus set by the user; `None` follows the active question.
    focus: Option<QuestionId>,
}

/// Status changes produced by one operation, in the order they happened.
pub type Changes = Vec<(QuestionId, QuestionStatus)>;

impl QuestionQueue {
    pub fn active(&self) -> Option<QuestionId> {
        self.items
            .iter()
            .find(|(_, s)| *s == QuestionStatus::Active)
            .map(|(id, _)| *id)
    }

    pub fn status(&self, id: QuestionId) -> Option<QuestionStatus> {
        self.items.iter().find(|(i, _)| *i == id).map(|(_, s)| *s)
    }

    pub fn items(&self) -> &[(QuestionId, QuestionStatus)] {
        &self.items
    }

    pub fn pending_count(&self) -> usize {
        self.items
            .iter()
            .filter(|(_, s)| *s == QuestionStatus::Pending)
            .count()
    }

    /// The question shown as "current": the user's focus, else the active one,
    /// else the most recent question.
    pub fn focus(&self) -> Option<QuestionId> {
        self.focus
            .or_else(|| self.active())
            .or_else(|| self.items.last().map(|(id, _)| *id))
    }

    pub fn add(&mut self, id: QuestionId, admission: Admission) -> Changes {
        let mut changes = Vec::new();
        match admission {
            Admission::AfterAnswer => {
                changes.extend(self.finish_active());
            }
            Admission::Preempt => {
                if let Some(active) = self.active() {
                    self.set(active, QuestionStatus::Pending, &mut changes);
                }
            }
            Admission::Queue => {}
        }
        let status = if self.active().is_none() {
            QuestionStatus::Active
        } else {
            QuestionStatus::Pending
        };
        self.items.push((id, status));
        changes.push((id, status));
        if status == QuestionStatus::Active {
            self.focus = None;
        }
        changes
    }

    /// Marks the active question answered and promotes the earliest pending one.
    pub fn finish_active(&mut self) -> Changes {
        let mut changes = Vec::new();
        if let Some(active) = self.active() {
            self.set(active, QuestionStatus::Answered, &mut changes);
            self.promote(&mut changes);
        }
        changes
    }

    /// Removes a question that turned out not to be one.
    pub fn remove(&mut self, id: QuestionId) -> Changes {
        let mut changes = Vec::new();
        let was_active = self.status(id) == Some(QuestionStatus::Active);
        self.items.retain(|(i, _)| *i != id);
        if self.focus == Some(id) {
            self.focus = None;
        }
        if was_active {
            self.promote(&mut changes);
        }
        changes
    }

    pub fn set_focus(&mut self, id: QuestionId) -> bool {
        if self.status(id).is_none() {
            return false;
        }
        self.focus = if Some(id) == self.active() {
            None
        } else {
            Some(id)
        };
        true
    }

    /// Moves the focus one question back or forward in time order.
    pub fn step_focus(&mut self, target: FocusTarget) -> bool {
        let Some(current) = self.focus() else {
            return false;
        };
        let Some(pos) = self.items.iter().position(|(id, _)| *id == current) else {
            return false;
        };
        let new_pos = match target {
            FocusTarget::Prev if pos > 0 => pos - 1,
            FocusTarget::Next if pos + 1 < self.items.len() => pos + 1,
            _ => return false,
        };
        let id = self.items[new_pos].0;
        self.set_focus(id)
    }

    fn promote(&mut self, changes: &mut Changes) {
        if self.active().is_some() {
            return;
        }
        if let Some(next) = self
            .items
            .iter()
            .find(|(_, s)| *s == QuestionStatus::Pending)
            .map(|(id, _)| *id)
        {
            self.set(next, QuestionStatus::Active, changes);
        }
        self.focus = None;
    }

    fn set(&mut self, id: QuestionId, status: QuestionStatus, changes: &mut Changes) {
        if let Some(item) = self.items.iter_mut().find(|(i, _)| *i == id) {
            if item.1 != status {
                item.1 = status;
                changes.push((id, status));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use QuestionStatus::*;

    #[test]
    fn first_question_is_active_and_interruptions_wait() {
        let mut q = QuestionQueue::default();
        assert_eq!(q.add(1, Admission::Queue), vec![(1, Active)]);
        assert_eq!(q.add(2, Admission::Queue), vec![(2, Pending)]);
        assert_eq!(q.add(3, Admission::Queue), vec![(3, Pending)]);
        assert_eq!(q.active(), Some(1));
        assert_eq!(q.pending_count(), 2);
        // Finishing promotes the earliest pending question.
        assert_eq!(q.finish_active(), vec![(1, Answered), (2, Active)]);
        assert_eq!(q.finish_active(), vec![(2, Answered), (3, Active)]);
        assert_eq!(q.finish_active(), vec![(3, Answered)]);
        assert_eq!(q.active(), None);
        assert!(q.finish_active().is_empty());
    }

    #[test]
    fn new_question_after_answer_finishes_active() {
        let mut q = QuestionQueue::default();
        q.add(1, Admission::Queue);
        assert_eq!(
            q.add(2, Admission::AfterAnswer),
            vec![(1, Answered), (2, Active)]
        );
        // With a waiting interruption, it gets its turn before the new question.
        q.add(3, Admission::Queue);
        assert_eq!(
            q.add(4, Admission::AfterAnswer),
            vec![(2, Answered), (3, Active), (4, Pending)]
        );
    }

    #[test]
    fn preempt_puts_active_back() {
        let mut q = QuestionQueue::default();
        q.add(1, Admission::Queue);
        assert_eq!(
            q.add(2, Admission::Preempt),
            vec![(1, Pending), (2, Active)]
        );
        assert_eq!(q.finish_active(), vec![(2, Answered), (1, Active)]);
    }

    #[test]
    fn at_most_one_active() {
        let mut q = QuestionQueue::default();
        for (i, adm) in [
            Admission::Queue,
            Admission::Preempt,
            Admission::AfterAnswer,
            Admission::Queue,
        ]
        .into_iter()
        .enumerate()
        {
            q.add(i as u32, adm);
            let actives = q.items().iter().filter(|(_, s)| *s == Active).count();
            assert!(actives <= 1);
        }
    }

    #[test]
    fn remove_active_promotes() {
        let mut q = QuestionQueue::default();
        q.add(1, Admission::Queue);
        q.add(2, Admission::Queue);
        assert_eq!(q.remove(1), vec![(2, Active)]);
        assert_eq!(q.items().len(), 1);
    }

    #[test]
    fn focus_is_display_only() {
        let mut q = QuestionQueue::default();
        q.add(1, Admission::Queue);
        q.add(2, Admission::Queue);
        q.add(3, Admission::Queue);
        assert_eq!(q.focus(), Some(1));
        assert!(q.step_focus(FocusTarget::Next));
        assert_eq!(q.focus(), Some(2));
        assert_eq!(q.active(), Some(1), "focus must not change status");
        assert!(q.step_focus(FocusTarget::Next));
        assert!(!q.step_focus(FocusTarget::Next));
        assert!(q.set_focus(1));
        assert!(!q.step_focus(FocusTarget::Prev));
        // When the active question changes, focus follows it again.
        q.set_focus(3);
        q.finish_active();
        assert_eq!(q.focus(), Some(2));
    }
}
