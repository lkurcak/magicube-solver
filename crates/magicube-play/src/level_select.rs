use std::io::{self, Write};
use std::path::Path;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use magicube_solver::GameSettings;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::BundledLevel;
use magicube_play::{render, replay, solutions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Continue,
    Select(usize),
    Solve(usize),
    Saved(usize),
    Corrupted,
    ToggleAirborneShooting,
    ToggleAirbornePushing,
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

    fn handle_level_key(&mut self, key: KeyEvent, levels: &[BundledLevel]) -> Action {
        let action = self.handle_key(key);
        match action {
            Action::Select(index) | Action::Solve(index) | Action::Saved(index)
                if !levels[index].is_clean() =>
            {
                Action::Corrupted
            }
            _ => action,
        }
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
            KeyCode::Up | KeyCode::Char('k' | 'K') if self.count > 0 => {
                self.index = (self.index + self.count - 1) % self.count;
                Action::Continue
            }
            KeyCode::Down | KeyCode::Char('j' | 'J') if self.count > 0 => {
                self.index = (self.index + 1) % self.count;
                Action::Continue
            }
            KeyCode::Home => {
                self.index = 0;
                Action::Continue
            }
            KeyCode::End if self.count > 0 => {
                self.index = self.count - 1;
                Action::Continue
            }
            KeyCode::Enter if self.count > 0 => Action::Select(self.index),
            KeyCode::Char('s' | 'S') if self.count > 0 && key.kind != KeyEventKind::Repeat => {
                Action::Solve(self.index)
            }
            KeyCode::Char('r' | 'R') if self.count > 0 && key.kind != KeyEventKind::Repeat => {
                Action::Saved(self.index)
            }
            KeyCode::Char('f' | 'F') if self.count > 0 && key.kind != KeyEventKind::Repeat => {
                Action::ToggleAirborneShooting
            }
            KeyCode::Char('p' | 'P') if self.count > 0 && key.kind != KeyEventKind::Repeat => {
                Action::ToggleAirbornePushing
            }
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => Action::Quit,
            _ => Action::Continue,
        }
    }
}

pub fn choose(
    out: &mut impl Write,
    levels: &[BundledLevel],
    solve: bool,
    solutions_dir: Option<&Path>,
    play_settings: &mut GameSettings,
) -> io::Result<Option<usize>> {
    let mut selection = Selection::new(levels.len());
    let mut message = None;
    loop {
        let action = select_level(
            out,
            levels,
            &mut selection,
            solve,
            *play_settings,
            message.as_deref(),
        )?;
        message = None;
        match action {
            Action::Select(index) if !solve => match levels[index].playable_map() {
                Ok(_) => return Ok(Some(index)),
                Err(error) => message = Some(error.to_string()),
            },
            Action::Select(index) | Action::Solve(index) => {
                let level = levels[index];
                if let Err(error) = level
                    .playable_map()
                    .and_then(|map| replay::solve_and_run(out, level.name, map))
                {
                    message = Some(error.to_string());
                }
            }
            Action::Saved(index) => {
                if let Err(error) = browse_saved(out, levels[index], solutions_dir) {
                    message = Some(error.to_string());
                }
            }
            Action::Quit => return Ok(None),
            Action::Corrupted => message = Some("This level is corrupted.".to_owned()),
            Action::ToggleAirborneShooting => {
                play_settings.allow_airborne_shooting = !play_settings.allow_airborne_shooting
            }
            Action::ToggleAirbornePushing => {
                play_settings.allow_airborne_pushing = !play_settings.allow_airborne_pushing
            }
            Action::Continue => {}
        }
    }
}

