use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::{collections::HashMap, sync::Arc};

pub mod db;
pub mod message_list;
pub mod ui;
pub mod vim;

use db::DatabaseHandler;
use log::{error, info};
use message_list::MessageListState;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::widgets::Block;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use tui_textarea::TextArea;
use vim::Vim;
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

pub enum FileMeta {
    Downloaded,
    DownloadFailed,
    LoadFailed,
}

pub enum Metadata {
    File(FileMeta),
}

pub enum AppEvent {
    DownloadFile(wr::MessageId, wr::FileId),
    SetFileState(wr::MessageId, FileMeta),
}

pub enum AppInput {
    Draw,
    App(AppEvent),
    Message(wr::Message),
    WhatsApp(wr::Event),
    Terminal(Event),
}

pub enum SelectedWidget {
    ChatList,
    Input,
    MessageList,
    MessageView,
}

pub struct App<'a> {
    pub db_handler: DatabaseHandler,
    pub media_path: &'a Path,

    pub messages: HashMap<wr::MessageId, wr::Message>,
    pub chats: HashMap<wr::JID, Chat>,
    pub chat_messages: HashMap<wr::JID, Vec<wr::MessageId>>,

    pub sorted_chats: Vec<Chat>,
    pub selected_chat_jid: Option<wr::JID>,
    pub selected_chat_index: Option<usize>,

    pub history_sync_percent: Option<u8>,

    pub quoting_message: Option<wr::Message>,
    pub message_list_state: MessageListState,
    pub metadata: HashMap<wr::MessageId, Metadata>,
    pub image_cache: HashMap<Arc<str>, StatefulProtocol>,
    pub picker: Picker,

    pub selected_widget: SelectedWidget,

    pub vim: Vim,
    pub input_widget: TextArea<'a>,
    pub input_border: Block<'a>,

    pub should_quit: bool,

    pub tx: mpsc::Sender<AppInput>,
    pub rx: mpsc::Receiver<AppInput>,
}

impl Default for App<'_> {
    fn default() -> Self {
        let mut input_widget = TextArea::default();
        // input_widget.set_cursor_line_style(vim::Mode::Nor::default());
        input_widget.set_cursor_style(vim::Mode::Insert.cursor_style());
        // input_widget.set_block(vim::Mode::Normal.block());
        input_widget.set_placeholder_text("Type a message...");

        let mut picker = Picker::from_query_stdio().unwrap();
        picker.set_protocol_type(ratatui_image::picker::ProtocolType::Halfblocks);

        let (tx, rx) = mpsc::channel::<AppInput>();

        Self {
            db_handler: DatabaseHandler::new("whatsapp.db"),
            media_path: Path::new("media"),
            messages: HashMap::new(),
            chats: HashMap::new(),
            chat_messages: HashMap::new(),
            sorted_chats: Vec::new(),
            selected_chat_jid: None,
            selected_chat_index: None,
            message_list_state: MessageListState::default(),
            metadata: HashMap::new(),
            history_sync_percent: None,
            image_cache: HashMap::new(),
            quoting_message: None,
            picker,
            selected_widget: SelectedWidget::ChatList,
            vim: Vim::new(vim::Mode::Insert),
            input_border: vim::Mode::Insert.block(),
            input_widget,
            should_quit: false,
            tx,
            rx,
        }
    }
}

