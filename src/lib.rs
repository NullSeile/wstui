use core::fmt;
use std::path::PathBuf;
use std::sync::mpsc;
use std::{collections::HashMap, sync::Arc, sync::Mutex};
use std::{fs, thread};

pub mod db;
pub mod ui;
pub mod vim;

use db::DatabaseHandler;
use directories::ProjectDirs;
use log::{debug, error, info, trace};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, ListState};
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, ResizeEncodeRender};
use ratatui_textarea::TextArea;
use rfd::FileDialog;
use ui::message_list::MessageListState;
use ui::message_list::{IMAGE_HEIGHT, IMAGE_WIDTH};
use vim::Vim;
use whatsrust as wr;

use crate::ui::text_input::TextInput;

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
    pub last_message_time: Option<i64>,
}

#[derive(Debug)]
pub enum FileMeta {
    Loaded,
    Loading,
    LoadFailed,
    Downloaded,
    Downloading,
    DownloadFailed,
}

pub enum Metadata {
    File(FileMeta),
}

pub enum AppEvent {
    DownloadFile(wr::MessageId, wr::FileId),
    DownloadFileDone(wr::MessageId, FileMeta),
    LoadFilePreview(wr::MessageId),
    SetFilePreview(wr::MessageId, Arc<str>, StatefulProtocol),
    SetFileState(wr::MessageId, FileMeta),
}

impl fmt::Debug for AppEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppEvent::DownloadFile(message_id, file_id) => f
                .debug_tuple("DownloadFile")
                .field(message_id)
                .field(file_id)
                .finish(),
            AppEvent::DownloadFileDone(message_id, state) => f
                .debug_tuple("DownloadFileDone")
                .field(message_id)
                .field(state)
                .finish(),
            AppEvent::LoadFilePreview(message_id) => {
                f.debug_tuple("LoadFilePreview").field(message_id).finish()
            }
            AppEvent::SetFilePreview(message_id, path, _) => f
                .debug_tuple("SetFilePreview")
                .field(message_id)
                .field(path)
                .finish(),
            AppEvent::SetFileState(message_id, state) => f
                .debug_tuple("SetFileState")
                .field(message_id)
                .field(state)
                .finish(),
        }
    }
}

#[derive(Debug)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}
impl Key {
    pub fn c(c: char) -> Self {
        if c.is_ascii_uppercase() {
            Self {
                code: KeyCode::Char(c),
                modifiers: KeyModifiers::SHIFT,
            }
        } else {
            Self {
                code: KeyCode::Char(c),
                modifiers: KeyModifiers::NONE,
            }
        }
    }

    pub fn k(c: KeyCode) -> Self {
        Self {
            code: c,
            modifiers: KeyModifiers::NONE,
        }
    }

    pub fn ctrl(c: char) -> Self {
        Self {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::CONTROL,
        }
    }

    pub fn ctrl_shift(c: char) -> Self {
        Self {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        }
    }
}

