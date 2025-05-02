use std::rc::Rc;
use std::sync::Mutex;
use std::{collections::HashMap, sync::Arc};

pub mod message_list;
pub mod ui;

use log::info;
use message_list::MessageListState;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::style::Style;
use ratatui::widgets::Block;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use tui_textarea::TextArea;
use whatsrust as wr;

pub fn get_contact_name(contact: &wr::Contact) -> Option<Rc<str>> {
    if !contact.full_name.is_empty() {
        Some(contact.full_name.clone())
    } else if !contact.first_name.is_empty() {
        Some(contact.first_name.clone())
    } else if !contact.push_name.is_empty() {
        Some(format!("~ {}", contact.push_name).into())
    } else if !contact.business_name.is_empty() {
        Some(format!("+ {}", contact.business_name.clone()).into())
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct ChatEntry {
    pub name: Rc<str>,
    pub last_message_time: Option<i64>,
}

pub type ChatList = HashMap<wr::JID, ChatEntry>;

pub fn get_chats() -> ChatList {
    let mut chats = HashMap::new();
    let chat_list = wr::get_all_contacts();

    for (jid, contact) in chat_list {
        if let Some(name) = get_contact_name(&contact) {
            chats.insert(
                jid.clone(),
                ChatEntry {
                    name,
                    last_message_time: None,
                },
            );
        }
    }

    chats
}

pub fn get_sorted_chats(chats: &ChatList) -> Vec<(wr::JID, ChatEntry)> {
    let mut entries: Vec<_> = chats
        .iter()
        .map(|(jid, entry)| (jid.clone(), entry.clone()))
        .collect();
    entries.sort_by(|a, b| {
        let a_time = a.1.last_message_time.unwrap_or_default();
        let b_time = b.1.last_message_time.unwrap_or_default();
        b_time.cmp(&a_time)
    });
    entries
}

pub type ChatMessages = HashMap<wr::MessageId, wr::Message>;

pub type MessagesStorage = HashMap<wr::JID, ChatMessages>;

pub enum FileState {
    None,
    Downloaded,
    Failed,
}

pub struct FileMeta {
    pub state: FileState,
}

pub enum Metadata {
    File(FileMeta),
}

pub type MetadataStorage = HashMap<wr::MessageId, Metadata>;

pub enum AppEvent {
    StateSyncComplete,
    DownloadFile(wr::MessageId, wr::FileId),
}

pub struct App<'a> {
    pub messages: MessagesStorage,
    pub chats: ChatList,
    pub sorted_chats: Vec<(wr::JID, ChatEntry)>,
    pub selected_chat_jid: Option<wr::JID>,
    pub selected_chat_index: Option<usize>,

    pub history_sync_percent: Arc<Mutex<Option<u32>>>,
    pub message_queue: Arc<Mutex<Vec<wr::Message>>>,
    pub event_queue: Arc<Mutex<Vec<AppEvent>>>,

    pub message_list_state: MessageListState,
    pub metadata: MetadataStorage,
    pub image_cache: HashMap<Rc<str>, Protocol>,
    pub picker: Picker,

    pub input_widget: TextArea<'a>,

    pub should_quit: bool,
}

impl Default for App<'_> {
    fn default() -> Self {
        let mut input_widget = TextArea::default();
        input_widget.set_cursor_line_style(Style::default());
        input_widget.set_placeholder_text("Type a message...");
        input_widget.set_block(
            Block::default()
                .title("Input")
                .borders(ratatui::widgets::Borders::ALL),
        );

        Self {
            messages: MessagesStorage::new(),
            chats: ChatList::new(),
            sorted_chats: Vec::new(),
            selected_chat_jid: None,
            selected_chat_index: None,
            message_list_state: MessageListState::default(),
            metadata: MetadataStorage::new(),
            history_sync_percent: Arc::new(Mutex::new(None)),
            message_queue: Arc::new(Mutex::new(Vec::new())),
            event_queue: Arc::new(Mutex::new(Vec::new())),
            image_cache: HashMap::new(),
            picker: Picker::from_query_stdio().unwrap(),
            input_widget,
            should_quit: false,
        }
    }
}

impl App<'_> {
    pub fn tick(&mut self) {
        {
            let mut event_queue = self.event_queue.lock().unwrap();
            while let Some(event) = event_queue.pop() {
                match event {
                    AppEvent::StateSyncComplete => {
                        self.chats = get_chats();
                        self.sorted_chats = get_sorted_chats(&self.chats);
                    }
                    AppEvent::DownloadFile(message_id, file_id) => {
                        let state = match wr::download_file(&file_id) {
                            Ok(_) => FileState::Downloaded,
                            Err(_) => FileState::Failed,
                        };

                        self.metadata
                            .insert(message_id.clone(), Metadata::File(FileMeta { state }));
                    }
                }
            }
        }

        {
            let mut message_queue = self.message_queue.lock().unwrap();
            while let Some(msg) = message_queue.pop() {
                info!("Event puto: {msg:?}");

                let chat = &msg.info.chat;
                let entry = self.chats.iter_mut().find(|(jid, _)| *jid == chat);

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
                self.sorted_chats = get_sorted_chats(&self.chats);
                if let Some(ref current_jid) = self.selected_chat_jid {
                    self.selected_chat_index = self
                        .sorted_chats
                        .iter()
                        .position(|(jid2, _)| jid2 == current_jid);
                }

                if let wr::MessageContent::Image(_) = &msg.message {
                    self.metadata.insert(
                        msg.info.id.clone(),
                        Metadata::File(FileMeta {
                            state: FileState::None,
                        }),
                    );
                }

                self.messages
                    .entry(chat.clone())
                    .or_default()
                    .insert(msg.info.id.clone(), msg);
            }
        }
    }

    pub fn on_event(&mut self, event: Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        self.should_quit = true;
                        return;
                    }
                    KeyCode::Char('n') | KeyCode::Char('p')
                        if key.modifiers == KeyModifiers::CONTROL =>
                    {
                        if let Some(index) = self.selected_chat_index {
                            let mut delta: isize = 0;
                            if key.code == KeyCode::Char('n') {
                                delta = 1;
                            } else if key.code == KeyCode::Char('p') {
                                delta = -1;
                            }
                            let next_index = (index as isize + delta)
                                .rem_euclid(self.sorted_chats.len() as isize)
                                as usize;
                            let next_chat = self.sorted_chats[next_index].0.clone();
                            self.selected_chat_jid = Some(next_chat);
                            self.selected_chat_index = Some(next_index);
                        } else {
                            self.selected_chat_index = Some(0);
                            self.selected_chat_jid = Some(self.sorted_chats[0].0.clone());
                        }
                    }
                    KeyCode::Up => {
                        self.message_list_state.select_next();
                    }
                    KeyCode::Down => {
                        self.message_list_state.select_previous();
                    }
                    KeyCode::Enter | KeyCode::Char('\n') => {
                        if let Some(c) = self.selected_chat_jid.clone() {
                            let msg = self.input_widget.lines().join("\n");
                            wr::send_message(&c, msg.as_str());
                            self.input_widget.select_all();
                            self.input_widget.delete_next_char();
                        }
                        return;
                    }
                    _ => {}
                }
                self.input_widget.input(key);
            }
        }
    }
}
