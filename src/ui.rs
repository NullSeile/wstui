use crate::{
    App, SelectedWidget,
    message_list::{get_quoted_text, render_messages},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    widgets::{Block, Borders, List, ListState, Paragraph},
};
use ratatui_image::{Resize, StatefulImage};
use tui_logger::TuiLoggerWidget;
use whatsrust as wr;

pub fn draw(frame: &mut Frame, app: &mut App) {
    if let SelectedWidget::MessageView = app.selected_widget {
        let msg_id = app.message_list_state.selected_message.clone().unwrap();
        let chat_id = app.selected_chat_jid.clone().unwrap();

        let block = Block::default()
            .title("Message")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(
                if let SelectedWidget::MessageView = app.selected_widget {
                    ratatui::style::Color::Green
                } else {
                    ratatui::style::Color::White
                },
            ));

        let area = block.inner(frame.area());
        frame.render_widget(block, frame.area());

        if let Some(msg) = app.messages.get(&chat_id).and_then(|m| m.get(&msg_id)) {
            match msg.message {
                wr::MessageContent::Image(ref image) => {
                    if let Some(image) = app.image_cache.get_mut(&image.path) {
                        frame.render_stateful_widget(
                            StatefulImage::default().resize(Resize::Scale(None)),
                            area,
                            image,
                        );
                    }
                }
                wr::MessageContent::Text(ref text) => {
                    let paragraph = Paragraph::new(text.to_string());
                    frame.render_widget(paragraph, area);
                }
            }
        }

        return;
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

    let list = List::new(items)
        .block(
            Block::bordered()
                .title(if let Some(p) = app.history_sync_percent {
                    format!("Contacts ({p}%)")
                } else {
                    "Contacts".to_string()
                })
                .border_style(Style::default().fg(
                    if let SelectedWidget::ChatList = app.selected_widget {
                        ratatui::style::Color::Green
                    } else {
                        ratatui::style::Color::White
                    },
                )),
        )
        .highlight_style(ratatui::style::Style::default().fg(ratatui::style::Color::Green));
    frame.render_stateful_widget(list, area, &mut list_state);
}

pub fn render_chats(frame: &mut Frame, app: &mut App, area: Rect) {
    let [chat_area, mut input_area] =
        Layout::vertical([Constraint::Percentage(100), Constraint::Min(10)]).areas(area);

    render_messages(frame, app, chat_area);

    if let Some(_chat_jid) = app.selected_chat_jid.clone() {
        // let input_block = Block::default().title("Input").borders(Borders::ALL);
        let input_block = app.input_border.clone().border_style(Style::default().fg(
            if let SelectedWidget::Input = app.selected_widget {
                ratatui::style::Color::Green
            } else {
                ratatui::style::Color::White
            },
        ));
        frame.render_widget(&input_block, input_area);

        input_area = input_block.inner(input_area);

        if let Some(msg) = &app.quoting_message {
            let [quote_area, input_areaa] =
                Layout::vertical([Constraint::Length(1), Constraint::Percentage(100)])
                    .areas(input_area);

            input_area = input_areaa;

            frame.render_widget(
                Paragraph::new(format!("> {}", get_quoted_text(msg))).dark_gray(),
                quote_area,
            );
        }

        frame.render_widget(&app.input_widget, input_area);
    }
}
