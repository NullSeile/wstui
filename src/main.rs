use log::{error, info};
use ratatui::{
    Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout, Rect},
    style::Style,
    widgets::{Block, List, ListState},
};
use wstui::*;

use std::{
    ffi::c_char,
    sync::{Arc, Mutex},
};
use tui_textarea::TextArea;
use whatsrust as wr;

extern "C" fn log_handler(msg: *const c_char, level: u8) {
    let msg = unsafe { std::ffi::CStr::from_ptr(msg) }
        .to_string_lossy()
        .into_owned();
    let level = match level {
        0 => log::Level::Error,
        1 => log::Level::Warn,
        2 => log::Level::Info,
        3 => log::Level::Debug,
        _ => log::Level::Trace,
    };
    log::log!(level, "{}", msg);
}

fn main() {
    let ws_database_path = "examplestore.db";

    let mut messages = MessagesStorage::new();

    let _ = tui_logger::init_logger(tui_logger::LevelFilter::Debug);
    tui_logger::set_default_level(tui_logger::LevelFilter::Debug);

    wr::set_log_handler(log_handler);

    info!("Starting WhatsRust...");

    wr::new_client(ws_database_path);
    wr::connect(|data| qr2term::print_qr(data).unwrap());

    let message_queue = Arc::new(Mutex::new(Vec::<Message>::new()));

    let event_queue_clone = Arc::clone(&message_queue);
    wr::add_event_handler(move |wr::TextMessage(info, text)| {
        let mut event_queue = event_queue_clone.lock().unwrap();
        info!("Event: {:?}", text);

        let message = Message {
            info: info.clone(),
            message: MessageType::TextMessage(text.clone()),
        };

        event_queue.push(message);
    });

    let mut chats = get_chats();
    let mut sorted_chats = get_sorted_chats(&chats);
    let mut selected_chat_index = None;
    let mut selected_chat_jid = None;

    // let mut selected_chat_index = Some(0);
    // let mut selected_chat_jid = sorted_chats
    //     .get(selected_chat_index.unwrap())
    //     .map(|(jid, _)| *jid)
    //     .cloned();

    // let (mut selected_chat_index, mut selected_chat_jid) = chats
    //     .keys()
    //     .enumerate()
    //     .find(|(_, jid)| jid.user == "34693729055")
    //     .map(|(i, jid)| (Some(i), Some(jid.clone())))
    //     .unwrap_or((None, None));

    let mut terminal = ratatui::init();

    let mut input_widget = TextArea::default();
    input_widget.set_cursor_line_style(Style::default());
    input_widget.set_placeholder_text("Type a message...");
    input_widget.set_block(
        Block::default()
            .title("Input")
            .borders(ratatui::widgets::Borders::ALL),
    );

    loop {
        {
            let mut event_queue = message_queue.lock().unwrap();
            while let Some(msg) = event_queue.pop() {
                info!("Event: {:?}", msg);

                let chat = &msg.info.chat;
                let entry = chats.iter_mut().find(|(jid, _)| *jid == chat);

                if let Some((_, entry)) = entry {
                    if let Some(last_message_time) = entry.last_message_time {
                        if msg.info.timestamp > last_message_time {
                            entry.last_message_time = Some(msg.info.timestamp);
                        }
                    } else {
                        entry.last_message_time = Some(msg.info.timestamp);
                    }
                } else {
                    error!("Chat not found: {:?}", chat);
                    // chats.insert(
                    //     chat.clone(),
                    //     ChatEntry {
                    //         name: chat.user.clone(),
                    //         last_message_time: Some(msg.info.timestamp),
                    //     },
                    // );
                }
                sorted_chats = get_sorted_chats(&chats);
                if let Some(ref current_jid) = selected_chat_jid {
                    selected_chat_index = sorted_chats
                        .iter()
                        .position(|(jid2, _)| *jid2 == current_jid);
                }

                messages.entry(chat.clone()).or_default().push(msg);
            }
        }

        terminal
            .draw(|frame| {
                let layout = Layout::horizontal([
                    Constraint::Min(30),
                    Constraint::Percentage(50),
                    Constraint::Percentage(50),
                ])
                .split(frame.area());
                let contacts_area = layout[0];
                let chat_area = layout[1];
                let logs_area = layout[2];
                render_contacts(frame, selected_chat_index, &sorted_chats, contacts_area);
                render_chat(
                    frame,
                    selected_chat_jid.clone(),
                    &messages,
                    &chats,
                    &mut input_widget,
                    chat_area,
                );

                let logs_widget = LogsWidgets {};
                frame.render_widget(logs_widget, logs_area);
            })
            .unwrap();

        if event::poll(std::time::Duration::from_millis(100)).unwrap() {
            if let Event::Key(key) = event::read().unwrap() {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => break,
                        KeyCode::Char('n') | KeyCode::Char('p')
                            if key.modifiers == KeyModifiers::CONTROL =>
                        {
                            if let Some(index) = selected_chat_index {
                                let mut delta: isize = 0;
                                if key.code == KeyCode::Char('n') {
                                    delta = 1;
                                } else if key.code == KeyCode::Char('p') {
                                    delta = -1;
                                }
                                let next_index = (index as isize + delta)
                                    .rem_euclid(sorted_chats.len() as isize)
                                    as usize;
                                let next_chat = sorted_chats[next_index].0.clone();
                                selected_chat_jid = Some(next_chat);
                                selected_chat_index = Some(next_index);
                            } else {
                                selected_chat_index = Some(0);
                                selected_chat_jid = Some(sorted_chats[0].0.clone());
                            }
                        }
                        KeyCode::Enter | KeyCode::Char('\n') => {
                            if let Some(c) = selected_chat_jid.clone() {
                                let msg = input_widget.lines().join("\n");
                                wr::send_message(&c, msg.as_str());
                                input_widget.select_all();
                                input_widget.delete_next_char();
                            }
                            continue;
                        }
                        _ => {}
                    }
                    input_widget.input(key);
                }
            }
        }
    }

    ratatui::restore();
    wr::disconnect();
}