fn select_level(
    out: &mut impl Write,
    levels: &[BundledLevel],
    selection: &mut Selection,
    solve: bool,
    play_settings: GameSettings,
    message: Option<&str>,
) -> io::Result<Action> {
    let mut terminal = Terminal::new(CrosstermBackend::new(out))?;
    // A replay draws outside Ratatui; discard its old screen and diff buffer.
    terminal.clear()?;
    loop {
        terminal.draw(|frame| {
            draw(
                frame,
                levels,
                selection.index,
                solve,
                play_settings,
                message,
            )
        })?;
        match event::read()? {
            Event::Key(key) => match selection.handle_level_key(key, levels) {
                Action::Continue => {}
                action => return Ok(action),
            },
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}

fn browse_saved(
    out: &mut impl Write,
    level: BundledLevel,
    directory: Option<&Path>,
) -> io::Result<()> {
    let attempts = solutions::list_for_level(level.playable_map()?, directory)?;
    let mut selection = Selection::new(attempts.entries.len());
    let mut message = None;
    loop {
        let selected = {
            let mut terminal = Terminal::new(CrosstermBackend::new(&mut *out))?;
            terminal.clear()?;
            loop {
                terminal.draw(|frame| {
                    draw_saved(
                        frame,
                        level.name,
                        &attempts,
                        selection.index,
                        message.as_deref(),
                    )
                })?;
                if let Event::Key(key) = event::read()? {
                    match selection.handle_key(key) {
                        Action::Select(index) => break Some(index),
                        Action::Quit => break None,
                        _ => {}
                    }
                }
            }
        };
        let Some(selected) = selected else {
            return Ok(());
        };
        message = None;
        match solutions::load(&attempts.entries[selected].path) {
            Ok((name, replay)) => replay::run(out, &format!("{name} (saved replay)"), replay)?,
            Err(error) => message = Some(error.to_string()),
        }
    }
}

fn draw(
    frame: &mut Frame,
    levels: &[BundledLevel],
    selected: usize,
    solve: bool,
    play_settings: GameSettings,
    message: Option<&str>,
) {
    let area = frame.area();
    let outer = Block::default()
        .title(
            Line::from(" MAGICUBE ").style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .title_bottom(Line::from(" Up/Down or J/K: choose  Q/Esc: quit ").centered())
        .borders(Borders::ALL);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let sections = Layout::vertical([Constraint::Min(1), Constraint::Length(6)]).split(inner);
    let actions = if levels.is_empty() {
        "No bundled screenshots found."
    } else if !levels[selected].is_clean() {
        "Corrupted | Play and replays unavailable"
    } else if solve {
        "Enter/S: solver replay | R: saved replays"
    } else {
        "Enter: play | S: solver replay | R: saved replays"
    };
    let help = Paragraph::new(vec![
        Line::from(actions).style(Style::default().fg(Color::Cyan)),
        Line::from(format!(
            "F: airborne shots {} | P: airborne pushes {}",
            if play_settings.allow_airborne_shooting {
                "ON"
            } else {
                "OFF"
            },
            if play_settings.allow_airborne_pushing {
                "ON"
            } else {
                "OFF"
            }
        )),
        Line::from("Fun settings: manual play only | Solver: grounded"),
        Line::from(message.unwrap_or("Close a replay to return here.")).style(Style::default().fg(
            if message.is_some() {
                Color::Red
            } else {
                Color::Gray
            },
        )),
    ])
    .wrap(Wrap { trim: false });
    frame.render_widget(help, sections[1]);
    let inner = sections[0];

    let horizontal = inner.width >= 56;
    let chunks = Layout::default()
        .direction(if horizontal {
            Direction::Horizontal
        } else {
            Direction::Vertical
        })
        .constraints(if horizontal {
            [Constraint::Length(28), Constraint::Min(1)]
        } else {
            [
                Constraint::Length((levels.len() as u16 + 2).min(inner.height / 2)),
                Constraint::Min(1),
            ]
        })
        .split(inner);

    draw_list(frame, chunks[0], levels, selected);
    if let Some(level) = levels.get(selected) {
        draw_preview(frame, chunks[1], *level);
    }
}

fn draw_saved(
    frame: &mut Frame,
    level: &str,
    attempts: &solutions::SavedAttempts,
    selected: usize,
    message: Option<&str>,
) {
    let outer = Block::default()
        .title(format!(" Saved replays: {level} "))
        .title_bottom(" Up/Down: choose  Enter: replay  Q/Esc: back ")
        .borders(Borders::ALL);
    let inner = outer.inner(frame.area());
    frame.render_widget(outer, frame.area());
    let sections = Layout::vertical([Constraint::Min(1), Constraint::Length(4)]).split(inner);
    if attempts.entries.is_empty() {
        frame.render_widget(Paragraph::new("No saved attempts for this level.\nPlay it and press P to save an attempt; wins save automatically.").wrap(Wrap { trim: false }), sections[0]);
    } else {
        let entries = attempts
            .entries
            .iter()
            .map(|entry| {
                ListItem::new(format!(
                    "{} | {} inputs | {} | {}",
                    entry.status,
                    entry.input_count,
                    entry
                        .settings
                        .map(render::rules_label)
                        .unwrap_or("Unknown rules"),
                    entry.path.file_name().unwrap_or_default().to_string_lossy()
                ))
            })
            .collect::<Vec<_>>();
        let list = List::new(entries)
            .block(Block::default().title(" Newest first "))
            .highlight_symbol("> ")
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan));
        let mut state = ListState::default().with_selected(Some(selected));
        frame.render_stateful_widget(list, sections[0], &mut state);
    }
    let footer = Paragraph::new(vec![
        Line::from(format!("Directory: {}", attempts.directory.display())),
        Line::from(message.unwrap_or("Close a replay to return to this list.")).style(
            Style::default().fg(if message.is_some() {
                Color::Red
            } else {
                Color::Gray
            }),
        ),
    ])
    .wrap(Wrap { trim: false });
    frame.render_widget(footer, sections[1]);
}

fn draw_list(frame: &mut Frame, area: Rect, levels: &[BundledLevel], selected: usize) {
    let items = levels
        .iter()
        .map(|level| {
            if level.is_clean() {
                ListItem::new(level.name)
            } else {
                ListItem::new(format!("{} — Corrupted", level.name))
                    .style(Style::default().fg(Color::Red))
            }
        })
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
    let mut state = ListState::default().with_selected((!levels.is_empty()).then_some(selected));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_preview(frame: &mut Frame, area: Rect, level: BundledLevel) {
    let preview = Paragraph::new(level.map.unwrap_or("Preview unavailable."))
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
        assert_eq!(
            selection.handle_key(key(KeyCode::Char('s'))),
            Action::Solve(2)
        );
        assert_eq!(
            selection.handle_key(key(KeyCode::Char('r'))),
            Action::Saved(2)
        );
        assert_eq!(
            selection.handle_key(key(KeyCode::Char('f'))),
            Action::ToggleAirborneShooting
        );
        assert_eq!(
            selection.handle_key(key(KeyCode::Char('p'))),
            Action::ToggleAirbornePushing
        );
        assert_eq!(selection.handle_key(key(KeyCode::Esc)), Action::Quit);
    }

    #[test]
    fn fun_toggles_ignore_repeats_releases_and_modified_shortcuts() {
        let mut selection = Selection::new(1);
        for code in [KeyCode::Char('f'), KeyCode::Char('p')] {
            for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
                assert_eq!(
                    selection.handle_key(KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind)),
                    Action::Continue
                );
            }
            assert_eq!(
                selection.handle_key(KeyEvent::new(code, KeyModifiers::CONTROL)),
                Action::Continue
            );
        }
    }

    #[test]
    fn menu_renders_at_wide_and_narrow_sizes() {
        let levels = [BundledLevel {
            id: "1",
            name: "Level 1",
            map: Some(
                r#"
###
#@#
###
"#
                .trim_matches('\n'),
            ),
            issues: &[],
        }];
        for (width, height) in [(80, 24), (30, 12), (1, 1)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).unwrap();
            for solve in [false, true] {
                terminal
                    .draw(|frame| draw(frame, &levels, 0, solve, GameSettings::default(), None))
                    .unwrap();
            }
        }
    }

    #[test]
    fn corrupted_entries_cannot_launch_play_solver_or_saved_replays() {
        for map in [Some("#@G?"), None] {
            let levels = [BundledLevel {
                id: "broken",
                name: "Level broken",
                map,
                issues: &["import failed"],
            }];
            let mut selection = Selection::new(1);
            for code in [KeyCode::Enter, KeyCode::Char('s'), KeyCode::Char('r')] {
                assert_eq!(
                    selection.handle_level_key(KeyEvent::new(code, KeyModifiers::NONE), &levels),
                    Action::Corrupted
                );
            }
            assert!(levels[0].playable_map().is_err());
            assert!(browse_saved(&mut Vec::new(), levels[0], None).is_err());
            assert_eq!(
                selection
                    .handle_level_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &levels),
                Action::Quit
            );
        }
    }

    #[test]
    fn corrupted_and_empty_catalogs_render_with_or_without_previews() {
        for map in [Some("#@G?"), None] {
            let levels = [BundledLevel {
                id: "9",
                name: "Level 9",
                map,
                issues: &["unrecognized tile"],
            }];
            for (width, height) in [(80, 24), (30, 12), (1, 1)] {
                for entries in [&levels[..], &[][..]] {
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    for solve in [false, true] {
                        terminal
                            .draw(|frame| {
                                draw(frame, entries, 0, solve, GameSettings::default(), None)
                            })
                            .unwrap();
                    }
                    if width == 80 && !entries.is_empty() {
                        let screen = terminal
                            .backend()
                            .buffer()
                            .content
                            .iter()
                            .map(|cell| cell.symbol())
                            .collect::<String>();
                        assert!(screen.contains("Level 9 — Corrupted"));
                        assert!(screen.contains(map.unwrap_or("Preview unavailable.")));
                    }
                }
            }
        }
    }

    #[test]
    fn build_bundles_the_screenshot_only_levels_and_clean_entries_launch() {
        for id in ["9", "10"] {
            let index = crate::BUNDLED_LEVELS
                .iter()
                .position(|level| level.id == id)
                .unwrap();
            let level = crate::BUNDLED_LEVELS[index];
            assert!(level.is_clean(), "{level:?}");
            magicube_solver::GameState::from_ascii(level.playable_map().unwrap()).unwrap();
            let mut selection = Selection {
                index,
                count: crate::BUNDLED_LEVELS.len(),
            };
            for (code, action) in [
                (KeyCode::Enter, Action::Select(index)),
                (KeyCode::Char('s'), Action::Solve(index)),
                (KeyCode::Char('r'), Action::Saved(index)),
            ] {
                assert_eq!(
                    selection.handle_level_key(
                        KeyEvent::new(code, KeyModifiers::NONE),
                        crate::BUNDLED_LEVELS
                    ),
                    action
                );
            }
        }
    }

    #[test]
    fn empty_saved_lists_can_be_closed_and_ignore_navigation_and_launch_keys() {
        let mut selection = Selection::new(0);
        for code in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::Enter,
            KeyCode::Char('s'),
            KeyCode::Char('r'),
            KeyCode::Char('f'),
            KeyCode::Char('p'),
        ] {
            assert_eq!(
                selection.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
                Action::Continue
            );
        }
        assert_eq!(
            selection.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Action::Quit
        );
    }

    #[test]
    fn saved_lists_and_errors_render_at_small_sizes() {
        let mut attempts = solutions::SavedAttempts {
            directory: "/tmp/solutions".into(),
            entries: Vec::new(),
        };
        for with_entry in [false, true] {
            if with_entry {
                attempts.entries.push(solutions::SavedAttempt {
                    path: "/tmp/solutions/attempt.json".into(),
                    status: "Won".into(),
                    input_count: 42,
                    settings: Some(GameSettings::default()),
                });
            }
            for (width, height) in [(80, 24), (30, 12), (1, 1)] {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| {
                        draw_saved(
                            frame,
                            "Level 1",
                            &attempts,
                            0,
                            Some("Cannot load this saved attempt"),
                        )
                    })
                    .unwrap();
            }
        }
    }
}
