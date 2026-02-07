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
    let chunks = Layout::vertical([
        Constraint::Length(1),  // Title
        Constraint::Min(5),    // Table
        Constraint::Length(1), // Status bar
        Constraint::Length(2), // Help
    ])
    .split(frame.area());

    draw_title(frame, chunks[0]);
    draw_table(frame, app, chunks[1]);
    draw_status_bar(frame, app, chunks[2]);
    draw_help(frame, app, chunks[3]);
    draw_suggestions(frame, app, chunks[1]);
}

fn draw_title(frame: &mut Frame, area: Rect) {
    let title = Paragraph::new(Line::from(vec![Span::styled(
        " gcloud-switch",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]));
    frame.render_widget(title, area);
}

fn draw_table(frame: &mut Frame, app: &App, area: Rect) {
    if app.profile_names.is_empty() {
        let empty = Paragraph::new("  No profiles. Press 'a' to add one.")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL));
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
        .height(3) // 2 for content + 1 for separator (drawn manually)
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
            let edit_bg = Color::Indexed(17); // dark blue edit background

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

            let row_bg = if is_selected && app.selected_col == Column::Both {
                Color::Indexed(236) // subtle dark gray for the whole row
            } else {
                Color::Reset
            };

            let highlight_bg = Color::Indexed(24);  // dark blue for Both mode
            let col_highlight_bg = Color::Indexed(39); // light blue for column mode

            let profile_bg = if is_selected && app.selected_col == Column::Both {
                highlight_bg
            } else {
                row_bg
            };
            let profile_style = if is_active {
                Style::default().bg(profile_bg).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else if is_selected && app.selected_col == Column::Both {
                Style::default().bg(profile_bg).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(row_bg)
            };

            let user_editing = is_editing && app.edit_col == Column::User;
            let user_selected = is_selected && app.selected_col == Column::User;
            let user_both = is_selected && app.selected_col == Column::Both;
            let user_style = if user_editing {
                Style::default().bg(edit_bg).fg(Color::White)
            } else if user_selected {
                Style::default().bg(col_highlight_bg).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else if user_both {
                Style::default().bg(highlight_bg).fg(Color::White).add_modifier(Modifier::BOLD)
            } else if is_active {
                Style::default().bg(row_bg).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(row_bg)
            };

            let adc_editing = is_editing && app.edit_col == Column::Adc;
            let adc_selected = is_selected && app.selected_col == Column::Adc;
            let adc_both = is_selected && app.selected_col == Column::Both;
            let adc_style = if adc_editing {
                Style::default().bg(edit_bg).fg(Color::White)
            } else if adc_selected {
                Style::default().bg(col_highlight_bg).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else if adc_both {
                Style::default().bg(highlight_bg).fg(Color::White).add_modifier(Modifier::BOLD)
            } else if is_active {
                Style::default().bg(row_bg).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(row_bg)
            };

            let row_style = if is_selected && app.selected_col == Column::Both {
                Style::default().bg(highlight_bg)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(profile_name).style(profile_style),
                Cell::from(user_info).style(user_style),
                Cell::from(adc_info).style(adc_style),
            ])
            .height(2)
            .style(row_style)
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
        .block(Block::default().borders(Borders::ALL))
        .row_highlight_style(Style::default());

    frame.render_widget(table, area);

    // Draw separator line between header and body rows
    let sep_y = area.y + 3; // top border (1) + header content (2)
    let sep_style = Style::default().bg(Color::Reset).fg(Color::Reset);
    if sep_y < area.y + area.height {
        let buf = frame.buffer_mut();
        if let Some(cell) = buf.cell_mut((area.x, sep_y)) {
            cell.set_symbol("├").set_style(sep_style);
        }
        for x in (area.x + 1)..(area.x + area.width - 1) {
            if let Some(cell) = buf.cell_mut((x, sep_y)) {
                cell.set_symbol("─").set_style(sep_style);
            }
        }
        if let Some(cell) = buf.cell_mut((area.x + area.width - 1, sep_y)) {
            cell.set_symbol("┤").set_style(sep_style);
        }
    }

    // Position the terminal cursor for blinking edit cursor
    if matches!(app.input_mode, InputMode::EditAccount | InputMode::EditProject) {
        let inner_w = area.width.saturating_sub(2) as usize;
        // Compute actual column widths (matching the percentage constraints)
        let col_px: Vec<usize> = col_max
            .iter()
            .map(|w| (w * inner_w / total).max(1))
            .collect();

        let col_offset: usize = match app.edit_col {
            Column::User => col_px[0],
            Column::Adc => col_px[0] + col_px[1],
            Column::Both => col_px[0],
        };

        let buf_len = if app.input_mode == InputMode::EditAccount {
            app.edit_account_buffer.len()
        } else {
            app.edit_project_buffer.len()
        };

        let cursor_x = area.x + 1 + col_offset as u16 + buf_len as u16;
        let cursor_y = area.y
            + 1  // top border
            + 3  // header height (2) + separator (1)
            + (app.selected_row as u16) * 2
            + if app.input_mode == InputMode::EditProject { 1 } else { 0 };

        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let sync_label = match app.sync_mode {
        SyncMode::Strict => "sync mode: strict",
        SyncMode::Add => "sync mode: add",
        SyncMode::Off => "sync mode: off",
    };

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
    } else {
        let mut spans = vec![
            Span::styled(
                format!(" {}", sync_label),
                Style::default().fg(Color::DarkGray),
            ),
        ];
        if let Some(ref msg) = app.status_message {
            spans.push(Span::styled(
                format!("  {}", msg),
                Style::default().fg(Color::Green),
            ));
        }
        Line::from(spans)
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
        Span::styled(format!("{} ", desc), Style::default().fg(Color::DarkGray)),
    ]
}

fn draw_help(frame: &mut Frame, app: &App, area: Rect) {
    let spans: Vec<Span> = match app.input_mode {
        InputMode::Normal => {
            let mut s = vec![Span::raw(" ")];
            s.extend(help_key("row:", "\u{2191}\u{2193}"));
            s.extend(help_key("col:", "\u{2190}\u{2192}"));
            s.push(Span::styled("activate all/col:", Style::default().fg(Color::DarkGray)));
            s.push(Span::styled("\u{21b5}  ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
            s.extend(help_key("r", "eauth "));
            s.extend(help_key("e", "dit "));
            s.extend(help_key("a", "dd "));
            s.extend(help_key("d", "el "));
            s.extend(help_key("s", "ync "));
            s.extend(help_key("Esc", " quit"));
            s
        }
        InputMode::ConfirmDelete => {
            let mut s = vec![Span::raw(" ")];
            s.extend(help_key("y", "es "));
            s.extend(help_key("n", "/Esc cancel"));
            s
        }
        InputMode::EditAccount | InputMode::EditProject => {
            let mut s = vec![Span::raw(" ")];
            s.extend(help_key("Tab", " next "));
            s.extend(help_key("\u{2193}", " suggestions "));
            s.extend(help_key("\u{23ce}", " save "));
            s.extend(help_key("Esc", " cancel"));
            s
        }
        _ => {
            let mut s = vec![Span::raw(" ")];
            s.extend(help_key("\u{23ce}", "confirm"));
            s.extend(help_key("Esc", " cancel"));
            s
        }
    };

    let help = Paragraph::new(Line::from(spans));
    frame.render_widget(help, area);
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

    // Inner area (inside table border)
    let inner_x = table_area.x + 1;
    let inner_w = table_area.width.saturating_sub(2);

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

    // Y position: table border (1) + header (2) + rows above * 2 + current row offset
    // If editing account (first line), dropdown appears below first line
    // If editing project (second line), dropdown appears below second line
    let row_y_offset = if app.input_mode == InputMode::EditAccount {
        1 // below the account line
    } else {
        2 // below the project line
    };
    let dropdown_y = table_area.y + 1 + 3 + (app.selected_row as u16) * 2 + row_y_offset;

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
