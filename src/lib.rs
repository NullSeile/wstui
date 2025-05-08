use std::sync::Mutex;
use std::{collections::HashMap, sync::Arc};

pub mod db;
pub mod message_list;
pub mod ui;

use db::DatabaseHandler;
use log::{error, info};
use message_list::MessageListState;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::style::Style;
use ratatui::widgets::Block;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use tui_textarea::TextArea;
use whatsrust as wr;

pub fn get_contact_name(contact: &wr::Contact) -> Option<Arc<str>> {
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

// Each chat has a name (can be Some(str) or None, in which case take the jid)

#[derive(Clone, Debug)]
pub struct Chat {
    pub jid: wr::JID,
    pub name: Option<Arc<str>>,
    pub last_message_time: Option<i64>,
}

impl Chat {
    pub fn get_name(&self) -> Arc<str> {
        if let Some(name) = &self.name {
            name.clone()
        } else {
            self.jid.clone().into()
        }
    }
}

pub type ChatList = HashMap<wr::JID, Chat>;

pub type ChatMessages = HashMap<wr::MessageId, wr::Message>;

pub type MessagesStorage = HashMap<wr::JID, ChatMessages>;

pub enum FileMeta {
    Downloaded,
    Failed,
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
    // pub connection: Connection,
    pub db_handler: DatabaseHandler,

    pub messages: MessagesStorage,
    pub chats: ChatList,
    pub sorted_chats: Vec<Chat>,
    pub selected_chat_jid: Option<wr::JID>,
    pub selected_chat_index: Option<usize>,

    pub history_sync_percent: Arc<Mutex<Option<u32>>>,
    pub message_queue: Arc<Mutex<Vec<wr::Message>>>,
    pub event_queue: Arc<Mutex<Vec<AppEvent>>>,

    pub message_list_state: MessageListState,
    pub metadata: MetadataStorage,
    pub image_cache: HashMap<Arc<str>, StatefulProtocol>,
    pub picker: Picker,
    pub selected_message: Option<wr::MessageId>,
    pub active_image: Option<wr::MessageId>,

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

        // let connection = Connection::open("whatsapp.db").unwrap();

        let mut picker = Picker::from_query_stdio().unwrap();
        picker.set_protocol_type(ratatui_image::picker::ProtocolType::Halfblocks);

        Self {
            db_handler: DatabaseHandler::new("whatsapp.db"),
            // connection,
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
            selected_message: None,
            active_image: None,
            picker,
            // picker: Picker::from_query_stdio().unwrap(),
            input_widget,
            should_quit: false,
        }
    }
}

