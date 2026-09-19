use std::io::{self, Write};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::BundledLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Continue,
    Select(usize),
    Quit,
}

#[derive(Debug)]
struct Selection {
    index: usize,
    count: usize,
}

impl Selection {
    fn new(count: usize) -> Self {
        Self { index: 0, count }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::Continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'C'))
        {
            return Action::Quit;
        }
        if !(key.modifiers - KeyModifiers::SHIFT).is_empty() {
            return Action::Continue;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k' | 'K') => {
                self.index = (self.index + self.count - 1) % self.count;
                Action::Continue
            }
            KeyCode::Down | KeyCode::Char('j' | 'J') => {
                self.index = (self.index + 1) % self.count;
                Action::Continue
            }
            KeyCode::Home => {
                self.index = 0;
                Action::Continue
            }
            KeyCode::End => {
                self.index = self.count - 1;
                Action::Continue
            }
            KeyCode::Enter => Action::Select(self.index),
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => Action::Quit,
            _ => Action::Continue,
        }
    }
}

pub fn choose(out: &mut impl Write, levels: &[BundledLevel]) -> io::Result<Option<usize>> {
    assert!(
        !levels.is_empty(),
        "level selector requires at least one level"
    );
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;
    let mut selection = Selection::new(levels.len());

    loop {
        terminal.draw(|frame| draw(frame, levels, selection.index))?;
        match event::read()? {
            Event::Key(key) => match selection.handle_key(key) {
                Action::Continue => {}
                Action::Select(index) => return Ok(Some(index)),
                Action::Quit => return Ok(None),
            },
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}

fn draw(frame: &mut Frame, levels: &[BundledLevel], selected: usize) {
    let area = frame.area();
    let outer = Block::default()
        .title(
            Line::from(" MAGICUBE ").style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .title_bottom(Line::from(" Up/Down or J/K: choose  Enter: play  Q/Esc: quit ").centered())
        .borders(Borders::ALL);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let horizontal = inner.width >= 56;
    let chunks = Layout::default()
        .direction(if horizontal {
            Direction::Horizontal
        } else {
            Direction::Vertical
        })
        .constraints(if horizontal {
            [Constraint::Length(22), Constraint::Min(1)]
        } else {
            [
                Constraint::Length((levels.len() as u16 + 2).min(inner.height / 2)),
                Constraint::Min(1),
            ]
        })
        .split(inner);

    draw_list(frame, chunks[0], levels, selected);
    draw_preview(frame, chunks[1], levels[selected]);
}

fn draw_list(frame: &mut Frame, area: Rect, levels: &[BundledLevel], selected: usize) {
    let items = levels
        .iter()
        .map(|level| ListItem::new(level.name))
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(
            Block::default()
                .title(" Select a level ")
                .borders(Borders::ALL),
        )
        .highlight_symbol("> ")
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_preview(frame: &mut Frame, area: Rect, level: BundledLevel) {
    let preview = Paragraph::new(level.map)
        .block(Block::default().title(" Preview ").borders(Borders::ALL))
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: false });
    frame.render_widget(preview, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn selection_wraps_and_can_launch_or_quit() {
        let mut selection = Selection::new(3);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert_eq!(selection.handle_key(key(KeyCode::Up)), Action::Continue);
        assert_eq!(selection.index, 2);
        assert_eq!(selection.handle_key(key(KeyCode::Down)), Action::Continue);
        assert_eq!(selection.index, 0);
        selection.handle_key(key(KeyCode::End));
        assert_eq!(selection.handle_key(key(KeyCode::Enter)), Action::Select(2));
        assert_eq!(selection.handle_key(key(KeyCode::Esc)), Action::Quit);
    }

    #[test]
    fn menu_renders_at_wide_and_narrow_sizes() {
        let levels = [BundledLevel {
            name: "Level 1",
            map: "###\n#@#\n###",
        }];
        for (width, height) in [(80, 24), (30, 12), (1, 1)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal.draw(|frame| draw(frame, &levels, 0)).unwrap();
        }
    }
}
