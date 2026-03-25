use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::app_state::CalculatorTool;

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let layout = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(8),
        Constraint::Min(10),
    ])
    .split(area);
    render_tool_tabs(frame, layout[0], app);
    let lower = Layout::horizontal([Constraint::Percentage(44), Constraint::Percentage(56)])
        .split(layout[2]);
    let right = Layout::vertical([Constraint::Length(10), Constraint::Min(10)]).split(lower[1]);

    render_summary(frame, layout[1], app);
    match app.calculator_tool() {
        CalculatorTool::Basic | CalculatorTool::Arb | CalculatorTool::Ev => {
            render_inputs(frame, lower[0], app);
            render_results(frame, right[0], app);
            render_help(frame, right[1], app);
        }
        CalculatorTool::EachWay => render_placeholder(
            frame,
            layout[2],
            "Each-Way",
            "Each-way split staking and place-term modelling is scaffolded. This view will use bookmaker terms plus exchange place markets.",
        ),
        CalculatorTool::Acca => render_placeholder(
            frame,
            layout[2],
            "Acca",
            "Acca modelling is scaffolded. This view will combine multi-leg probability, blended margin, and boosted-return overlays.",
        ),
        CalculatorTool::ExtraPlace => render_placeholder(
            frame,
            layout[2],
            "Extra Place",
            "Extra Place modelling is scaffolded. This view will compare standard terms against enhanced-place offers and layable place books.",
        ),
    }
}

fn render_tool_tabs(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let titles = CalculatorTool::ALL.map(CalculatorTool::label);
    let selected = CalculatorTool::ALL
        .iter()
        .position(|tool| *tool == app.calculator_tool())
        .unwrap_or(0);
    let tabs = Tabs::new(titles.to_vec())
        .select(selected)
        .block(section_block("Calculator Tools", accent_blue()))
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(accent_cyan())
                .add_modifier(Modifier::BOLD),
        )
        .divider("│");
    frame.render_widget(tabs, area);
    register_tab_targets(area, &titles)
        .into_iter()
        .enumerate()
        .for_each(|(index, rect)| {
            app.register_calculator_tool_target(rect, CalculatorTool::ALL[index])
        });
}

fn render_summary(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let output = app.calculator_output().ok();
    let lines = vec![
        Line::from(vec![
            metric("Tool", accent_gold()),
            Span::raw(app.calculator_tool().label()),
            Span::raw("   "),
            metric("Bet Type", accent_blue()),
            Span::raw(app.calculator_bet_type().label()),
            Span::raw("   "),
            metric("Mode", accent_cyan()),
            Span::raw(app.calculator_mode().label()),
        ]),
        Line::from(vec![
            metric("Selected", accent_gold()),
            Span::raw(app.calculator_selected_field().label()),
            Span::raw("   "),
            metric("Editing", accent_pink()),
            Span::raw(if app.calculator_is_editing() {
                "yes"
            } else {
                "no"
            }),
        ]),
        Line::from(vec![
            metric("Qualifying", accent_green()),
            Span::raw(
                output
                    .as_ref()
                    .map(|output| format_currency(output.qualifying_profit))
                    .unwrap_or_else(|| String::from("-")),
            ),
            Span::raw("   "),
            metric("Rating", accent_green()),
            Span::raw(
                output
                    .as_ref()
                    .map(|output| format!("{:.2}%", output.rating_pct))
                    .unwrap_or_else(|| String::from("-")),
            ),
        ]),
        Line::from(vec![
            metric("Event", accent_blue()),
            Span::raw(
                app.calculator_source()
                    .map(|source| source.event_name.clone())
                    .unwrap_or_else(|| String::from("-")),
            ),
        ]),
        Line::from(vec![
            metric("Selection", accent_cyan()),
            Span::raw(
                app.calculator_source()
                    .map(|source| {
                        format!(
                            "{} | {} | {:.2}",
                            source.selection_name, source.competition_name, source.rating
                        )
                    })
                    .unwrap_or_else(|| String::from("-")),
            ),
        ]),
    ];

    let body = Paragraph::new(lines)
        .block(section_block("Calculator Summary", accent_blue()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_inputs(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let rows = app
        .calculator_field_rows()
        .into_iter()
        .map(|(field, value, selected)| {
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(accent_cyan())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(text_color())
            };
            Row::new(vec![
                Cell::from(field.label().to_string()),
                Cell::from(value),
                Cell::from(if selected { "<<" } else { "" }),
            ])
            .style(style)
        });
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(46),
            Constraint::Percentage(38),
            Constraint::Length(4),
        ],
    )
    .block(section_block("Inputs", accent_blue()))
    .column_spacing(1);
    frame.render_widget(table, area);
}

