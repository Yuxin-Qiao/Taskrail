use crate::{core::Ownership, storage::Registry};
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};
use std::{
    io::{self, IsTerminal},
    path::Path,
    time::Duration,
};

pub fn run(registry_path: &Path) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(anyhow::anyhow!("interactive TUI requires a terminal"));
    }
    enable_raw_mode().context("enable terminal raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;
    let result = run_loop(&mut terminal, registry_path);
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    registry_path: &Path,
) -> Result<()> {
    loop {
        let registry = Registry::open(registry_path)?;
        let automations = registry.list_automations()?;
        let inbox = registry.list_inbox(100)?;
        let pending_approvals = registry
            .list_approvals(100)?
            .into_iter()
            .filter(|approval| approval.status == "pending")
            .count();
        let integration_count = crate::integrations::built_in_registry()?
            .descriptors()
            .len();
        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),
                    Constraint::Length(8),
                    Constraint::Length(3),
                    Constraint::Length(1),
                ])
                .split(frame.area());
            let rows = automations.iter().map(|automation| {
                let ownership = format!("{:?}", automation.ownership);
                let state = format!("{:?}", automation.runtime_state);
                let next_run = automation
                    .next_run_at
                    .map_or_else(|| "manual".to_owned(), |value| value.to_rfc3339());
                Row::new([
                    Cell::from(automation.name.clone()),
                    Cell::from(ownership),
                    Cell::from(state),
                    Cell::from(next_run),
                ])
            });
            let table = Table::new(
                rows,
                [
                    Constraint::Percentage(35),
                    Constraint::Length(12),
                    Constraint::Length(18),
                    Constraint::Min(24),
                ],
            )
            .header(
                Row::new(["NAME", "OWNERSHIP", "STATE", "NEXT RUN"]).style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Automation Control Plane "),
            )
            .column_spacing(1);
            frame.render_widget(table, chunks[0]);
            let inbox_rows = inbox.iter().take(5).map(|item| {
                Row::new([
                    Cell::from(item.severity.clone()),
                    Cell::from(item.kind.clone()),
                    Cell::from(item.title.clone()),
                    Cell::from(item.status.clone()),
                ])
            });
            let inbox_table = Table::new(
                inbox_rows,
                [
                    Constraint::Length(12),
                    Constraint::Length(20),
                    Constraint::Percentage(45),
                    Constraint::Min(18),
                ],
            )
            .header(
                Row::new(["SEVERITY", "KIND", "TITLE", "STATUS"]).style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Inbox · {} item(s) ", inbox.len())),
            )
            .column_spacing(1);
            frame.render_widget(inbox_table, chunks[1]);
            let integration_summary = Paragraph::new(format!(
                "{} typed integration(s) available · {} pending approval(s) · native discovery remains read-only",
                integration_count, pending_approvals
            ))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Integrations & approvals "),
            );
            frame.render_widget(integration_summary, chunks[2]);
            let observed = automations
                .iter()
                .filter(|automation| automation.ownership == Ownership::Observed)
                .count();
            let footer = Paragraph::new(format!(
                "{} automation(s) · {} observed · {} inbox item(s) · r refresh · q quit",
                automations.len(),
                observed,
                inbox.len()
            ));
            frame.render_widget(footer, chunks[3]);
        })?;
        if event::poll(Duration::from_millis(500)).context("poll TUI input")?
            && let Event::Key(key) = event::read().context("read TUI input")?
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('r') => {}
                _ => {}
            }
        }
    }
    Ok(())
}
