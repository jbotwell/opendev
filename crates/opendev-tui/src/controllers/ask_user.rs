//! Ask-user prompt controller for the TUI.
//!
//! Displays a question with numbered options and tracks the user's selection.
//! The key handler is responsible for sending the answer through the response
//! channel stored in `App::ask_user_response_tx`.

/// Controller for displaying questions with selectable options.
pub struct AskUserController {
    question: String,
    options: Vec<String>,
    default: Option<String>,
    selected: usize,
    active: bool,
}

impl AskUserController {
    /// Create a new inactive ask-user controller.
    pub fn new() -> Self {
        Self {
            question: String::new(),
            options: Vec::new(),
            default: None,
            selected: 0,
            active: false,
        }
    }

    /// Whether the prompt is currently active.
    pub fn active(&self) -> bool {
        self.active
    }

    /// The question being asked.
    pub fn question(&self) -> &str {
        &self.question
    }

    /// The available options.
    pub fn options(&self) -> &[String] {
        &self.options
    }

    /// The currently selected index.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// The default value (used as fallback on cancel/Esc).
    pub fn default_value(&self) -> Option<String> {
        self.default.clone()
    }

    /// Start the ask-user prompt.
    pub fn start(&mut self, question: String, options: Vec<String>, default: Option<String>) {
        self.question = question;
        self.options = options;
        self.default = default;
        self.selected = 0;
        self.active = true;
    }

    /// Move selection to the next option (wrapping).
    pub fn next(&mut self) {
        if !self.active || self.options.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.options.len();
    }

    /// Move selection to the previous option (wrapping).
    pub fn prev(&mut self) {
        if !self.active || self.options.is_empty() {
            return;
        }
        self.selected = (self.selected + self.options.len() - 1) % self.options.len();
    }

    /// Confirm the current selection and deactivate.
    ///
    /// Returns the selected option text, or `None` if options list is empty.
    /// The caller is responsible for sending the answer through the response channel.
    pub fn confirm(&mut self) -> Option<String> {
        if !self.active || self.options.is_empty() {
            return None;
        }

        let answer = self.options[self.selected].clone();
        self.cleanup();
        Some(answer)
    }

    /// Cancel the prompt and deactivate.
    /// The caller is responsible for sending the fallback through the response channel.
    pub fn cancel(&mut self) {
        if !self.active {
            return;
        }
        self.cleanup();
    }

    /// Reset to inactive state.
    fn cleanup(&mut self) {
        self.active = false;
        self.question.clear();
        self.options.clear();
        self.default = None;
        self.selected = 0;
    }
}

impl Default for AskUserController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_options() -> Vec<String> {
        vec!["Rust".into(), "Python".into(), "Go".into()]
    }

    #[test]
    fn test_new_is_inactive() {
        let ctrl = AskUserController::new();
        assert!(!ctrl.active());
    }

    #[test]
    fn test_start_activates() {
        let mut ctrl = AskUserController::new();
        ctrl.start("Pick a language?".into(), sample_options(), None);
        assert!(ctrl.active());
        assert_eq!(ctrl.options().len(), 3);
        assert_eq!(ctrl.selected_index(), 0);
        assert!(ctrl.question().contains("language"));
    }

    #[test]
    fn test_confirm_returns_selection() {
        let mut ctrl = AskUserController::new();
        ctrl.start("Pick?".into(), sample_options(), None);
        ctrl.next(); // index 1 = "Python"
        let answer = ctrl.confirm().unwrap();
        assert_eq!(answer, "Python");
        assert!(!ctrl.active());
    }

    #[test]
    fn test_cancel_deactivates() {
        let mut ctrl = AskUserController::new();
        ctrl.start("Pick?".into(), sample_options(), Some("Go".into()));
        ctrl.cancel();
        assert!(!ctrl.active());
    }

    #[test]
    fn test_default_value() {
        let mut ctrl = AskUserController::new();
        ctrl.start("Pick?".into(), sample_options(), Some("Go".into()));
        assert_eq!(ctrl.default_value(), Some("Go".into()));

        let mut ctrl2 = AskUserController::new();
        ctrl2.start("Pick?".into(), sample_options(), None);
        assert_eq!(ctrl2.default_value(), None);
    }

    #[test]
    fn test_next_prev_wraps() {
        let mut ctrl = AskUserController::new();
        ctrl.start("Q?".into(), sample_options(), None);

        ctrl.next();
        assert_eq!(ctrl.selected_index(), 1);
        ctrl.next();
        ctrl.next();
        assert_eq!(ctrl.selected_index(), 0); // wrap

        ctrl.prev();
        assert_eq!(ctrl.selected_index(), 2); // wrap back
    }

    #[test]
    fn test_confirm_empty_options() {
        let mut ctrl = AskUserController::new();
        ctrl.start("Q?".into(), vec![], None);
        assert!(ctrl.confirm().is_none());
    }
}
