use operator_console::app::App;

#[test]
fn help_text_mentions_core_operator_keys() {
    let app = App::default();
    let help = app.help_text();

    assert!(help.contains("q"));
    assert!(help.contains("tab"));
    assert!(help.contains("r"));
    assert!(help.contains("s"));
    assert!(help.contains("x"));
}
