use operator_console::app::{App, Panel};

#[test]
fn tab_navigation_cycles_through_recorder_panel() {
    let mut app = App::default();

    assert_eq!(app.active_panel(), Panel::Dashboard);

    app.next_panel();
    assert_eq!(app.active_panel(), Panel::Exchanges);

    app.next_panel();
    assert_eq!(app.active_panel(), Panel::Recorder);

    app.previous_panel();
    assert_eq!(app.active_panel(), Panel::Exchanges);
}