impl App<'_> {
    pub fn init(&mut self) {
        self.db_handler.init();

        info!("Reading database");
        for chat in self.db_handler.get_chats() {
            self.chats.insert(chat.jid.clone(), chat);
        }

        for message in self.db_handler.get_messages() {
            self.add_message(message);
        }
        self.sort_chats();
    }

    pub fn tick(&mut self) {
        {
            let events = {
                let mut event_queue = self.event_queue.lock().unwrap();
                let mut events = Vec::new();
                while let Some(event) = event_queue.pop() {
                    events.push(event);
                }
                events
            };

            for event in events {
                match event {
                    AppEvent::StateSyncComplete => {
                        self.get_contacts();
                        self.sort_chats();
                    }
                    AppEvent::DownloadFile(message_id, file_id) => {
                        let state = match wr::download_file(&file_id) {
                            Ok(_) => FileMeta::Downloaded,
                            Err(_) => FileMeta::Failed,
                        };

                        self.metadata
                            .insert(message_id.clone(), Metadata::File(state));
                    }
                }
            }
        }

        {
            let messages = {
                let mut message_queue = self.message_queue.lock().unwrap();
                let mut messages = Vec::new();
                while let Some(msg) = message_queue.pop() {
                    messages.push(msg);
                }
                messages
            };

            if !messages.is_empty() {
                info!("Handling {} new messages", messages.len());
            }

            for msg in messages {
                self.db_handler.add_message(&msg);
                self.add_message(msg);

                self.sort_chats();
                if let Some(ref current_jid) = self.selected_chat_jid {
                    self.selected_chat_index = self
                        .sorted_chats
                        .iter()
                        .position(|chat| &chat.jid == current_jid);
                }
            }
        }
    }

    pub fn on_event(&mut self, event: Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') if key.modifiers == KeyModifiers::CONTROL => {
                        self.db_handler.stop();
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
                            let next_chat = self.sorted_chats[next_index].jid.clone();
                            self.selected_chat_jid = Some(next_chat);
                            self.selected_chat_index = Some(next_index);
                        } else {
                            self.selected_chat_index = Some(0);
                            self.selected_chat_jid = Some(self.sorted_chats[0].jid.clone());
                        }
                        self.message_list_state.select(None);
                    }
                    KeyCode::Char('v') if key.modifiers == KeyModifiers::CONTROL => {
                        // TODO: Make this not horrible
                        if let Some(chat_jid) = self.selected_chat_jid.clone() {
                            if let Some(msg_id) = &self.selected_message {
                                if let Some(message) = self.messages.get_mut(&chat_jid) {
                                    if let Some(msg) = message.get(msg_id) {
                                        if let wr::MessageContent::Image(image) = &msg.message {
                                            if let Some(Metadata::File(meta)) =
                                                self.metadata.get(msg_id)
                                            {
                                                if let FileMeta::Downloaded = meta {
                                                    self.active_image = Some(image.path.clone());
                                                    // info!("Opening image: {:?}", msg);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    KeyCode::Esc => {
                        if self.active_image.is_none() {
                            self.message_list_state.select(None);
                        }
                        self.active_image = None;
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

    fn add_message(&mut self, message: wr::Message) {
        let chat_jid = message.info.chat.clone();
        self.add_or_update_chat(
            Chat {
                jid: chat_jid.clone(),
                name: None,
                last_message_time: Some(message.info.timestamp),
            },
            |chat| {
                if Some(message.info.timestamp) > chat.last_message_time {
                    chat.last_message_time = Some(message.info.timestamp);
                }
            },
        );

        self.messages
            .entry(chat_jid)
            .or_default()
            .insert(message.info.id.clone(), message);
    }

    fn add_or_update_chat<F: FnOnce(&mut Chat)>(&mut self, chat: Chat, callback: F) {
        if let Some(existing_chat) = self.chats.get_mut(&chat.jid) {
            callback(existing_chat);
            self.db_handler.add_chat(existing_chat);
        } else {
            self.db_handler.add_chat(&chat);
            self.chats.insert(chat.jid.clone(), chat);
        }
    }

    fn get_contacts(&mut self) {
        let chat_list = wr::get_all_contacts();
        for (jid, contact) in chat_list {
            let name = get_contact_name(&contact);
            self.add_or_update_chat(
                Chat {
                    jid: jid.clone(),
                    name: name.clone(),
                    last_message_time: None,
                },
                |chat| {
                    chat.name = name;
                },
            );

            // if let Some(chat) = self.chats.get_mut(&jid) {
            //     chat.name = name;
            // } else {
            //     self.add_new_chat(Chat {
            //         jid,
            //         name,
            //         last_message_time: None,
            //     });
            // }
        }

        for group_info in wr::get_joined_groups() {
            self.add_or_update_chat(
                Chat {
                    jid: group_info.jid.clone(),
                    name: Some(group_info.name.clone()),
                    last_message_time: None,
                },
                |chat| {
                    chat.name = Some(group_info.name);
                },
            );

            // if let Some(chat) = self.chats.get_mut(&group_info.jid) {
            //     chat.name = Some(group_info.name.clone());
            // } else {
            //     self.add_new_chat(Chat {
            //         jid: group_info.jid,
            //         name: Some(group_info.name),
            //         last_message_time: None,
            //     });
            // };
        }
    }

    fn sort_chats(&mut self) {
        let mut entries: Vec<_> = self.chats.values().cloned().collect();
        entries.sort_by(|a, b| {
            let a_time = a.last_message_time.unwrap_or_default();
            let b_time = b.last_message_time.unwrap_or_default();
            b_time.cmp(&a_time)
        });
        self.sorted_chats = entries;
    }

    // fn add_new_chat(&mut self, chat: Chat) {
    //     if chat.jid.0.to_string() == *"34642137933@s.whatsapp.net" {
    //         error!("{:?}", chat);
    //     }
    //
    //     self.db_handler.add_chat(&chat);
    //
    //     self.chats.insert(chat.jid.clone(), chat);
    // }
}
