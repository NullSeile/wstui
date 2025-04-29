use image::DynamicImage;
use list::{WidgetList, WidgetListState};
use log::{error, info};
use ratatui::{
    Frame,
    buffer::Buffer,
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::Line,
    widgets::{Block, Paragraph, StatefulWidget, Widget, Wrap},
};
use ratatui_image::{Image, Resize, picker::Picker, protocol::Protocol};
use wstui::{
    list::{ListDirection, WidgetListItem},
    *,
};

use chrono::DateTime;
use clap::Parser;
use std::{
    collections::HashMap,
    ffi::c_char,
    rc::Rc,
    sync::{Arc, Mutex},
};
use tui_textarea::TextArea;
use whatsrust as wr;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    phone: Option<String>,
}

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
    log::log!(level, "{msg}");
}

fn main() {
    let args = Args::parse();

    let ws_database_path = "examplestore.db";

    let mut messages = MessagesStorage::new();

    let mut chats = ChatList::new();
    let mut sorted_chats = Vec::new();

    let _ = tui_logger::init_logger(tui_logger::LevelFilter::Debug);
    tui_logger::set_default_level(tui_logger::LevelFilter::Debug);

    wr::set_log_handler(log_handler);

    let history_sync_percent = Arc::new(Mutex::new(None));
    let history_sync_percent_clone = Arc::clone(&history_sync_percent);
    wr::set_history_sync_handler(move |percent| {
        let mut history_sync_percent = history_sync_percent_clone.lock().unwrap();
        *history_sync_percent = Some(percent);
    });

    let event_queue = Arc::new(Mutex::new(Vec::<AppEvent>::new()));
    let event_queue_clone = Arc::clone(&event_queue);
    wr::set_state_sync_complete_handler(move || {
        let mut event_queue = event_queue_clone.lock().unwrap();
        event_queue.push(AppEvent::StateSyncComplete);
        info!("State sync complete");
        // chats = get_chats();
        // sorted_chats = get_sorted_chats(&chats);
        // info!("Chats: {:?}", chats);
    });

    let message_queue = Rc::new(Mutex::new(Vec::<wr::Message>::new()));
    let message_queue_clone = Rc::clone(&message_queue);
    wr::set_message_handler(move |message| {
        let mut message_queue = message_queue_clone.lock().unwrap();
        info!("Event: {message:?}");
        message_queue.push(message);
    });

    info!("Starting WhatsRust...");

    wr::new_client(ws_database_path);
    wr::connect(move |data| {
        qr2term::print_qr(data).unwrap();
        if let Some(phone) = args.phone.as_ref() {
            let code = wr::pair_phone(phone);
            println!("Pairing code: {}", code);
        }
    });

    chats = get_chats();
    sorted_chats = get_sorted_chats(&chats);
    wr::add_event_handlers();

    let mut selected_chat_index = None;
    let mut selected_chat_jid = None;

    let mut terminal = ratatui::init();

    let mut messages_state = WidgetListState::new(MessagesState {
        picker: Picker::from_query_stdio().unwrap(),
        image_cache: HashMap::new(),
    });

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
            let mut event_queue = event_queue.lock().unwrap();
            while let Some(event) = event_queue.pop() {
                match event {
                    AppEvent::StateSyncComplete => {
                        chats = get_chats();
                        sorted_chats = get_sorted_chats(&chats);
                    }
                }
            }

            let mut message_queue = message_queue.lock().unwrap();
            while let Some(msg) = message_queue.pop() {
                info!("Event: {msg:?}");

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

                messages
                    .entry(chat.clone())
                    .or_default()
                    .insert(msg.info.id.clone(), msg);
            }
        }

        terminal
            .draw(|frame| {
                let [contacts_area, chat_area, logs_area] = Layout::horizontal([
                    Constraint::Min(30),
                    Constraint::Percentage(50),
                    Constraint::Percentage(50),
                ])
                .areas(frame.area());

                {
                    let percent = history_sync_percent.lock().unwrap();
                    render_contacts(
                        frame,
                        *percent,
                        selected_chat_index,
                        &sorted_chats,
                        contacts_area,
                    );
                }
                render_chat(
                    frame,
                    selected_chat_jid.clone(),
                    &messages,
                    &chats,
                    &mut input_widget,
                    chat_area,
                    &mut messages_state,
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

fn message_height(message: &wr::MessageContent, width: usize) -> usize {
    match message {
        wr::MessageContent::Text(text) => {
            let lines = textwrap::wrap(text, width);
            lines.len() as usize
        }
        wr::MessageContent::Image(_, caption) => {
            let lines = if let Some(caption) = caption {
                textwrap::wrap(caption, width).len()
            } else {
                0
            };
            6 + lines
        }
    }
}

struct MessagesState {
    image_cache: HashMap<Rc<str>, Protocol>,
    picker: Picker,
}

fn render_message(
    message: &wr::MessageContent,
    area: Rect,
    buf: &mut Buffer,
    state: &mut MessagesState,
) {
    match message {
        wr::MessageContent::Text(text) => {
            let lines = textwrap::wrap(text, area.width as usize)
                .iter()
                .map(|line| Line::raw(line.to_string()))
                .collect::<Vec<_>>();
            Paragraph::new(lines).render(area, buf);
        }
        wr::MessageContent::Image(path, caption) => {
            let image_static = state.image_cache.entry(path.clone()).or_insert_with(|| {
                let image_src = image::ImageReader::open(path.to_string())
                    .unwrap()
                    .decode()
                    .unwrap();

                state
                    .picker
                    .new_protocol(
                        image_src,
                        Rect::new(0, 0, area.width, 6),
                        Resize::Scale(None),
                    )
                    .unwrap()
            });

            let [img_area, caption_area] =
                Layout::vertical([Constraint::Length(6), Constraint::Min(0)]).areas(area);

            Image::new(image_static).render(img_area, buf);
            if let Some(caption) = caption {
                let lines = textwrap::wrap(caption, area.width as usize)
                    .iter()
                    .map(|line| Line::raw(line.to_string()))
                    .collect::<Vec<_>>();
                Paragraph::new(lines).render(caption_area, buf);
            }
        }
    };
}

#[derive(Debug, Clone)]
struct MessageWidget {
    msg: wr::Message,
    sender_name: Rc<str>,
    quoted_text: Option<Rc<str>>,
}
impl WidgetListItem for MessageWidget {
    fn height(&self, width: usize) -> usize {
        let header_height = if self.msg.info.quote_id.is_some() {
            2
        } else {
            1
        };
        message_height(&self.msg.message, width) + header_height + 1
    }
}

impl StatefulWidget for MessageWidget {
    type State = MessagesState;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let mut area = area;
        area.height = self.height(area.width as usize) as u16;

        let timestamp =
            if let Some(timestamp) = DateTime::from_timestamp(self.msg.info.timestamp, 0) {
                timestamp.to_rfc2822()
            } else {
                "".to_string()
            }
            .italic();

        let sender_widget = Line::from_iter([
            self.sender_name.to_string().into(),
            " (".into(),
            timestamp,
            ")".into(),
        ])
        .bold();

        let quote_widget = self.msg.info.quote_id.as_ref().map(|_quote_id| {
            let quoted_text = self.quoted_text.unwrap_or_else(|| "not found".into());

            Line::from(format!("> {quoted_text}").gray())
        });

        let [_padding, sender_area, quoted_area, content_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(if quote_widget.is_some() { 1 } else { 0 }),
            Constraint::Min(1),
        ])
        .areas(area);

        sender_widget.render(sender_area, buf);
        if let Some(quoted_widget) = quote_widget {
            quoted_widget.render(quoted_area, buf);
        }
        render_message(&self.msg.message, content_area, buf, state);
    }
}
fn render_chat(
    frame: &mut Frame,
    selected_chat: Option<wr::JID>,
    messages: &MessagesStorage,
    chats: &ChatList,
    input_widget: &mut TextArea,
    area: Rect,
    state: &mut WidgetListState<MessageWidget>,
) {
    let [chat_area, input_area] =
        Layout::vertical([Constraint::Percentage(100), Constraint::Min(1 + 2)]).areas(area);

    if let Some(chat_jid) = selected_chat {
        let contact = chats
            .get(&chat_jid)
            .unwrap_or_else(|| panic!("Contact not found for chat: {chat_jid:?}"));

        let chat_messages_opt = messages.get(&chat_jid);
        let items = chat_messages_opt
            .map(|chat_messages| {
                let mut msgs = chat_messages.values().cloned().collect::<Vec<_>>();
                msgs.sort_by(|a, b| b.info.timestamp.cmp(&a.info.timestamp));

                msgs.iter()
                    .map(|msg| {
                        let sender_name = if let Some(user) = chats.get(&msg.info.sender) {
                            user.name.clone()
                        } else {
                            msg.info.sender.user.clone()
                        };

                        let quoted_text = msg.info.quote_id.as_ref().and_then(|quote_id| {
                            chat_messages.get(quote_id).map(|quoted_msg| {
                                match &quoted_msg.message {
                                    wr::MessageContent::Text(text) => text.clone(),
                                    wr::MessageContent::Image(path, caption) => {
                                        format!("Image: {path} Caption: {caption:?}").into()
                                    }
                                }
                            })
                        });

                        MessageWidget {
                            msg: msg.clone(),
                            sender_name,
                            quoted_text,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // let mut list_state = WidgetListState::default().with_selected(Some(0));
        // let mut list_state = WidgetListState::new(MessagesState {});
        let list = WidgetList::new(items)
            .direction(ListDirection::BottomToTop)
            .block(Block::bordered().title(format!("Chat with {}", contact.name)))
            .highlight_style(ratatui::style::Style::default().fg(ratatui::style::Color::Green));
        frame.render_stateful_widget(list, chat_area, state);
    }

    frame.render_widget(&*input_widget, input_area);
}

#[derive(Default, Clone)]
struct ContactWidget<'a>(Line<'a>);
impl<'a> ContactWidget<'a> {
    pub fn new(text: Line<'a>) -> Self {
        Self(text)
    }
}

impl StatefulWidget for ContactWidget<'_> {
    type State = ();
    fn render(self, area: Rect, buf: &mut Buffer, _state: &mut Self::State) {
        self.0.render(area, buf)
    }
}
impl WidgetListItem for ContactWidget<'_> {
    fn height(&self, _width: usize) -> usize {
        1
    }
}

fn render_contacts(
    frame: &mut Frame,
    percent: Option<u32>,
    selected_index: Option<usize>,
    sorted_chats: &Vec<(&wr::JID, &ChatEntry)>,
    area: Rect,
) {
    let items = sorted_chats
        .iter()
        .map(|entry| ContactWidget::new(entry.1.name.to_string().into()))
        .collect::<Vec<_>>();

    let mut list_state =
        WidgetListState::default().with_selected(Some(selected_index.unwrap_or(0)));
    let list = WidgetList::new(items)
        .block(Block::bordered().title(if let Some(p) = percent {
            format!("Contacts ({p}%)")
        } else {
            "Contacts".to_string()
        }))
        .highlight_style(ratatui::style::Style::default().fg(ratatui::style::Color::Green));
    frame.render_stateful_widget(list, area, &mut list_state);
}
