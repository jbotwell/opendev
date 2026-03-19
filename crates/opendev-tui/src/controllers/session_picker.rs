//! Session picker controller for selecting past sessions in the TUI.
//!
//! Provides a searchable session selection popup.

/// A session option displayed in the picker.
#[derive(Debug, Clone)]
pub struct SessionOption {
    /// Session identifier.
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Last updated timestamp (formatted string).
    pub updated_at: String,
    /// Number of messages in the session.
    pub message_count: usize,
}

/// Controller for navigating and selecting a session from a list.
pub struct SessionPickerController {
    /// All available sessions (unfiltered).
    all_sessions: Vec<SessionOption>,
    /// Filtered sessions matching the current search query.
    filtered_sessions: Vec<usize>,
    /// Current selected index into `filtered_sessions`.
    selected_index: usize,
    /// Whether the picker is currently active.
    active: bool,
    /// Current search/filter query.
    search_query: String,
    /// Scroll offset for the visible window.
    scroll_offset: usize,
    /// Maximum visible items in the popup.
    max_visible: usize,
}

impl Default for SessionPickerController {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionPickerController {
    /// Create a new picker with the given session options.
    pub fn new() -> Self {
        Self {
            all_sessions: Vec::new(),
            filtered_sessions: Vec::new(),
            selected_index: 0,
            active: true,
            search_query: String::new(),
            scroll_offset: 0,
            max_visible: 15,
        }
    }

    /// Create a picker pre-populated with session options.
    pub fn from_sessions(sessions: Vec<SessionOption>) -> Self {
        let filtered: Vec<usize> = (0..sessions.len()).collect();
        Self {
            all_sessions: sessions,
            filtered_sessions: filtered,
            selected_index: 0,
            active: true,
            search_query: String::new(),
            scroll_offset: 0,
            max_visible: 15,
        }
    }

    /// Whether the picker is currently active.
    pub fn active(&self) -> bool {
        self.active
    }

    /// The filtered session options to display.
    pub fn visible_sessions(&self) -> Vec<(usize, &SessionOption)> {
        self.filtered_sessions
            .iter()
            .enumerate()
            .skip(self.scroll_offset)
            .take(self.max_visible)
            .map(|(i, &session_idx)| (i, &self.all_sessions[session_idx]))
            .collect()
    }

    /// Total number of filtered sessions.
    pub fn filtered_count(&self) -> usize {
        self.filtered_sessions.len()
    }

    /// The currently selected index in the filtered list.
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// The current search query.
    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    /// Move selection to the next item (wrapping).
    pub fn next(&mut self) {
        if self.filtered_sessions.is_empty() {
            return;
        }
        self.selected_index = (self.selected_index + 1) % self.filtered_sessions.len();
        self.ensure_visible();
    }

    /// Move selection to the previous item (wrapping).
    pub fn prev(&mut self) {
        if self.filtered_sessions.is_empty() {
            return;
        }
        self.selected_index =
            (self.selected_index + self.filtered_sessions.len() - 1) % self.filtered_sessions.len();
        self.ensure_visible();
    }

    /// Ensure the selected item is within the visible scroll window.
    fn ensure_visible(&mut self) {
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + self.max_visible {
            self.scroll_offset = self.selected_index + 1 - self.max_visible;
        }
    }

    /// Confirm the current selection and deactivate the picker.
    ///
    /// Returns `None` if the filtered list is empty.
    pub fn select(&mut self) -> Option<SessionOption> {
        if self.filtered_sessions.is_empty() {
            return None;
        }
        self.active = false;
        let session_idx = self.filtered_sessions[self.selected_index];
        Some(self.all_sessions[session_idx].clone())
    }

    /// Cancel the picker without selecting.
    pub fn cancel(&mut self) {
        self.active = false;
    }

    /// Add a character to the search query and re-filter.
    pub fn search_push(&mut self, c: char) {
        self.search_query.push(c);
        self.refilter();
    }

    /// Remove the last character from the search query and re-filter.
    pub fn search_pop(&mut self) {
        self.search_query.pop();
        self.refilter();
    }

