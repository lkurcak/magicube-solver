use std::error::Error;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use crossterm::event::{self, Event as TerminalEvent, KeyCode, KeyEventKind, KeyModifiers};
use magicube_play::replay::{self, Replay};
use magicube_play::terminal::TerminalSession;
use magicube_solver::project::{
    CachedSolveOutcome, ProjectImport, ProjectLevel, ProjectPaths, SolverCacheRecord,
    executable_fingerprint, import_project,
};
use magicube_solver::{
    GameInput, GameState, SolveOptions, SolveOutcome, SolveStats, solve_with_progress,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row as TableRow, Table, TableState, Wrap};
use ratatui::{Frame, Terminal};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("magicube-progress: {error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
enum WorkerEvent {
    Imported(ProjectImport),
    Queued(String),
    Solving(String, SolveStats),
    Solved(String, Vec<GameInput>, bool, SolveStats),
    Unsolvable(String, bool, SolveStats),
    LimitReached(String, bool, SolveStats),
    Finished,
    Fatal(String),
}

#[derive(Debug, Clone)]
enum SolveView {
    NotApplicable,
    CheckingCache,
    Queued,
    Solving(SolveStats),
    Solved {
        input_count: usize,
        cached: bool,
        stats: SolveStats,
    },
    Unsolvable {
        cached: bool,
        stats: SolveStats,
    },
    LimitReached {
        cached: bool,
        stats: SolveStats,
    },
}

#[derive(Debug, Clone)]
struct LevelRow {
    level: ProjectLevel,
    solve: SolveView,
    inputs: Option<Vec<GameInput>>,
}

struct Dashboard {
    rows: Vec<LevelRow>,
    expected_count: usize,
    selected: usize,
    /// First visible table row, kept between frames so the table only scrolls
    /// when the selection would leave the viewport.
    table_offset: usize,
    finished: bool,
    message: String,
}

impl Dashboard {
    fn loading() -> Self {
        Self {
            rows: Vec::new(),
            expected_count: 0,
            selected: 0,
            table_offset: 0,
            finished: false,
            message: "Reading trusted inputs…".to_owned(),
        }
    }

    fn apply(&mut self, event: WorkerEvent) {
        match event {
            WorkerEvent::Imported(project) => {
                self.expected_count = project.expected_count;
                self.rows = project
                    .levels
                    .into_iter()
                    .map(|level| LevelRow {
                        solve: if level.is_clean() {
                            SolveView::CheckingCache
                        } else {
                            SolveView::NotApplicable
                        },
                        level,
                        inputs: None,
                    })
                    .collect();
                self.selected = self.selected.min(self.rows.len().saturating_sub(1));
                self.message = "Imports rebuilt; validating solver cache…".to_owned();
            }
            WorkerEvent::Queued(id) => self.update(&id, |row| row.solve = SolveView::Queued),
            WorkerEvent::Solving(id, stats) => {
                self.update(&id, |row| row.solve = SolveView::Solving(stats))
            }
            WorkerEvent::Solved(id, inputs, cached, stats) => self.update(&id, |row| {
                row.solve = SolveView::Solved {
                    input_count: inputs.len(),
                    cached,
                    stats,
                };
                row.inputs = Some(inputs);
            }),
            WorkerEvent::Unsolvable(id, cached, stats) => self.update(&id, |row| {
                row.solve = SolveView::Unsolvable { cached, stats }
            }),
            WorkerEvent::LimitReached(id, cached, stats) => self.update(&id, |row| {
                row.solve = SolveView::LimitReached { cached, stats }
            }),
            WorkerEvent::Finished => {
                self.finished = true;
                self.message = "Pipeline complete. Cached artifacts are under cache/.".to_owned();
            }
            WorkerEvent::Fatal(message) => {
                self.finished = true;
                self.message = format!("Pipeline failed: {message}");
            }
        }
    }

    fn update(&mut self, id: &str, update: impl FnOnce(&mut LevelRow)) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.level.id == id) {
            update(row);
        }
    }

    fn selected(&self) -> Option<&LevelRow> {
        self.rows.get(self.selected)
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    if std::env::args_os().any(|argument| argument == "-h" || argument == "--help") {
        println!(
            "Usage: magicube-progress\n\nRebuild imports, validate cached solver results, solve clean levels, and show project progress.\n"
        );
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            io::Error::other("interactive mode requires a terminal on stdin and stdout").into(),
        );
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let paths = ProjectPaths::from_root(&root);
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || worker(paths, sender));

    let mut dashboard = Dashboard::loading();
    let mut session = TerminalSession::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(&mut session.stdout))?;
    terminal.clear()?;
    loop {
        drain_events(&receiver, &mut dashboard);
        terminal.draw(|frame| draw(frame, &mut dashboard))?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let TerminalEvent::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'C'))
        {
            break;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => break,
            KeyCode::Up | KeyCode::Char('k' | 'K') if !dashboard.rows.is_empty() => {
                dashboard.selected = dashboard
                    .selected
                    .checked_sub(1)
                    .unwrap_or(dashboard.rows.len() - 1);
            }
            KeyCode::Down | KeyCode::Char('j' | 'J') if !dashboard.rows.is_empty() => {
                dashboard.selected = (dashboard.selected + 1) % dashboard.rows.len();
            }
            KeyCode::Home => dashboard.selected = 0,
            KeyCode::End if !dashboard.rows.is_empty() => {
                dashboard.selected = dashboard.rows.len() - 1
            }
            KeyCode::Enter => {
                let replay_data = dashboard.selected().and_then(|row| {
                    Some((
                        row.level.name.clone(),
                        row.level.map.clone()?,
                        row.inputs.clone()?,
                    ))
                });
                if let Some((name, map, inputs)) = replay_data {
                    let initial = GameState::from_ascii(&map)?;
                    drop(terminal);
                    replay::run(
                        &mut session.stdout,
                        &format!("{name} (cached solver replay)"),
                        Replay::new(initial, inputs),
                    )?;
                    terminal = Terminal::new(CrosstermBackend::new(&mut session.stdout))?;
                    terminal.clear()?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn drain_events(receiver: &Receiver<WorkerEvent>, dashboard: &mut Dashboard) {
    while let Ok(event) = receiver.try_recv() {
        dashboard.apply(event);
    }
}

fn worker(paths: ProjectPaths, sender: Sender<WorkerEvent>) {
    if let Err(error) = worker_inner(&paths, &sender) {
        let _ = sender.send(WorkerEvent::Fatal(error.to_string()));
    }
}

fn worker_inner(paths: &ProjectPaths, sender: &Sender<WorkerEvent>) -> Result<(), Box<dyn Error>> {
    let project = import_project(paths)?;
    sender.send(WorkerEvent::Imported(project.clone()))?;
    let fingerprint = executable_fingerprint()?;
    let options = SolveOptions::default();
    let solver_dir = paths.solver_cache();
    std::fs::create_dir_all(&solver_dir)?;

    let mut pending = Vec::new();
    for level in project.levels.into_iter().filter(ProjectLevel::is_clean) {
        let map = level.map.as_deref().unwrap();
        let path = solver_dir.join(format!("{}.json", level.id));
        let cached = SolverCacheRecord::load(&path).ok();
        if let Some((record, inputs)) = cached
            .as_ref()
            .and_then(|record| record.solved_inputs_for(map).map(|inputs| (record, inputs)))
        {
            sender.send(WorkerEvent::Solved(level.id, inputs, true, stats(record)))?;
            continue;
        }
        if let Some(record) = cached
            .as_ref()
            .filter(|record| record.reusable_failure(&level.id, map, options, &fingerprint))
        {
            let event = match record.outcome {
                CachedSolveOutcome::Unsolvable => {
                    WorkerEvent::Unsolvable(level.id, true, stats(record))
                }
                CachedSolveOutcome::StateLimitReached => {
                    WorkerEvent::LimitReached(level.id, true, stats(record))
                }
                CachedSolveOutcome::Solved { .. } => unreachable!(),
            };
            sender.send(event)?;
            continue;
        }
        sender.send(WorkerEvent::Queued(level.id.clone()))?;
        pending.push(level);
    }

    for level in pending {
        let map = level.map.as_deref().unwrap();
        let initial = GameState::from_ascii(map)?;
        let id = level.id.clone();
        sender.send(WorkerEvent::Solving(id.clone(), SolveStats::default()))?;
        let result = solve_with_progress(&initial, options, |stats| {
            let _ = sender.send(WorkerEvent::Solving(id.clone(), stats));
        });
        let record = SolverCacheRecord::from_result(&id, map, options, &fingerprint, &result);
        record.save(&solver_dir.join(format!("{id}.json")))?;
        let event = match result.outcome {
            SolveOutcome::Solved(inputs) => WorkerEvent::Solved(id, inputs, false, result.stats),
            SolveOutcome::Unsolvable => WorkerEvent::Unsolvable(id, false, result.stats),
            SolveOutcome::StateLimitReached => WorkerEvent::LimitReached(id, false, result.stats),
        };
        sender.send(event)?;
    }
    sender.send(WorkerEvent::Finished)?;
    Ok(())
}

fn stats(record: &SolverCacheRecord) -> SolveStats {
    SolveStats {
        discovered_states: record.stats.discovered_states,
        expanded_states: record.stats.expanded_states,
    }
}

fn draw(frame: &mut Frame, dashboard: &mut Dashboard) {
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(8),
        Constraint::Length(8),
        Constraint::Length(1),
    ])
    .split(frame.area());

    let expected = dashboard.rows.iter().filter(|row| row.level.listed);
    let screenshots = expected
        .clone()
        .filter(|row| row.level.screenshot_present)
        .count();
    let maps = expected.clone().filter(|row| row.level.is_clean()).count();
    let solved = expected
        .filter(|row| matches!(row.solve, SolveView::Solved { .. }))
        .count();
    let summary = Paragraph::new(format!(
        "Goal: {} levels  |  Screenshots: {screenshots}/{}  |  Tilemaps: {maps}/{}  |  Solved: {solved}/{}",
        dashboard.expected_count,
        dashboard.expected_count,
        dashboard.expected_count,
        dashboard.expected_count
    ))
    .block(
        Block::default()
            .title(" MAGICUBE PROJECT ")
            .borders(Borders::ALL),
    )
    .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    frame.render_widget(summary, areas[0]);

    let rows = dashboard.rows.iter().map(|row| {
        let screenshot = if !row.level.listed {
            "Unlisted"
        } else if row.level.screenshot_present {
            "Ready"
        } else {
            "Missing"
        };
        let tilemap = if row.level.is_clean() {
            match (row.level.width, row.level.height) {
                (Some(width), Some(height)) => format!("Ready {width}x{height}"),
                _ => "Ready".to_owned(),
            }
        } else if row.level.unknown_tiles > 0 {
            format!("{} unknown", row.level.unknown_tiles)
        } else {
            "Blocked".to_owned()
        };
        TableRow::new([
            Cell::from(row.level.id.clone()),
            Cell::from(screenshot),
            Cell::from(tilemap),
            Cell::from(solve_label(&row.solve)),
        ])
        .style(if row.level.is_clean() {
            Style::default()
        } else {
            Style::default().fg(Color::Red)
        })
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(20),
            Constraint::Min(24),
        ],
    )
    .header(
        TableRow::new(["Level", "Screenshot", "Tilemap", "Solution"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().borders(Borders::ALL).title(" Progress "))
    .row_highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan))
    .highlight_symbol("> ");
    let mut state = TableState::default()
        .with_selected((!dashboard.rows.is_empty()).then_some(dashboard.selected))
        .with_offset(dashboard.table_offset);
    frame.render_stateful_widget(table, areas[1], &mut state);
    dashboard.table_offset = state.offset();

    let detail = dashboard.selected().map_or_else(
        || Text::from(dashboard.message.clone()),
        |row| detail_text(row, &dashboard.message),
    );
    frame.render_widget(
        Paragraph::new(detail)
            .block(Block::default().borders(Borders::ALL).title(" Details "))
            .wrap(Wrap { trim: false }),
        areas[2],
    );
    frame.render_widget(
        Paragraph::new("Up/Down or J/K: select  Enter: replay solved level  Q/Esc: quit")
            .style(Style::default().fg(Color::Gray)),
        areas[3],
    );
}

fn solve_label(status: &SolveView) -> String {
    match status {
        SolveView::NotApplicable => "—".to_owned(),
        SolveView::CheckingCache => "Checking cache".to_owned(),
        SolveView::Queued => "Queued".to_owned(),
        SolveView::Solving(stats) => format!("Solving ({} states)", stats.discovered_states),
        SolveView::Solved {
            input_count,
            cached,
            ..
        } => format!(
            "Solved ({input_count} inputs{})",
            if *cached { ", cached" } else { "" }
        ),
        SolveView::Unsolvable { cached, .. } => {
            format!("Unsolvable{}", if *cached { " (cached)" } else { "" })
        }
        SolveView::LimitReached { cached, .. } => format!(
            "State limit reached{}",
            if *cached { " (cached)" } else { "" }
        ),
    }
}

fn detail_text(row: &LevelRow, pipeline_message: &str) -> Text<'static> {
    let mut lines = vec![Line::from(format!(
        "{} — {}",
        row.level.name,
        solve_label(&row.solve)
    ))];
    let stats = match row.solve {
        SolveView::Solving(stats)
        | SolveView::Solved { stats, .. }
        | SolveView::Unsolvable { stats, .. }
        | SolveView::LimitReached { stats, .. } => Some(stats),
        _ => None,
    };
    if let Some(stats) = stats {
        lines.push(Line::from(format!(
            "Search: {} discovered, {} expanded",
            stats.discovered_states, stats.expanded_states
        )));
    }
    for issue in row.level.issues.iter().take(4) {
        lines.push(Line::from(issue.clone()).style(Style::default().fg(Color::Red)));
    }
    lines.push(Line::from(pipeline_message.to_owned()).style(Style::default().fg(Color::Gray)));
    Text::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn level(id: &str, clean: bool) -> ProjectLevel {
        ProjectLevel {
            id: id.to_owned(),
            name: format!("Level {id}"),
            listed: true,
            screenshot_present: true,
            map: clean.then(|| {
                r#"
#####
#@ ##
##G##
"#
                .trim_matches('\n')
                .to_owned()
            }),
            width: clean.then_some(5),
            height: clean.then_some(3),
            unknown_tiles: usize::from(!clean),
            issues: (!clean)
                .then(|| "unrecognized tile".to_owned())
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn worker_events_update_replayable_rows_and_summary_state() {
        let mut dashboard = Dashboard::loading();
        dashboard.apply(WorkerEvent::Imported(ProjectImport {
            levels: vec![level("1", true), level("2", false)],
            expected_count: 2,
        }));
        dashboard.apply(WorkerEvent::Solved(
            "1".to_owned(),
            vec![GameInput::Shoot, GameInput::Right],
            true,
            SolveStats {
                discovered_states: 3,
                expanded_states: 2,
            },
        ));
        assert!(matches!(
            dashboard.rows[0].solve,
            SolveView::Solved { cached: true, .. }
        ));
        assert_eq!(dashboard.rows[0].inputs.as_ref().unwrap().len(), 2);
        assert!(matches!(dashboard.rows[1].solve, SolveView::NotApplicable));
    }

    #[test]
    fn table_and_details_render_at_compact_and_wide_sizes() {
        let mut dashboard = Dashboard::loading();
        dashboard.apply(WorkerEvent::Imported(ProjectImport {
            levels: (1..=45).map(|id| level(&id.to_string(), id <= 2)).collect(),
            expected_count: 45,
        }));
        dashboard.apply(WorkerEvent::LimitReached(
            "1".to_owned(),
            false,
            SolveStats {
                discovered_states: 1_000_000,
                expanded_states: 999_999,
            },
        ));
        for size in [(40, 12), (80, 24), (120, 35)] {
            let backend = TestBackend::new(size.0, size.1);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal.draw(|frame| draw(frame, &mut dashboard)).unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(text.contains("MAGICUBE PROJECT"));
            if size.0 >= 80 {
                assert!(text.contains("State limit reached"));
            }
            assert!(text.contains("45 levels"));
        }
    }
}