fn render_chat(
    frame: &mut Frame,
    selected_chat: Option<wr::JID>,
    messages: &MessagesStorage,
    chats: &ChatList,
    input_widget: &mut TextArea,
    area: Rect,
) {
    let layout =
        Layout::vertical([Constraint::Percentage(100), Constraint::Min(1 + 2)]).split(area);

    let chat_area = layout[0];
    let input_area = layout[1];

    if let Some(chat_jid) = selected_chat {
        let msgs = messages.get(&chat_jid);

        let items = msgs.map(|msgs| {
            msgs.iter()
                .map(|msg| match msg.message {
                    MessageType::TextMessage(ref text) => {
                        format!("{}: {}", msg.info.sender.user, text)
                    }
                })
                .collect::<Vec<_>>()
        });

        let contact = chats
            .get(&chat_jid)
            .unwrap_or_else(|| panic!("Contact not found for chat: {:?}", chat_jid));

        let mut list_state = ListState::default();
        let list = List::new(items.unwrap_or_default())
            .block(Block::bordered().title(format!("Chat with {}", contact.name)))
            .highlight_style(ratatui::style::Style::default().fg(ratatui::style::Color::Green));
        frame.render_stateful_widget(list, chat_area, &mut list_state);
    }

    frame.render_widget(&*input_widget, input_area);
}

fn render_contacts(
    frame: &mut Frame,
    selected_index: Option<usize>,
    sorted_chats: &Vec<(&wr::JID, &ChatEntry)>,
    area: Rect,
) {
    let items = sorted_chats
        .iter()
        .map(|entry| entry.1.name.to_string())
        .collect::<Vec<_>>();

    let mut list_state = ListState::default().with_selected(Some(selected_index.unwrap_or(0)));
    let list = List::new(items)
        .block(Block::bordered().title("Contacts"))
        .highlight_style(ratatui::style::Style::default().fg(ratatui::style::Color::Green));
    frame.render_stateful_widget(list, area, &mut list_state);
}
