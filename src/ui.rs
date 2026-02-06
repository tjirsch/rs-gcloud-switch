use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table},
    Frame,
};

use crate::app::{App, Column, InputMode};

pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(1),  // Title
        Constraint::Min(5),    // Table
        Constraint::Length(1), // Status / input
        Constraint::Length(2), // Help
    ])
    .split(frame.area());

    draw_title(frame, chunks[0]);
    draw_table(frame, app, chunks[1]);
    draw_status(frame, app, chunks[2]);
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
            let is_auth_valid = app.auth_valid.get(i).copied().unwrap_or(false);

            let lock_icon = if is_auth_valid { "\u{2705} " } else { "\u{1F512} " };
            let prefix = if is_active { "* " } else { "" };
            let profile_name = format!("{}{}{}", lock_icon, prefix, name);

            let is_editing = i == app.selected_row
                && matches!(app.input_mode, InputMode::EditAccount | InputMode::EditProject);
            let edit_bg = Color::Indexed(17); // dark blue edit background

            let user_info = if is_editing && app.edit_col == Column::User {
                let acct = if app.input_mode == InputMode::EditAccount {
                    format!("{}\u{2588}", app.edit_account_buffer)
                } else {
                    app.edit_account_buffer.clone()
                };
                let proj = if app.input_mode == InputMode::EditProject {
                    format!("{}\u{2588}", app.edit_project_buffer)
                } else {
                    app.edit_project_buffer.clone()
                };
                format!("{}\n{}", acct, proj)
            } else {
                format!("{}\n{}", profile.user_account, profile.user_project)
            };

            let adc_info = if is_editing && app.edit_col == Column::Adc {
                let acct = if app.input_mode == InputMode::EditAccount {
                    format!("{}\u{2588}", app.edit_account_buffer)
                } else {
                    app.edit_account_buffer.clone()
                };
                let proj = if app.input_mode == InputMode::EditProject {
                    format!("{}\u{2588}", app.edit_project_buffer)
                } else {
                    app.edit_project_buffer.clone()
                };
                format!("{}\n{}", acct, proj)
            } else {
                format!("{}\n{}", profile.adc_account, profile.adc_quota_project)
            };

            let row_bg = if is_selected {
                Color::Indexed(236) // subtle dark gray for the whole row
            } else {
                Color::Reset
            };

            let highlight_bg = Color::Indexed(24);

            let profile_bg = if is_selected && app.selected_col == Column::Both {
                highlight_bg
            } else {
                row_bg
            };
            let profile_style = if is_active {
                Style::default().bg(profile_bg).fg(Color::Green).add_modifier(Modifier::BOLD)
            } else if is_selected && app.selected_col == Column::Both {
                Style::default().bg(profile_bg).fg(Color::White).add_modifier(Modifier::BOLD)
            } else if !is_auth_valid {
                Style::default().bg(row_bg).fg(Color::DarkGray)
            } else {
                Style::default().bg(row_bg)
            };

            let user_editing = is_editing && app.edit_col == Column::User;
            let user_selected = is_selected
                && (app.selected_col == Column::User || app.selected_col == Column::Both);
            let user_style = if user_editing {
                Style::default().bg(edit_bg).fg(Color::White)
            } else if user_selected {
                Style::default().bg(highlight_bg).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(row_bg)
            };

            let adc_editing = is_editing && app.edit_col == Column::Adc;
            let adc_selected = is_selected
                && (app.selected_col == Column::Adc || app.selected_col == Column::Both);
            let adc_style = if adc_editing {
                Style::default().bg(edit_bg).fg(Color::White)
            } else if adc_selected {
                Style::default().bg(highlight_bg).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(row_bg)
            };

            let row_style = if is_selected {
                match app.selected_col {
                    Column::Both => Style::default().bg(highlight_bg),
                    _ => Style::default().bg(row_bg),
                }
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
        let profile_w = name.len() + 4; // lock icon + "* " prefix
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
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let content = match &app.input_mode {
        InputMode::Normal | InputMode::EditAccount | InputMode::EditProject => {
            if let Some(ref msg) = app.status_message {
                Line::from(Span::styled(
                    format!(" {}", msg),
                    Style::default().fg(Color::Green),
                ))
            } else {
                Line::from("")
            }
        }
        InputMode::ConfirmDelete => {
            if let Some(ref msg) = app.status_message {
                Line::from(Span::styled(
                    format!(" {}", msg),
                    Style::default().fg(Color::Red),
                ))
            } else {
                Line::from("")
            }
        }
        _ => {
            let prompt = app.status_message.as_deref().unwrap_or("Input:");
            Line::from(vec![
                Span::styled(
                    format!(" {} ", prompt),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    &app.input_buffer,
                    Style::default().fg(Color::White),
                ),
                Span::styled("_", Style::default().fg(Color::Gray)),
            ])
        }
    };

    let paragraph = Paragraph::new(content);
    frame.render_widget(paragraph, area);
}

fn draw_help(frame: &mut Frame, app: &App, area: Rect) {
    let help_text = match app.input_mode {
        InputMode::Normal => {
            " \u{2191}\u{2193}/jk navigate  \u{2190}\u{2192}/hl column  Enter activate+quit  Alt+Enter activate+stay  r reauth  e edit  q quit  a add  d delete"
        }
        InputMode::ConfirmDelete => " y confirm  n/Esc cancel",
        InputMode::EditAccount | InputMode::EditProject => {
            " Tab next field  \u{2193} suggestions  Enter save  Esc cancel"
        }
        _ => " Enter confirm  Esc cancel",
    };

    let help = Paragraph::new(Line::from(Span::styled(
        help_text,
        Style::default().fg(Color::DarkGray),
    )));
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
    let dropdown_y = table_area.y + 1 + 2 + (app.selected_row as u16) * 2 + row_y_offset;

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

    frame.render_widget(list, dropdown_area);
}