    /// Re-filter sessions based on the current search query.
    fn refilter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_sessions = (0..self.all_sessions.len()).collect();
        } else {
            let query = self.search_query.to_lowercase();
            self.filtered_sessions = self
                .all_sessions
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    s.title.to_lowercase().contains(&query) || s.id.to_lowercase().contains(&query)
                })
                .map(|(i, _)| i)
                .collect();
        }
        self.selected_index = 0;
        self.scroll_offset = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_sessions() -> Vec<SessionOption> {
        vec![
            SessionOption {
                id: "abc123".into(),
                title: "Refactor auth module".into(),
                updated_at: "2026-03-19 10:00".into(),
                message_count: 12,
            },
            SessionOption {
                id: "def456".into(),
                title: "Fix login bug".into(),
                updated_at: "2026-03-18 15:30".into(),
                message_count: 5,
            },
            SessionOption {
                id: "ghi789".into(),
                title: "Add unit tests".into(),
                updated_at: "2026-03-17 09:00".into(),
                message_count: 20,
            },
        ]
    }

    #[test]
    fn test_new_picker() {
        let picker = SessionPickerController::from_sessions(sample_sessions());
        assert!(picker.active());
        assert_eq!(picker.selected_index(), 0);
        assert_eq!(picker.filtered_count(), 3);
    }

    #[test]
    fn test_new_empty() {
        let picker = SessionPickerController::new();
        assert!(picker.active());
        assert_eq!(picker.selected_index(), 0);
        assert_eq!(picker.filtered_count(), 0);
    }

    #[test]
    fn test_next_wraps() {
        let mut picker = SessionPickerController::from_sessions(sample_sessions());
        picker.next();
        assert_eq!(picker.selected_index(), 1);
        picker.next();
        assert_eq!(picker.selected_index(), 2);
        picker.next();
        assert_eq!(picker.selected_index(), 0); // wrap
    }

    #[test]
    fn test_prev_wraps() {
        let mut picker = SessionPickerController::from_sessions(sample_sessions());
        picker.prev();
        assert_eq!(picker.selected_index(), 2); // wrap back
        picker.prev();
        assert_eq!(picker.selected_index(), 1);
    }

    #[test]
    fn test_select() {
        let mut picker = SessionPickerController::from_sessions(sample_sessions());
        picker.next(); // select index 1
        let selected = picker.select().unwrap();
        assert_eq!(selected.id, "def456");
        assert!(!picker.active());
    }

    #[test]
    fn test_select_empty() {
        let mut picker = SessionPickerController::from_sessions(vec![]);
        assert!(picker.select().is_none());
    }

    #[test]
    fn test_cancel() {
        let mut picker = SessionPickerController::from_sessions(sample_sessions());
        picker.cancel();
        assert!(!picker.active());
    }

    #[test]
    fn test_search_filters_by_title() {
        let mut picker = SessionPickerController::from_sessions(sample_sessions());
        picker.search_push('l');
        picker.search_push('o');
        picker.search_push('g');
        picker.search_push('i');
        picker.search_push('n');
        assert_eq!(picker.filtered_count(), 1);
        let visible = picker.visible_sessions();
        assert_eq!(visible[0].1.id, "def456");
    }

    #[test]
    fn test_search_filters_by_id() {
        let mut picker = SessionPickerController::from_sessions(sample_sessions());
        picker.search_push('g');
        picker.search_push('h');
        picker.search_push('i');
        assert_eq!(picker.filtered_count(), 1);
        let visible = picker.visible_sessions();
        assert_eq!(visible[0].1.id, "ghi789");
    }

    #[test]
    fn test_search_pop_restores() {
        let mut picker = SessionPickerController::from_sessions(sample_sessions());
        picker.search_push('x');
        picker.search_push('y');
        picker.search_push('z');
        assert_eq!(picker.filtered_count(), 0);
        picker.search_pop();
        picker.search_pop();
        picker.search_pop();
        assert_eq!(picker.filtered_count(), 3);
    }

    #[test]
    fn test_next_on_empty_is_noop() {
        let mut picker = SessionPickerController::from_sessions(vec![]);
        picker.next(); // should not panic
        assert_eq!(picker.selected_index(), 0);
    }
}
