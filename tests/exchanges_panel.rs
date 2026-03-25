use operator_console::app::{App, Panel, TradingSection};

#[test]
fn exchange_selection_state_tracks_selected_row() {
    let mut app = App::default();
    app.set_active_panel(Panel::Trading);
    app.set_trading_section(TradingSection::Positions);

    assert_eq!(app.selected_exchange_row(), None);

    app.select_next_exchange_row();
    assert_eq!(app.selected_exchange_row(), Some(0));
}