fn render_results(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let output = app.calculator_output();
    let Some(output) = output.as_ref().ok() else {
        let body = Paragraph::new("Calculator inputs are invalid.")
            .block(section_block("Results", accent_red()))
            .wrap(Wrap { trim: true });
        frame.render_widget(body, area);
        return;
    };

    let title = match app.calculator_tool() {
        CalculatorTool::Basic => "Basic Results",
        CalculatorTool::Arb => "Arb Results",
        CalculatorTool::Ev => "EV Lens",
        _ => "Results",
    };
    let mut rows = vec![
        result_line(
            "Standard",
            &output.standard,
            accent_green(),
            output.qualifying_profit,
        ),
        result_line(
            "Underlay",
            &output.underlay,
            accent_gold(),
            output.qualifying_profit,
        ),
        result_line(
            "Overlay",
            &output.overlay,
            accent_pink(),
            output.qualifying_profit,
        ),
    ];
    if app.calculator_tool() == CalculatorTool::Ev {
        rows.push(Line::raw(format!(
            "Source EV: {}",
            app.calculator_source()
                .map(|source| format!("{:.2}% rating", source.rating))
                .unwrap_or_else(|| String::from("-"))
        )));
    }

    let body = Paragraph::new(rows)
        .block(section_block(title, accent_green()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_help(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let output = app.calculator_output();
    let mut lines = vec![
        Line::raw("Up/Down select field"),
        Line::raw("Enter edit/apply field"),
        Line::raw("Esc cancel field edit"),
        Line::raw("b cycle bet type"),
        Line::raw("m toggle simple/advanced"),
        Line::raw("Tab cycle calculator tool"),
        Line::raw(String::new()),
    ];
    if let Some(source) = app.calculator_source() {
        lines.push(Line::raw(format!(
            "Seeded from {} -> {} / {}",
            source.event_name, source.bookmaker_name, source.exchange_name
        )));
    }
    match output {
        Ok(output) => {
            lines.push(Line::raw(format!(
                "Retained free-bet value: {}",
                format_currency(output.retained_risk_free_value)
            )));
            lines.push(Line::raw(format!(
                "Std lay {} | liability {}",
                format_currency(output.standard.lay_stake),
                format_currency(output.standard.liability)
            )));
        }
        Err(error) => lines.push(Line::raw(format!("Error: {error}"))),
    }

    let body = Paragraph::new(lines)
        .block(section_block("Operator Notes", accent_pink()))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_placeholder(frame: &mut Frame<'_>, area: Rect, title: &str, detail: &str) {
    let body = Paragraph::new(detail)
        .block(
            Block::default()
                .title(Span::styled(
                    title,
                    Style::default()
                        .fg(accent_gold())
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .style(Style::default().bg(panel_background()).fg(text_color()))
                .border_style(Style::default().fg(border_color())),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn register_tab_targets(area: Rect, titles: &[&str]) -> Vec<Rect> {
    let mut targets = Vec::new();
    let mut x = area.x.saturating_add(1);
    let y = area.y.saturating_add(1);
    for title in titles {
        let width = title.len() as u16;
        targets.push(Rect {
            x,
            y,
            width,
            height: 1,
        });
        x = x.saturating_add(width).saturating_add(2);
    }
    targets
}

fn result_line(
    label: &str,
    scenario: &crate::calculator::Scenario,
    accent: Color,
    qualifying_profit: f64,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "lay {} | liab {} | back {} | lay {} | q {}",
            format_currency(scenario.lay_stake),
            format_currency(scenario.liability),
            format_signed_currency(scenario.profit_if_back_wins),
            format_signed_currency(scenario.profit_if_lay_wins),
            format_signed_currency(qualifying_profit),
        )),
    ])
}

fn metric(label: &'static str, accent: Color) -> Span<'static> {
    Span::styled(
        format!("{label}: "),
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    )
}

fn section_block(title: &'static str, accent: Color) -> Block<'static> {
    Block::default()
        .title(Span::styled(
            title,
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(Style::default().bg(panel_background()).fg(text_color()))
        .border_style(Style::default().fg(border_color()))
}

fn format_currency(value: f64) -> String {
    format!("£{value:.2}")
}

fn format_signed_currency(value: f64) -> String {
    if value >= 0.0 {
        format!("+£{value:.2}")
    } else {
        format!("-£{:.2}", value.abs())
    }
}

fn panel_background() -> Color {
    Color::Rgb(16, 22, 30)
}

fn text_color() -> Color {
    Color::Rgb(234, 240, 246)
}

fn border_color() -> Color {
    Color::Rgb(74, 88, 104)
}

fn accent_blue() -> Color {
    Color::Rgb(109, 180, 255)
}

fn accent_cyan() -> Color {
    Color::Rgb(94, 234, 212)
}

fn accent_green() -> Color {
    Color::Rgb(134, 239, 172)
}

fn accent_gold() -> Color {
    Color::Rgb(248, 208, 119)
}

fn accent_pink() -> Color {
    Color::Rgb(244, 143, 177)
}

fn accent_red() -> Color {
    Color::Rgb(248, 113, 113)
}
