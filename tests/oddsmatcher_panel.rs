use crossterm::event::KeyCode;
use operator_console::app::{App, OddsMatcherFocus, Panel, TradingSection};
use operator_console::oddsmatcher::OddsMatcherField;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

#[test]
fn trading_navigation_reaches_oddsmatcher_section() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);

    app.next_section();
    app.next_section();
    app.next_section();

    assert_eq!(app.active_trading_section(), TradingSection::OddsMatcher);
}

#[test]
fn oddsmatcher_refresh_loads_live_rows() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::OddsMatcher);

    app.handle_key(KeyCode::Char('r'));

    assert!(!app.oddsmatcher_rows().is_empty());
    assert!(app.status_message().contains("OddsMatcher") || app.status_message().contains("Loaded"));
}

#[test]
fn oddsmatcher_selection_moves_with_arrow_keys() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::OddsMatcher);
    app.handle_key(KeyCode::Char('r'));

    assert_eq!(app.selected_oddsmatcher_row().map(|row| row.id.clone()).is_some(), true);

    let first = app.selected_oddsmatcher_row().expect("first row").id.clone();
    app.handle_key(KeyCode::Down);
    let second = app.selected_oddsmatcher_row().expect("selected row").id.clone();

    if app.oddsmatcher_rows().len() > 1 {
        assert_ne!(first, second);
    } else {
        assert_eq!(first, second);
    }
}

#[test]
fn oddsmatcher_filters_are_editable_from_the_app() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::OddsMatcher);

    app.handle_key(KeyCode::Left);
    assert_eq!(app.oddsmatcher_focus(), OddsMatcherFocus::Filters);

    while app.oddsmatcher_selected_field() != OddsMatcherField::Limit {
        app.handle_key(KeyCode::Down);
    }

    app.handle_key(KeyCode::Enter);
    assert!(app.oddsmatcher_is_editing());
    app.handle_key(KeyCode::Char('2'));
    app.handle_key(KeyCode::Char('5'));
    app.handle_key(KeyCode::Enter);

    assert_eq!(app.oddsmatcher_query().limit, 25);
}

#[test]
fn oddsmatcher_panel_renders_filter_sidebar() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::OddsMatcher);

    let backend = TestBackend::new(160, 36);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| operator_console::ui::render(frame, &mut app))
        .expect("draw ui");

    let buffer = terminal.backend().buffer().clone();
    let area = buffer.area;
    let mut lines = Vec::new();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buffer.cell((x, y)).expect("cell").symbol());
        }
        lines.push(line);
    }
    let rendered = lines.join("\n");

    assert!(rendered.contains("Filters"));
    assert!(rendered.contains("Live Matches"));
    assert!(rendered.contains("Selection"));
    assert!(rendered.contains("Bookmaker"));
}
