#[allow(dead_code)] // used by TUI in Plan 2
pub struct TuiApp {
    selected: usize,
    scroll: Vec<usize>,
    follow_tail: Vec<bool>,
    footer_message: Option<String>,
    should_quit: bool,
}

#[allow(dead_code)] // used by TUI in Plan 2
impl TuiApp {
    pub fn new(server_count: usize) -> Self {
        let row_count = server_count + 1;
        Self {
            selected: 0,
            scroll: vec![0; row_count],
            follow_tail: vec![true; row_count],
            footer_message: None,
            should_quit: false,
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn request_quit(&mut self) {
        self.should_quit = true;
    }

    pub fn select_next(&mut self) {
        if self.scroll.is_empty() {
            return;
        }

        self.selected = (self.selected + 1) % self.scroll.len();
    }

    pub fn select_previous(&mut self) {
        if self.scroll.is_empty() {
            return;
        }

        self.selected = self
            .selected
            .checked_sub(1)
            .unwrap_or_else(|| self.scroll.len() - 1);
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll[self.selected] += amount;
        self.follow_tail[self.selected] = false;
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll[self.selected] = self.scroll[self.selected].saturating_sub(amount);
        if self.scroll[self.selected] == 0 {
            self.follow_tail[self.selected] = true;
        }
    }

    pub fn follow_tail(&mut self) {
        self.scroll[self.selected] = 0;
        self.follow_tail[self.selected] = true;
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll[self.selected]
    }

    pub fn follows_tail(&self) -> bool {
        self.follow_tail[self.selected]
    }

    pub fn set_footer_message(&mut self, message: impl Into<String>) {
        self.footer_message = Some(message.into());
    }

    pub fn clear_footer_message(&mut self) {
        self.footer_message = None;
    }

    pub fn footer_message(&self) -> Option<&str> {
        self.footer_message.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_wraps_across_servers_and_final_command() {
        let mut app = TuiApp::new(3);

        assert_eq!(app.selected(), 0);
        app.select_next();
        app.select_next();
        app.select_next();
        assert_eq!(app.selected(), 3);
        app.select_next();
        assert_eq!(app.selected(), 0);
        app.select_previous();
        assert_eq!(app.selected(), 3);
    }

    #[test]
    fn scrolling_disables_tail_follow_until_end() {
        let mut app = TuiApp::new(1);

        assert!(app.follows_tail());
        app.scroll_up(3);
        assert_eq!(app.scroll_offset(), 3);
        assert!(!app.follows_tail());
        app.scroll_down(1);
        assert_eq!(app.scroll_offset(), 2);
        assert!(!app.follows_tail());
        app.follow_tail();
        assert_eq!(app.scroll_offset(), 0);
        assert!(app.follows_tail());
    }

    #[test]
    fn footer_message_can_be_set_and_cleared() {
        let mut app = TuiApp::new(1);

        app.set_footer_message("Servers are not ready");
        assert_eq!(app.footer_message(), Some("Servers are not ready"));
        app.clear_footer_message();
        assert_eq!(app.footer_message(), None);
    }
}