pub struct App<'a> {
    pub db_handler: DatabaseHandler,
    pub media_path: PathBuf,
    pub whatsmeow_db: PathBuf,

    pub messages: HashMap<wr::MessageId, wr::Message>,
    pub chats: HashMap<wr::JID, Chat>,

    // Maps JID to display name
    pub contacts: HashMap<wr::JID, Arc<str>>,

    pub chat_messages: HashMap<wr::JID, Vec<wr::MessageId>>,

    pub sorted_chats: Vec<wr::JID>,
    pub chat_list_state: ListState,

    pub history_sync_percent: Option<u8>,

    pub quoting_message: Option<wr::Message>,
    pub attached_file: Option<(Arc<str>, wr::FileKind)>,
    pub message_list_state: MessageListState,
    pub metadata: HashMap<wr::MessageId, Metadata>,
    pub image_cache: HashMap<Arc<str>, StatefulProtocol>,
    pub default_protocol_type: ProtocolType,
    pub picker: Arc<Mutex<Picker>>,

    pub selected_widget: SelectedWidget,

    pub key_buffer: Vec<Key>,
    pub key_sequence_active: bool,

    pub show_logs: bool,

    pub vim: Vim,
    pub input_widget: TextArea<'a>,
    pub input_border: Block<'a>,

    pub contact_search_active: bool,
    pub contact_search: TextInput,
    pub filtered_chats: Vec<wr::JID>,

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

        let picker = Picker::from_query_stdio().unwrap_or_else(|err| {
            // Fallback for non-interactive environments (e.g. CI, piped stdio).
            log::warn!(
                "Failed to query terminal image capabilities; falling back to halfblocks: {err}"
            );
            Picker::halfblocks()
        });
        let default_protocol_type = picker.protocol_type();

        let project_dirs = ProjectDirs::from("com", "nullptr", "wstui").unwrap();
        let data_dir = project_dirs.data_dir();
        fs::create_dir_all(data_dir).unwrap();

        let (tx, rx) = mpsc::channel::<AppInput>();

        Self {
            db_handler: DatabaseHandler::new(&data_dir.join("whatsapp.db")),
            media_path: data_dir.join("media"),
            whatsmeow_db: data_dir.join("whatsmeow.db"),
            messages: HashMap::new(),
            chats: HashMap::new(),
            contacts: HashMap::new(),
            chat_messages: HashMap::new(),

            sorted_chats: Vec::new(),
            chat_list_state: ListState::default(),

            message_list_state: MessageListState::default(),
            metadata: HashMap::new(),
            history_sync_percent: None,
            image_cache: HashMap::new(),
            default_protocol_type,
            quoting_message: None,
            attached_file: None,
            picker: Arc::new(Mutex::new(picker)),
            selected_widget: SelectedWidget::ChatList,

            key_buffer: Vec::new(),
            key_sequence_active: false,

            contact_search_active: false,
            contact_search: TextInput::new(),
            filtered_chats: Vec::new(),

            show_logs: false,
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

        info!("Starting WhatsRust Client...");
        wr::new_client(self.whatsmeow_db.to_str().unwrap());
        info!("WhatsRust Client started");

        // Single dedicated thread for all CGo downloads. Calling Go from many Rust-spawned
        // threads can crash even with a mutex; one long-lived worker avoids that.
        let (download_tx, download_rx) = mpsc::channel::<(wr::MessageId, wr::FileId)>();
        let media_path = self.media_path.to_owned();
        let app_tx = self.tx.clone();
        thread::spawn(move || {
            for (message_id, file_id) in download_rx {
                let result = wr::download_file(&file_id, &media_path);
                let state = if result.is_err() {
                    FileMeta::DownloadFailed
                } else {
                    FileMeta::Downloaded
                };
                app_tx
                    .send(AppInput::App(AppEvent::SetFileState(message_id, state)))
                    .unwrap();
            }
        });

        // thread::spawn(|| {
        wr::connect(move |data| {
            qr2term::print_qr(data).unwrap();
            if let Some(phone) = phone.as_ref() {
                let code = wr::pair_phone(phone);
                println!("Pairing code: {}", code);
            }
        });
        // });

        let mut terminal = match ratatui::try_init() {
            Ok(terminal) => terminal,
            Err(e) => {
                error!("Failed to initialize terminal UI: {e}");
                eprintln!("Failed to initialize terminal UI: {e}");
                return;
            }
        };

        {
            let tx = self.tx.clone();
            thread::spawn(move || {
                loop {
                    if let Ok(event) = event::read() {
                        if let Err(e) = tx.send(AppInput::Terminal(event)) {
                            error!("Failed to send terminal event: {:?}", e);
                            break;
                        }
                    }
                }
            });
        }

        terminal.draw(|frame| ui::draw(frame, self)).unwrap();

        loop {
            let msg = self.rx.recv();
            // info!("Received message: {:?}", &msg);
            let should_draw = match msg {
                Ok(AppInput::App(event)) => match event {
                    AppEvent::SetFilePreview(message_id, file_path, img) => {
                        self.image_cache.insert(file_path.clone(), img);
                        self.metadata
                            .insert(message_id.clone(), Metadata::File(FileMeta::Loaded));

                        trace!("Set file preview for message: {:?}", message_id);

                        true
                    }
                    AppEvent::LoadFilePreview(message_id) => {
                        if !matches!(
                            self.metadata.get(&message_id),
                            Some(Metadata::File(FileMeta::Loading))
                        ) {
                            self.metadata
                                .insert(message_id.clone(), Metadata::File(FileMeta::Loading));

                            let tx = self.tx.clone();
                            let media_path = self.media_path.to_owned();
                            let picker = Arc::clone(&self.picker);

                            let file = match &self.messages.get(&message_id).unwrap().message {
                                wr::MessageContent::File(f) => Some(f.clone()),
                                _ => None,
                            };
                            if let Some(file) = file {
                                thread::spawn(move || {
                                    let binding = file.path.to_string();
                                    let path = std::path::Path::new(&binding);
                                    let image_res = image::ImageReader::open(media_path.join(path))
                                        .unwrap()
                                        .decode();

                                    if let Ok(image_src) = image_res {
                                        let mut img =
                                            picker.lock().unwrap().new_resize_protocol(image_src);
                                        img.resize_encode(
                                            &Resize::Scale(None),
                                            Rect {
                                                x: 0,
                                                y: 0,
                                                width: IMAGE_WIDTH as u16,
                                                height: IMAGE_HEIGHT as u16,
                                            },
                                        );

                                        tx.send(AppInput::App(AppEvent::SetFilePreview(
                                            message_id.clone(),
                                            file.path.clone(),
                                            img,
                                        )))
                                        .unwrap();
                                    } else {
                                        tx.send(AppInput::App(AppEvent::SetFileState(
                                            message_id.clone(),
                                            FileMeta::LoadFailed,
                                        )))
                                        .unwrap();
                                    }
                                });
                            } else {
                                error!("Expected a file message for preview");
                            }
                        }
                        false // We will redraw after the preview is loaded
                    }
                    AppEvent::SetFileState(message_id, state) => {
                        self.metadata
                            .insert(message_id.clone(), Metadata::File(state));

                        true
                    }
                    AppEvent::DownloadFile(message_id, file_id) => {
                        if matches!(
                            self.metadata.get(&message_id),
                            Some(Metadata::File(FileMeta::Downloading))
                        ) {
                            false
                        } else {
                            self.metadata
                                .insert(message_id.clone(), Metadata::File(FileMeta::Downloading));
                            download_tx.send((message_id, file_id)).unwrap();
                            false
                        }
                    }
                    AppEvent::DownloadFileDone(message_id, state) => {
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
                    wr::Event::Receipt {
                        kind,
                        chat,
                        message_ids,
                    } => {
                        debug!(
                            "Received receipt: {:?} for chat: {:?} with messages: {:?}",
                            kind, chat, message_ids
                        );
                        for msg_id in message_ids {
                            if let Some(message) = self.messages.get_mut(&msg_id) {
                                message.info.read_by += 1;
                                self.db_handler.add_message(message);
                            }
                        }
                        true
                    }
                },
                Ok(AppInput::Message(msg)) => {
                    self.db_handler.add_message(&msg);
                    self.add_message(msg);

                    let chat_jid = self.get_selected_chat();

                    self.sort_chats();

                    self.select_chat(chat_jid);
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

    fn key_matches(&mut self, expected: &[Key]) -> bool {
        if self.key_buffer.len() == expected.len()
            && self
                .key_buffer
                .iter()
                .zip(expected.iter())
                .all(|(a, b)| a == b)
        {
            self.key_buffer.clear();
            return true;
        }

        if self.key_buffer.len() < expected.len()
            && self
                .key_buffer
                .iter()
                .zip(expected.iter())
                .all(|(a, b)| a == b)
        {
            self.key_sequence_active = true;
        }
        false
    }

    fn db_init(&mut self) {
        self.db_handler.init();

        info!("Reading database");
        for chat in self.db_handler.get_chats() {
            self.chats.insert(chat.jid.clone(), chat);
        }
        for (jid, name) in self.db_handler.get_contacts() {
            self.contacts.insert(jid, name);
        }

        for message in self.db_handler.get_messages() {
            self.add_message(message);
        }
        self.sort_chats();
    }

    /// Display name for a JID (chat or sender). Falls back to the JID string if not in contacts.
    pub fn contact_name(&self, jid: &wr::JID) -> Arc<str> {
        self.contacts
            .get(jid)
            .cloned()
            .unwrap_or_else(|| jid.0.clone())
    }

    fn on_event(&mut self, event: Event) {
        // Handle widget transitions
        if let Event::Key(key_event) = event {
            if key_event.kind == KeyEventKind::Press {
                self.key_sequence_active = false;

                let key = Key {
                    code: key_event.code,
                    modifiers: key_event.modifiers,
                };
                if key == Key::k(KeyCode::Esc) && self.key_buffer.len() > 0 {
                    self.key_buffer.clear();
                    return;
                } else {
                    self.key_buffer.push(key);
                }

                if self.key_matches(&[Key::ctrl('q')]) {
                    self.db_handler.stop();
                    self.should_quit = true;
                    return;
                }

                if self.key_matches(&[Key::ctrl_shift('l')]) {
                    self.show_logs = !self.show_logs;
                    return;
                }

                if self.key_matches(&[Key::ctrl('p')]) {
                    let next = {
                        let mut picker = self.picker.lock().unwrap();
                        let current = picker.protocol_type();
                        let next = if current == ProtocolType::Halfblocks {
                            self.default_protocol_type
                        } else {
                            ProtocolType::Halfblocks
                        };
                        picker.set_protocol_type(next);
                        next
                    };
                    self.image_cache.clear();
                    for (message_id, meta) in self.metadata.iter_mut() {
                        if let Metadata::File(FileMeta::Loaded) = meta {
                            if let Some(msg) = self.messages.get(message_id) {
                                if let wr::MessageContent::File(file) = &msg.message {
                                    if matches!(
                                        file.kind,
                                        wr::FileKind::Image | wr::FileKind::Sticker
                                    ) {
                                        *meta = Metadata::File(FileMeta::Downloaded);
                                    }
                                }
                            }
                        }
                    }
                    debug!("Image protocol: {:?}", next);
                    return;
                }

                match self.selected_widget {
                    SelectedWidget::ChatList => {
                        if self.key_matches(&[Key::ctrl('l')]) {
                            self.selected_widget = SelectedWidget::MessageList;
                            self.input_widget.select_all();
                            return;
                        }
                    }
                    SelectedWidget::Input => {
                        if self.key_matches(&[Key::ctrl('k')]) {
                            self.selected_widget = SelectedWidget::MessageList;
                            return;
                        }
                        if self.key_matches(&[Key::ctrl('h')]) {
                            self.selected_widget = SelectedWidget::ChatList;
                            return;
                        }
                    }
                    SelectedWidget::MessageList => {
                        if self.key_matches(&[Key::ctrl('j')]) {
                            self.selected_widget = SelectedWidget::Input;
                            return;
                        }
                        if self.key_matches(&[Key::ctrl('h')]) {
                            self.selected_widget = SelectedWidget::ChatList;
                            return;
                        }
                    }
                    SelectedWidget::MessageView => {
                        if self.key_matches(&[Key::k(KeyCode::Esc)]) {
                            self.selected_widget = SelectedWidget::MessageList;
                            return;
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
            SelectedWidget::MessageView => {}
        }

        if let Event::Key(_) = event {
            if self.key_sequence_active == false {
                self.key_buffer.clear();
            }
        }
    }

    fn input_on_event(&mut self, event: &Event) {
        if let Event::Key(key) = *event
            && key.kind == KeyEventKind::Press
        {
            if self.key_matches(&[Key::ctrl('x')]) {
                if let Some(c) = self.get_selected_chat() {
                    let text = self.input_widget.lines().join("\n");
                    let msg = if let Some((path, typ)) = &self.attached_file {
                        wr::MessageContent::File(wr::FileContent {
                            kind: typ.clone(),
                            path: path.clone(),
                            file_id: "".into(),
                            caption: Some(text.into()),
                        })
                    } else {
                        wr::MessageContent::Text(text.into())
                    };

                    wr::send_message(&c, &msg, self.quoting_message.as_ref());

                    self.input_widget.select_all();
                    self.input_widget.delete_next_char();
                    self.quoting_message = None;
                    self.attached_file = None;
                }
                return;
            }
            if self.vim.mode == vim::Mode::Normal {
                if self.key_matches(&[Key::c(' '), Key::c('r')]) {
                    self.quoting_message = None;
                    return;
                }

                if self.key_matches(&[Key::c(' '), Key::c('a'), Key::c('r')]) {
                    self.attached_file = None;
                }
                if self.key_matches(&[Key::c(' '), Key::c('a'), Key::c('i')]) {
                    if let Some(path) = FileDialog::new().pick_file() {
                        self.attached_file =
                            Some((path.to_str().unwrap().into(), wr::FileKind::Image));
                    }
                } else if self.key_matches(&[Key::c(' '), Key::c('a'), Key::c('d')]) {
                    if let Some(path) = FileDialog::new().pick_file() {
                        self.attached_file =
                            Some((path.to_str().unwrap().into(), wr::FileKind::Document));
                    }
                }
            }
        }

        if !self.key_sequence_active {
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
    }

    fn chat_list_on_event(&mut self, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                if self.key_matches(&[Key::k(KeyCode::Esc)]) {
                    let chat_jid = self.get_selected_chat();

                    self.contact_search_active = false;
                    self.contact_search.clean();

                    self.select_chat(chat_jid);

                    return;
                }

                if !self.contact_search_active {
                    let mut moved = false;
                    if self.key_matches(&[Key::c('j')]) {
                        self.chat_list_state.select_next();
                        moved = true;
                    } else if self.key_matches(&[Key::c('k')]) {
                        self.chat_list_state.select_previous();
                        moved = true;
                    }
                    if moved {
                        // Bound the selected index to the number of chats
                        let len = self.sorted_chats.len();
                        if len == 0 {
                            self.chat_list_state.select(None);
                            return;
                        } else if let Some(selected) = self.chat_list_state.selected() {
                            if selected >= len {
                                self.chat_list_state.select(Some(len.saturating_sub(1)));
                            }
                        }

                        self.sort_chat_messages(self.get_selected_chat().unwrap());
                        self.message_list_state.reset();
                    }

                    if self.key_matches(&[Key::k(KeyCode::Enter)]) {
                        if self.chat_list_state.selected().is_some() {
                            self.message_list_state.reset();
                            self.selected_widget = SelectedWidget::Input;
                        }
                    } else if self.key_matches(&[Key::c('/')]) {
                        self.contact_search_active = true;
                    }
                } else {
                    match key.code {
                        // KeyCode::Enter => self.contact_search_active.submit_message(),
                        KeyCode::Char(to_insert) => {
                            self.contact_search.enter_char(to_insert);
                            self.update_filtered_chats();
                        }
                        KeyCode::Backspace => self.contact_search.delete_char(),
                        KeyCode::Left => self.contact_search.move_cursor_left(),
                        KeyCode::Right => self.contact_search.move_cursor_right(),
                        KeyCode::Enter => {
                            self.contact_search_active = false;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn get_selected_chat(&self) -> Option<wr::JID> {
        self.chat_list_state.selected().map(|index| {
            if self.contact_search.input.is_empty() {
                self.sorted_chats[index].clone()
            } else {
                self.filtered_chats[index].clone()
            }
        })
    }

    pub fn select_chat(&mut self, jid: Option<wr::JID>) {
        if let Some(jid) = jid
            && let Some(index) = self
                .sorted_chats
                .iter()
                .position(|chat_jid| chat_jid == &jid)
        {
            self.chat_list_state.select(Some(index));
        } else if self.sorted_chats.len() > 0 {
            self.chat_list_state.select(Some(0));
        } else {
            self.chat_list_state.select(None);
        }
    }

    fn update_filtered_chats(&mut self) {
        let query = self.contact_search.input.to_lowercase();
        self.filtered_chats = self
            .sorted_chats
            .iter()
            .filter(|chat| {
                let name = self.contact_name(&chat).to_lowercase();
                name.contains(&query)
            })
            .map(|chat| chat.clone())
            .collect();

        if self.filtered_chats.len() > 0 {
            self.chat_list_state.select(Some(0));
        } else {
            self.chat_list_state.select(None);
        }
    }

    fn message_list_on_event(&mut self, event: &Event) {
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            if self.key_matches(&[Key::ctrl('e')]) {
                self.message_list_state.offset = self.message_list_state.offset.saturating_sub(1);
            }
            if self.key_matches(&[Key::ctrl('y')]) {
                self.message_list_state.offset = self.message_list_state.offset.saturating_add(1);
            }
            if self.key_matches(&[Key::c('g'), Key::c('q')]) {
                if let Some(msg_id) = &self.message_list_state.get_selected_message()
                    && let Some(msg) = self.messages.get(msg_id)
                    && let Some(ref quote_id) = msg.info.quote_id
                {
                    self.message_list_state
                        .set_selected_message(quote_id.clone());
                }
                return;
            }

            if self.key_matches(&[Key::c('k')]) {
                self.message_list_state.select_next();
            } else if self.key_matches(&[Key::c('j')]) {
                self.message_list_state.select_previous();
            } else if self.key_matches(&[Key::c('G')]) {
                self.message_list_state.select_first();
            } else if self.key_matches(&[Key::c('g'), Key::c('g')]) {
                self.message_list_state.select_last();
            } else if self.key_matches(&[Key::c('r')]) {
                if let Some(msg_id) = &self.message_list_state.get_selected_message() {
                    if let Some(msg) = self.messages.get(msg_id) {
                        self.quoting_message = Some(msg.clone());
                        self.selected_widget = SelectedWidget::Input;
                    }
                }
            } else if self.key_matches(&[Key::k(KeyCode::Enter)]) {
                if self.message_list_state.get_selected_message().is_some() {
                    self.selected_widget = SelectedWidget::MessageView;
                }
            } else if self.key_matches(&[Key::k(KeyCode::Esc)]) {
                self.message_list_state.reset();
            } else if self.key_matches(&[Key::c('o')]) {
                if let Some(msg_id) = self.message_list_state.get_selected_message()
                    && let Some(msg) = self.messages.get(&msg_id)
                {
                    match &msg.message {
                        wr::MessageContent::Text(_text) => {
                            // let mut file = tempfile::tempfile().unwrap();
                            //
                            // file.write_all(text.as_bytes()).unwrap();
                            //
                            // open::that(file.).unwrap();
                        }
                        wr::MessageContent::File(content) => {
                            match open::that(self.media_path.join(content.path.as_ref())) {
                                Ok(_) => {}
                                Err(e) => {
                                    error!("Failed to open file {}: {:?}", content.path, e);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn add_message(&mut self, message: wr::Message) {
        let chat_jid = message.info.chat.clone();
        self.add_or_update_chat(
            Chat {
                jid: chat_jid.clone(),
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
        for (jid, name) in wr::get_contacts() {
            self.contacts.insert(jid.clone(), name.clone());
            self.db_handler.add_contact(&jid, name.as_ref());
        }
    }

    fn sort_chats(&mut self) {
        let mut entries: Vec<_> = self.chats.values().cloned().collect();
        entries.sort_by(|a, b| {
            let a_time = a.last_message_time.unwrap_or_default();
            let b_time = b.last_message_time.unwrap_or_default();
            b_time.cmp(&a_time)
        });

        self.sorted_chats = entries.iter().map(|chat| chat.jid.clone()).collect();
    }

    fn sort_chat_messages(&mut self, chat_jid: wr::JID) {
        if let Some(messages) = self.chat_messages.get_mut(&chat_jid) {
            messages.sort_by_cached_key(|msg_id| {
                self.messages
                    .get(msg_id)
                    .map(|m| m.info.timestamp)
                    .unwrap_or(i64::MIN)
            });
        }
    }
}
