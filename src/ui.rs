use crate::{App, message_list::render_messages};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::{Block, Borders, List, ListState},
};
use ratatui_image::{Resize, StatefulImage};
use tui_logger::TuiLoggerWidget;

pub fn draw(frame: &mut Frame, app: &mut App) {
    if let Some(img_id) = &app.active_image {
        if let Some(image) = app.image_cache.get_mut(img_id) {
            frame.render_stateful_widget(
                StatefulImage::default().resize(Resize::Scale(None)),
                frame.area(),
                image,
            );
            return;
        }
    }

    let [contacts_area, chat_area, logs_area] = Layout::horizontal([
        Constraint::Min(30),
        Constraint::Percentage(50),
        Constraint::Percentage(50),
    ])
    .areas(frame.area());

    render_logs(frame, logs_area);
    render_contacts(frame, app, contacts_area);
    render_chats(frame, app, chat_area);
}

fn render_logs(frame: &mut Frame, area: Rect) {
    let log_widget =
        TuiLoggerWidget::default().block(Block::default().title("Logs").borders(Borders::ALL));
    frame.render_widget(log_widget, area);
}

fn render_contacts(frame: &mut Frame, app: &mut App, area: Rect) {
    let items = app
        .sorted_chats
        .iter()
        .map(|entry| entry.get_name().to_string())
        .collect::<Vec<_>>();

    let mut list_state = ListState::default().with_selected(app.selected_chat_index);

    let percent = app.history_sync_percent.lock().unwrap();
    let list = List::new(items)
        .block(Block::bordered().title(if let Some(p) = *percent {
            format!("Contacts ({p}%)")
        } else {
            "Contacts".to_string()
        }))
        .highlight_style(ratatui::style::Style::default().fg(ratatui::style::Color::Green));
    frame.render_stateful_widget(list, area, &mut list_state);
}

pub fn render_chats(frame: &mut Frame, app: &mut App, area: Rect) {
    let [chat_area, input_area] =
        Layout::vertical([Constraint::Percentage(100), Constraint::Min(1 + 2)]).areas(area);

    render_messages(frame, app, chat_area);

    if let Some(_chat_jid) = app.selected_chat_jid.clone() {
        frame.render_widget(&app.input_widget, input_area);
    }
}
