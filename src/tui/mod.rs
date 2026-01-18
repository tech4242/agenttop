pub mod app;
pub mod ui;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use std::io;
use std::time::Duration;

use crate::storage::StorageHandle;
use app::App;

pub async fn run(storage: StorageHandle) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new(storage);

    // Run the main loop
    let res = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        // Refresh data from storage
        app.refresh()?;

        // Draw UI
        terminal.draw(|f| ui::draw(f, app))?;

        // Handle input with timeout for refresh
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('s') => app.toggle_sort(),
                KeyCode::Char('p') => app.toggle_pause(),
                KeyCode::Char('d') => app.toggle_detail(),
                KeyCode::Char('t') => app.toggle_time_filter(),
                KeyCode::Char('r') => app.reset_stats(),
                KeyCode::Up | KeyCode::Char('k') => app.select_previous(),
                KeyCode::Down | KeyCode::Char('j') => app.select_next(),
                KeyCode::Enter => app.toggle_detail(),
                KeyCode::Esc => app.close_detail(),
                _ => {}
            }
        }
    }
}
