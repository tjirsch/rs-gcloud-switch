use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Table,
    },
    Frame,
};

use crate::app::{App, Column, InputMode};
use crate::profile::SyncMode;

pub fn draw(frame: &mut Frame, app: &App) {
    let frame_area = frame.area();

    // Always use normal mode help line width for stable layout
    let normal_help_width = build_normal_help_width(app);

    // Build the actual help line for the current mode
    let help_line = build_help_line(app);

    // Calculate table content width
    let table_width = table_content_width(app);

    // Minimum width: max of normal help and table, capped at terminal width
    let content_width = (normal_help_width.max(table_width) as u16).min(frame_area.width);

    // Table height
    let table_h: u16 = if app.profile_names.is_empty() {
        1
    } else {
        2 + (app.profile_names.len() as u16) * 2
    };

    // Total content height: table + status bar + help
    let total_h = table_h + 2;

    // Center horizontally; center vertically if content fits
    let x = (frame_area.width.saturating_sub(content_width)) / 2;
    let (y, height, table_constraint) = if total_h <= frame_area.height {
        let y = (frame_area.height - total_h) / 2;
        (y, total_h, Constraint::Length(table_h))
    } else {
        (0, frame_area.height, Constraint::Min(3))
    };

    let centered = Rect {
        x,
        y,
        width: content_width,
        height,
    };

    let chunks = Layout::vertical([
        table_constraint,
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(centered);

    draw_table(frame, app, chunks[0]);
    draw_status_bar(frame, app, chunks[1]);
    frame.render_widget(Paragraph::new(help_line), chunks[2]);
    draw_suggestions(frame, app, chunks[0]);
}

fn table_content_width(app: &App) -> usize {
    if app.profile_names.is_empty() {
        return 36;
    }
    let header_labels: [(&str, &str); 3] = [
        ("Profile", ""),
        ("User Account", "Project"),
        ("ADC Account", "Quota Project"),
    ];
    let mut col_max = [0usize; 3];
    for (i, (line1, line2)) in header_labels.iter().enumerate() {
        col_max[i] = col_max[i].max(line1.len()).max(line2.len());
    }
    for (name, profile) in app.profile_names.iter().zip(app.profiles.iter()) {
        col_max[0] = col_max[0].max(name.len());
        col_max[1] = col_max[1]
            .max(profile.user_account.len() + 3)
            .max(profile.user_project.len());
        col_max[2] = col_max[2]
            .max(profile.adc_account.len() + 3)
            .max(profile.adc_quota_project.len());
    }
    col_max.iter().sum::<usize>() + 4
}

fn draw_table(frame: &mut Frame, app: &App, area: Rect) {
    if app.profile_names.is_empty() {
        let empty = Paragraph::new("  No profiles. Press 'n' to add one.")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(empty, area);
        return;
    }

    let header_labels: [(&str, &str); 3] = [
        ("Profile", ""),
        ("User Account", "Project"),
        ("ADC Account", "Quota Project"),
    ];
    let header_cells = header_labels.iter().map(|(line1, line2)| {
        let style = Style::default()
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD);
        if line2.is_empty() {
            Cell::from(*line1).style(style)
        } else {
            Cell::from(format!("{}\n{}", line1, line2)).style(style)
        }
    });
    let header = Row::new(header_cells)
        .height(2)
        .style(Style::default().bg(Color::Indexed(254)));

    let rows = app
        .profile_names
        .iter()
        .zip(app.profiles.iter())
        .enumerate()
        .map(|(i, (name, profile))| {
            let is_active = app.active_profile.as_deref() == Some(name.as_str());
            let is_selected = i == app.selected_row;
            let profile_name = name.to_string();

            let is_editing = i == app.selected_row
                && matches!(app.input_mode, InputMode::EditAccount | InputMode::EditProject);

            let user_auth_status = app.user_auth_valid.get(i).copied().flatten();
            let user_lock = match user_auth_status {
                Some(true) => " \u{1F511}",
                Some(false) => " \u{1F512}",
                None => "",
            };
            let user_info = if is_editing && app.edit_col == Column::User {
                format!("{}\n{}", app.edit_account_buffer, app.edit_project_buffer)
            } else {
                format!("{}{}\n{}", profile.user_account, user_lock, profile.user_project)
            };

            let adc_auth_status = app.adc_auth_valid.get(i).copied().flatten();
            let adc_lock = match adc_auth_status {
                Some(true) => " \u{1F511}",
                Some(false) => " \u{1F512}",
                None => "",
            };
            let adc_info = if is_editing && app.edit_col == Column::Adc {
                format!("{}\n{}", app.edit_account_buffer, app.edit_project_buffer)
            } else {
                format!("{}{}\n{}", profile.adc_account, adc_lock, profile.adc_quota_project)
            };

            let light_grey       = Color::Indexed(255);
            let highlight_bg     = Color::Blue ;
            let col_highlight_bg = Color::Indexed(75); // lighter blue for selected column
            let edit_bg          = Color::Indexed(255); // Light Grey edit background

            let base_style = if is_selected {
                Style::default().bg(highlight_bg).fg(Color::White)
            } else if is_active {
                Style::default().bg(light_grey).fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(light_grey).fg(Color::Black)
            };

            let col_style = |col: Column, editing: bool| -> Style {
                if editing {
                    Style::default().bg(edit_bg).fg(Color::Black)
                } else if is_selected && app.selected_col == col {
                    Style::default().bg(col_highlight_bg).fg(Color::White).add_modifier(Modifier::BOLD)
                } else {
                    base_style
                }
            };

            let profile_style = base_style;
            let user_style    = col_style(Column::User, is_editing && app.edit_col == Column::User);
            let adc_style     = col_style(Column::Adc,  is_editing && app.edit_col == Column::Adc);

            Row::new(vec![
                Cell::from(profile_name).style(profile_style),
                Cell::from(user_info   ).style(user_style   ),
                Cell::from(adc_info    ).style(adc_style    ),
            ])
            .height(2).style(base_style)
        });

    // Calculate max content width per column
    let mut col_max = [0usize; 3];
    // Header widths
    for (i, (line1, line2)) in header_labels.iter().enumerate() {
        col_max[i] = col_max[i].max(line1.len()).max(line2.len());
    }
    // Data widths
    for (name, profile) in app.profile_names.iter().zip(app.profiles.iter()) {
        let profile_w = name.len();
        col_max[0] = col_max[0].max(profile_w);
        col_max[1] = col_max[1]
            .max(profile.user_account.len())
            .max(profile.user_project.len());
        col_max[2] = col_max[2]
            .max(profile.adc_account.len())
            .max(profile.adc_quota_project.len());
    }
    let total: usize = col_max.iter().sum::<usize>().max(1);
    let widths = col_max.map(|w| {
        Constraint::Percentage((w as u16 * 100 / total as u16).max(1))
    });

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(0)
        .row_highlight_style(Style::default());

    frame.render_widget(table, area);

    // Position the terminal cursor for blinking edit cursor
    if matches!(app.input_mode, InputMode::EditAccount | InputMode::EditProject) {
        let inner_w = area.width as usize;
        // Compute actual column widths (matching the percentage constraints)
        let col_px: Vec<usize> = col_max
            .iter()
            .map(|w| (w * inner_w / total).max(1))
            .collect();

        let col_offset: usize = match app.edit_col {
            Column::User => col_px[0],
            Column::Adc  => col_px[0] + col_px[1],
            Column::Both => col_px[0],
        };

        let buf_len = if app.input_mode == InputMode::EditAccount {
            app.edit_account_buffer.len()
        } else {
            app.edit_project_buffer.len()
        };

        let cursor_x = area.x + col_offset as u16 + buf_len as u16;
        let cursor_y = area.y
            + 2  // header height
            + (app.selected_row as u16) * 2
            + if app.input_mode == InputMode::EditProject { 1 } else { 0 };

        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let is_input_mode = matches!(
        app.input_mode,
        InputMode::AddProfileName
            | InputMode::AddProfileUserAccount
            | InputMode::AddProfileUserProject
            | InputMode::AddProfileAdcAccount
            | InputMode::AddProfileAdcQuotaProject
    );

    let line = if is_input_mode {
        let prompt = app.status_message.as_deref().unwrap_or("Input:");
        Line::from(vec![
            Span::styled(
                format!(" {} ", prompt),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(
                app.input_buffer.as_str().to_string(),
                Style::default().fg(Color::White),
            ),
            Span::styled("_", Style::default().fg(Color::Gray)),
        ])
    } else if let Some(ref msg) = app.status_message {
        Line::from(vec![
            Span::styled(
                format!(" {}", msg),
                Style::default().fg(Color::Green),
            ),
        ])
    } else {
        Line::default()
    };

    let bar = Paragraph::new(line);
    frame.render_widget(bar, area);
}

fn help_key(key: &str, desc: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            key.to_string(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{}", desc), Style::default().fg(Color::DarkGray)),
    ]
}

fn title_prefix() -> Vec<Span<'static>> {
    vec![
        Span::styled(
            "gcloud-switch",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" v{}", env!("CARGO_PKG_VERSION")),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("  "),
    ]
}

fn build_normal_help_spans(app: &App) -> Vec<Span<'static>> {
    let mut s = title_prefix();
    s.extend(help_key("row:", "\u{2191}\u{2193} "));
    s.extend(help_key("col:", "\u{2190}\u{2192} "));
    let activate_label = match app.selected_col {
        Column::Both => "activate both:",
        Column::User => "activate user:",
        Column::Adc  => "activate adc: ",
    };
    s.push(Span::styled(activate_label.to_string(), Style::default().fg(Color::DarkGray)));
    s.push(Span::styled("\u{21b5} ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
    s.extend(help_key("a", "uthenticate "));
    s.extend(help_key("e", "dit "));
    s.extend(help_key("n", "ew "));
    s.extend(help_key("d", "el "));
    s.extend(help_key("s", "ync"));
    let sync_mode_label = match app.sync_mode {
        SyncMode::Strict => "(both)",
        SyncMode::Add => "(add)",
        SyncMode::Off => "(off)",
    };
    s.push(Span::styled(
        format!("{} ", sync_mode_label),
        Style::default().fg(Color::DarkGray),
    ));
    s.extend(help_key("i", "mport "));
    s.extend(help_key("esc", " exit"));
    s
}

fn build_normal_help_width(app: &App) -> usize {
    Line::from(build_normal_help_spans(app)).width()
}

fn build_help_line(app: &App) -> Line<'static> {
    let spans: Vec<Span> = match app.input_mode {
        InputMode::Normal => build_normal_help_spans(app),
        InputMode::ConfirmDelete => {
            let mut s = title_prefix();
            s.extend(help_key("y", "es "));
            s.extend(help_key("n", "/Esc cancel"));
            s
        }
        InputMode::EditAccount | InputMode::EditProject => {
            let mut s = title_prefix();
            s.extend(help_key("Tab", " next "));
            s.extend(help_key("\u{2193}", " suggestions "));
            s.extend(help_key("\u{23ce}", " save "));
            s.extend(help_key("Esc", " cancel"));
            s
        }
        _ => {
            let mut s = title_prefix();
            s.extend(help_key("\u{23ce}", "confirm"));
            s.extend(help_key("Esc", " cancel"));
            s
        }
    };
    Line::from(spans)
}

fn draw_suggestions(frame: &mut Frame, app: &App, table_area: Rect) {
    if app.suggestion_index.is_none() || app.suggestions.is_empty() {
        return;
    }

    let selected_idx = app.suggestion_index.unwrap_or(0);

    // Replicate column width calculation to find dropdown x position
    let header_labels: [(&str, &str); 3] = [
        ("Profile", ""),
        ("User Account", "Project"),
        ("ADC Account", "Quota Project"),
    ];
    let mut col_max = [0usize; 3];
    for (i, (line1, line2)) in header_labels.iter().enumerate() {
        col_max[i] = col_max[i].max(line1.len()).max(line2.len());
    }
    for (name, profile) in app.profile_names.iter().zip(app.profiles.iter()) {
        let profile_w = name.len() + 4;
        col_max[0] = col_max[0].max(profile_w);
        col_max[1] = col_max[1]
            .max(profile.user_account.len())
            .max(profile.user_project.len());
        col_max[2] = col_max[2]
            .max(profile.adc_account.len())
            .max(profile.adc_quota_project.len());
    }
    let total: usize = col_max.iter().sum::<usize>().max(1);

    let inner_x = table_area.x;
    let inner_w = table_area.width;

    // Column pixel widths (proportional)
    let col_widths: Vec<u16> = col_max
        .iter()
        .map(|w| ((*w as u16) * inner_w / total as u16).max(1))
        .collect();

    // X position based on which column is being edited
    let dropdown_x = match app.edit_col {
        Column::User => inner_x + col_widths[0],
        Column::Adc => inner_x + col_widths[0] + col_widths[1],
        Column::Both => inner_x + col_widths[0],
    };

    // Y position: header (2) + rows above * 2 + current row offset
    let row_y_offset = if app.input_mode == InputMode::EditAccount {
        1 // below the account line
    } else {
        2 // below the project line
    };
    let dropdown_y = table_area.y + 2 + (app.selected_row as u16) * 2 + row_y_offset;

    // Dropdown dimensions
    let max_item_width = app
        .suggestions
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(20) as u16;
    let dropdown_w = (max_item_width + 4).min(50).max(20);
    let dropdown_h = (app.suggestions.len() as u16 + 2).min(12); // +2 for borders

    // Clamp to screen bounds
    let frame_area = frame.area();
    let dropdown_x = dropdown_x.min(frame_area.width.saturating_sub(dropdown_w));
    let dropdown_y = dropdown_y.min(frame_area.height.saturating_sub(dropdown_h));

    let dropdown_area = Rect {
        x: dropdown_x,
        y: dropdown_y,
        width: dropdown_w,
        height: dropdown_h,
    };

    // Clear the area behind the popup
    frame.render_widget(Clear, dropdown_area);

    let items: Vec<ListItem> = app
        .suggestions
        .iter()
        .enumerate()
        .map(|(i, suggestion)| {
            let style = if i == selected_idx {
                Style::default()
                    .bg(Color::Indexed(24))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(suggestion.as_str()).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    let mut list_state = ListState::default().with_selected(Some(selected_idx));
    frame.render_stateful_widget(list, dropdown_area, &mut list_state);

    // Scrollbar (only if items overflow the visible area)
    let visible_items = dropdown_area.height.saturating_sub(2) as usize; // minus borders
    if app.suggestions.len() > visible_items {
        let mut scrollbar_state = ScrollbarState::new(app.suggestions.len().saturating_sub(visible_items))
            .position(selected_idx.saturating_sub(visible_items / 2).min(app.suggestions.len().saturating_sub(visible_items)));
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_stateful_widget(
            scrollbar,
            dropdown_area.inner(ratatui::layout::Margin { horizontal: 0, vertical: 1 }),
            &mut scrollbar_state,
        );
    }
}