impl App<'_> {
    pub fn run(&mut self, phone: Option<String>) {
        self.db_init();

        let ws_database_path = "examplestore.db";

        {
            let tx = self.tx.clone();
            wr::set_log_handler(move |msg, level| {
                let level = match level {
                    0 => log::Level::Error,
                    1 => log::Level::Warn,
                    2 => log::Level::Info,
                    3 => log::Level::Debug,
                    _ => log::Level::Trace,
                };
                log::log!(level, "{msg}");
                tx.send(AppInput::Draw).unwrap();
            });
        }
        {
            let tx = self.tx.clone();
            wr::set_event_handler(move |event| {
                tx.send(AppInput::WhatsApp(event)).unwrap();
            })
        }
        {
            let tx = self.tx.clone();
            wr::set_message_handler(move |message, is_sync| {
                tx.send(AppInput::Message(message)).unwrap();
                if !is_sync {
                    tx.send(AppInput::Draw).unwrap();
                }
            });
        }

        info!("Starting WhatsRust...");

        wr::new_client(ws_database_path);

        wr::connect(move |data| {
            qr2term::print_qr(data).unwrap();
            if let Some(phone) = phone.as_ref() {
                let code = wr::pair_phone(phone);
                println!("Pairing code: {}", code);
            }
        });

        let mut terminal = ratatui::init();

        {
            let tx = self.tx.clone();
            thread::spawn(move || {
                loop {
                    if let Ok(event) = event::read() {
                        tx.send(AppInput::Terminal(event)).unwrap();
                    }
                }
            });
        }

        terminal.draw(|frame| ui::draw(frame, self)).unwrap();

        loop {
            let should_draw = match self.rx.recv() {
                Ok(AppInput::App(event)) => match event {
                    AppEvent::SetFileState(message_id, state) => {
                        self.metadata
                            .insert(message_id.clone(), Metadata::File(state));

                        true
                    }
                    AppEvent::DownloadFile(message_id, file_id) => {
                        let state = match wr::download_file(&file_id, self.media_path) {
                            Ok(_) => FileMeta::Downloaded,
                            Err(_) => FileMeta::DownloadFailed,
                        };

                        self.metadata
                            .insert(message_id.clone(), Metadata::File(state));

                        true
                    }
                },
                Ok(AppInput::WhatsApp(event)) => match event {
                    wr::Event::AppStateSyncComplete => {
                        self.get_contacts();
                        self.sort_chats();

                        true
                    }
                    wr::Event::SyncProgress(percent) => {
                        self.history_sync_percent = Some(percent);
                        true
                    }
                },
                Ok(AppInput::Message(msg)) => {
                    self.db_handler.add_message(&msg);
                    self.add_message(msg);

                    self.sort_chats();
                    if let Some(ref current_jid) = self.selected_chat_jid {
                        self.selected_chat_index = self
                            .sorted_chats
                            .iter()
                            .position(|chat| &chat.jid == current_jid);
                    }

                    false // We will redraw manually
                }
                Ok(AppInput::Terminal(event)) => {
                    self.on_event(event);
                    true
                }
                Ok(AppInput::Draw) => true,
                Err(_) => {
                    error!("Failed to receive input from channel");
                    true
                }
            };

            if should_draw {
                terminal.draw(|frame| ui::draw(frame, self)).unwrap();
            }

            if self.should_quit {
                break;
            }
        }

        ratatui::restore();
        wr::disconnect();
    }

    fn db_init(&mut self) {
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

    fn on_event(&mut self, event: Event) {
        // Handle widget transitions
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                if key.code == KeyCode::Char('q') && key.modifiers == KeyModifiers::CONTROL {
                    self.db_handler.stop();
                    self.should_quit = true;
                    return;
                }

                match self.selected_widget {
                    SelectedWidget::ChatList => {
                        if key.code == KeyCode::Char('l') && key.modifiers == KeyModifiers::CONTROL
                        {
                            self.selected_widget = SelectedWidget::Input;
                            self.input_widget.select_all();
                            return;
                        }
                    }
                    SelectedWidget::Input => {
                        if key.code == KeyCode::Char('k') && key.modifiers == KeyModifiers::CONTROL
                        {
                            self.selected_widget = SelectedWidget::MessageList;
                            return;
                        }
                        if key.code == KeyCode::Char('h') && key.modifiers == KeyModifiers::CONTROL
                        {
                            self.selected_widget = SelectedWidget::ChatList;
                            return;
                        }
                    }
                    SelectedWidget::MessageList => {
                        if key.code == KeyCode::Char('j') && key.modifiers == KeyModifiers::CONTROL
                        {
                            self.selected_widget = SelectedWidget::Input;
                            return;
                        }
                        if key.code == KeyCode::Char('h') && key.modifiers == KeyModifiers::CONTROL
                        {
                            self.selected_widget = SelectedWidget::ChatList;
                            return;
                        }
                    }
                    SelectedWidget::MessageView => {
                        if let Event::Key(key) = event {
                            if key.kind == KeyEventKind::Press && key.code == KeyCode::Esc {
                                self.selected_widget = SelectedWidget::MessageList;
                                return;
                            }
                        }
                    }
                }
            }
        }

        match self.selected_widget {
            SelectedWidget::ChatList => {
                self.chat_list_on_event(&event);
            }
            SelectedWidget::MessageList => {
                self.message_list_on_event(&event);
            }
            SelectedWidget::Input => {
                self.input_on_event(&event);
            }
            SelectedWidget::MessageView => {
                // self.message_list_on_event(&event);
            }
        }
    }

    fn input_on_event(&mut self, event: &Event) {
        if let Event::Key(key) = *event {
            if key.code == KeyCode::Char('r') && key.modifiers == KeyModifiers::CONTROL {
                self.quoting_message = None;
                return;
            }
            if key.code == KeyCode::Char('x') && key.modifiers == KeyModifiers::CONTROL {
                if let Some(c) = self.selected_chat_jid.clone() {
                    let text = self.input_widget.lines().join("\n");
                    wr::send_message(&c, text.as_str(), self.quoting_message.as_ref());
                    self.input_widget.select_all();
                    self.input_widget.delete_next_char();
                    self.quoting_message = None;
                }
                return;
            }
        }

        self.vim = match self
            .vim
            .transition(event.clone().into(), &mut self.input_widget)
        {
            vim::Transition::Mode(mode) if self.vim.mode != mode => {
                self.input_border = mode.block();
                self.input_widget.set_cursor_style(mode.cursor_style());
                Vim::new(mode)
            }
            vim::Transition::Nop | vim::Transition::Mode(_) => self.vim.clone(),
            vim::Transition::Pending(input) => self.vim.with_pending(input),
        };
    }

    fn chat_list_on_event(&mut self, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('j') | KeyCode::Char('k') => {
                        if let Some(index) = self.selected_chat_index {
                            let mut delta: isize = 0;
                            if key.code == KeyCode::Char('j') {
                                delta = 1;
                            } else if key.code == KeyCode::Char('k') {
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
                        self.sort_chat_messages(self.selected_chat_jid.as_ref().unwrap().clone());
                        self.message_list_state.reset();
                    }
                    KeyCode::Enter => {
                        if let Some(index) = self.selected_chat_index {
                            let chat_jid = self.sorted_chats[index].jid.clone();
                            self.selected_chat_jid = Some(chat_jid);
                            self.message_list_state.reset();
                            self.selected_widget = SelectedWidget::Input;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn message_list_on_event(&mut self, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('J') => {
                        self.message_list_state.offset =
                            self.message_list_state.offset.saturating_sub(1);
                    }
                    KeyCode::Char('K') => {
                        self.message_list_state.offset =
                            self.message_list_state.offset.saturating_add(1);
                    }
                    KeyCode::Char('j') => {
                        self.message_list_state.select_previous();
                    }
                    KeyCode::Char('k') => {
                        self.message_list_state.select_next();
                    }
                    KeyCode::Char('r') => {
                        if let Some(msg_id) = &self.message_list_state.selected_message {
                            if let Some(msg) = self.messages.get(msg_id) {
                                self.quoting_message = Some(msg.clone());
                                self.selected_widget = SelectedWidget::Input;
                            }
                        }
                    }
                    KeyCode::Enter => {
                        if self.message_list_state.selected_message.is_some() {
                            self.selected_widget = SelectedWidget::MessageView;
                        }
                    }
                    KeyCode::Esc => {
                        self.message_list_state.reset();
                    }
                    _ => {}
                }
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

        let id = message.info.id.clone();

        // Insert the message into the messages map, if it's aleardy present,
        // updated if the message is newer.
        if let Some(existing_message) = self.messages.get_mut(&id) {
            if existing_message.info.timestamp < message.info.timestamp {
                *existing_message = message;
                self.sort_chat_messages(chat_jid.clone());
            }
        } else {
            self.messages.insert(id.clone(), message);
            self.chat_messages
                .entry(chat_jid.clone())
                .or_default()
                .push(id);
        }
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

    fn sort_chat_messages(&mut self, chat_jid: wr::JID) {
        if let Some(messages) = self.chat_messages.get_mut(&chat_jid) {
            messages.sort_by_cached_key(|msg_id| self.messages.get(msg_id).unwrap().info.timestamp);
        }
    }
}
