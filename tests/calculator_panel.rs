use crossterm::event::KeyCode;
use operator_console::app::{App, Panel, TradingSection};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

#[test]
fn calculator_section_is_reachable_from_trading_navigation() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);

    while app.active_trading_section() != TradingSection::Calculator {
        app.next_section();
    }

    assert_eq!(app.active_trading_section(), TradingSection::Calculator);
}

#[test]
fn calculator_hotkeys_cycle_bet_type_and_toggle_mode() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Calculator);

    assert_eq!(app.calculator_bet_type().label(), "Normal");
    assert_eq!(app.calculator_mode().label(), "Simple");

    app.handle_key(KeyCode::Char('b'));
    app.handle_key(KeyCode::Char('m'));

    assert_eq!(app.calculator_bet_type().label(), "Free Bet (SNR)");
    assert_eq!(app.calculator_mode().label(), "Advanced");
}

#[test]
fn calculator_fields_are_editable_from_the_app() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Calculator);

    app.handle_key(KeyCode::Enter);
    app.handle_key(KeyCode::Char('2'));
    app.handle_key(KeyCode::Char('0'));
    app.handle_key(KeyCode::Enter);

    let output = app.calculator_output().expect("calculator output");
    assert_eq!(output.standard.lay_stake, 19.17);
}

#[test]
fn calculator_panel_renders_core_sections() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Calculator);

    let backend = TestBackend::new(120, 30);
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

    assert!(rendered.contains("Calculator Summary"));
    assert!(rendered.contains("Inputs"));
    assert!(rendered.contains("Operator Notes"));
    assert!(rendered.contains("Back Stake"));
}
