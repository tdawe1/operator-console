use crossterm::event::KeyCode;
use operator_console::app::{App, Panel, TradingSection};

#[test]
fn app_defaults_to_trading_panel() {
    let app = App::default();

    assert_eq!(app.active_panel(), Panel::Trading);
}

#[test]
fn observability_toggle_swaps_between_trading_and_observability() {
    let mut app = App::default();

    app.handle_key(KeyCode::Char('o'));
    assert_eq!(app.active_panel(), Panel::Observability);

    app.handle_key(KeyCode::Char('o'));
    assert_eq!(app.active_panel(), Panel::Trading);
}

#[test]
fn panel_helpers_toggle_between_trading_and_observability() {
    let mut app = App::default();

    app.next_panel();
    assert_eq!(app.active_panel(), Panel::Observability);

    app.previous_panel();
    assert_eq!(app.active_panel(), Panel::Trading);
}

#[test]
fn trading_section_navigation_cycles_inside_trading_module() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);

    assert_eq!(app.active_trading_section(), TradingSection::Accounts);

    app.next_section();
    assert_eq!(app.active_trading_section(), TradingSection::Positions);

    app.previous_section();
    assert_eq!(app.active_trading_section(), TradingSection::Accounts);
}
