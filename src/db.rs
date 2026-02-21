use std::sync::{Arc, Mutex};

use log::info;
use rusqlite::Connection;
use strum::IntoEnumIterator;
use whatsrust as wr;

use crate::Chat;

pub struct DatabaseHandler {
    db: Connection,
    new_messages_queue: Arc<Mutex<Vec<wr::Message>>>,
    new_chats_queue: Arc<Mutex<Vec<Chat>>>,
    should_stop: Arc<Mutex<bool>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl DatabaseHandler {
    pub fn new(db_path: &str) -> Self {
        let db = Connection::open(db_path).unwrap();

        let new_messages_queue = Arc::new(Mutex::new(Vec::<wr::Message>::new()));
        let new_chats_queue = Arc::new(Mutex::new(Vec::<Chat>::new()));
        let should_stop = Arc::new(Mutex::new(false));

        let new_messages_queue_clone = Arc::clone(&new_messages_queue);
        let new_chats_queue_clone = Arc::clone(&new_chats_queue);
        let should_stop_clone = Arc::clone(&should_stop);
        let db_path = db_path.to_string();
        let thread = std::thread::spawn(move || {
            let mut db = Connection::open(db_path).unwrap();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let new_chats = {
                    let mut queue = new_chats_queue_clone.lock().unwrap();
                    let mut chats = Vec::new();
                    while let Some(chat) = queue.pop() {
                        chats.push(chat);
                    }
                    chats
                };
                if !new_chats.is_empty() {
                    info!("Saving {} new chats to the database", new_chats.len());
                    let tx = db.transaction().unwrap();
                    {
                        let mut statement = tx
                            .prepare("INSERT OR REPLACE INTO chats (jid) VALUES (?)")
                            .unwrap();
                        for chat in new_chats {
                            statement.execute(rusqlite::params![&*chat.jid.0]).unwrap();
                        }
                    }
                    tx.commit().unwrap();
                }

                let messages = {
                    let mut queue = new_messages_queue_clone.lock().unwrap();
                    let mut messages = Vec::new();
                    while let Some(message) = queue.pop() {
                        messages.push(message);
                    }
                    messages
                };
                if !messages.is_empty() {
                    info!("Saving {} new messages to the database", messages.len());
                    let tx = db.transaction().unwrap();

                    {
                        let mut text_stmt = tx
                            .prepare("INSERT OR REPLACE INTO text_messages (id, chat_jid, sender_jid, timestamp, quote_id, is_from_me, read, message) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                            .unwrap();
                        let mut file_stmt = tx
                            .prepare("INSERT OR REPLACE INTO file_messages (id, chat_jid, sender_jid, timestamp, quote_id, is_from_me, read, kind, path, file_id, caption) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                            .unwrap();
                        for msg in &messages {
                            match &msg.message {
                                wr::MessageContent::Text(text) => {
                                    text_stmt
                                        .execute(rusqlite::params![
                                            msg.info.id,
                                            msg.info.chat.0,
                                            msg.info.sender.0,
                                            msg.info.timestamp,
                                            msg.info.quote_id,
                                            msg.info.is_from_me,
                                            msg.info.read_by,
                                            text,
                                        ])
                                        .unwrap();
                                }
                                wr::MessageContent::File(file) => {
                                    file_stmt
                                        .execute(rusqlite::params![
                                            msg.info.id,
                                            msg.info.chat.0,
                                            msg.info.sender.0,
                                            msg.info.timestamp,
                                            msg.info.quote_id,
                                            msg.info.is_from_me,
                                            msg.info.read_by,
                                            file.kind.clone() as u8,
                                            file.path,
                                            file.file_id,
                                            file.caption,
                                        ])
                                        .unwrap();
                                }
                            }
                        }
                    }
                    tx.commit().unwrap();
                }

                let should_stop = should_stop_clone.lock().unwrap();
                if *should_stop {
                    break;
                }
                drop(should_stop);
            }
        });

        Self {
            db,
            new_messages_queue,
            new_chats_queue,
            should_stop,
            thread: Some(thread),
        }
    }

    pub fn stop(&mut self) {
        let mut should_stop = self.should_stop.lock().unwrap();
        *should_stop = true;
        drop(should_stop);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }

    pub fn add_message(&self, message: &wr::Message) {
        let mut queue = self.new_messages_queue.lock().unwrap();
        queue.push(message.clone());
    }

    pub fn add_chat(&self, chat: &Chat) {
        let mut queue = self.new_chats_queue.lock().unwrap();
        queue.push(chat.clone());
    }

    pub fn get_chats(&self) -> Vec<Chat> {
        let mut query = self.db.prepare("SELECT jid FROM chats").unwrap();
        query
            .query_map([], |row| {
                let jid: String = row.get(0).unwrap();
                Ok(Chat {
                    jid: jid.into(),
                    last_message_time: None,
                })
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    pub fn get_messages(&self) -> Vec<wr::Message> {
        let mut messages = Vec::new();
        for kind in wr::MessageContent::iter() {
            let msgs = match kind {
                wr::MessageContent::Text(_) => {
                    let mut query = self.db.prepare("SELECT * FROM text_messages").unwrap();
                    query
                        .query_map([], |row| {
                            let id: String = row.get(0).unwrap();
                            let chat_jid: String = row.get(1).unwrap();
                            let sender_jid: String = row.get(2).unwrap();
                            let timestamp: i64 = row.get(3).unwrap();
                            let quote_id: Option<String> = row.get(4).unwrap_or(None);
                            let is_from_me: bool = row.get(5).unwrap();
                            let read_by: u16 = row.get(6).unwrap();

                            let message: String = row.get(7).unwrap();

                            Ok(wr::Message {
                                info: wr::MessageInfo {
                                    id: id.into(),
                                    chat: chat_jid.into(),
                                    sender: sender_jid.into(),
                                    timestamp,
                                    quote_id: quote_id.map(|q| q.into()),
                                    is_from_me,
                                    read_by,
                                },
                                message: wr::MessageContent::Text(message.into()),
                            })
                        })
                        .unwrap()
                        .collect::<Vec<Result<_, _>>>()
                }
                wr::MessageContent::File(_) => {
                    let mut query = self.db.prepare("SELECT * FROM file_messages").unwrap();
                    query
                        .query_map([], |row| {
                            let id: String = row.get(0).unwrap();
                            let chat_jid: String = row.get(1).unwrap();
                            let sender_jid: String = row.get(2).unwrap();
                            let timestamp: i64 = row.get(3).unwrap();
                            let quote_id: Option<String> = row.get(4).unwrap_or(None);
                            let is_from_me: bool = row.get(5).unwrap();
                            let read_by: u16 = row.get(6).unwrap();

                            let kind: u8 = row.get(7).unwrap();
                            let path: String = row.get(8).unwrap();
                            let file_id: String = row.get(9).unwrap();
                            let caption: Option<String> = row.get(10).unwrap_or(None);

                            Ok(wr::Message {
                                info: wr::MessageInfo {
                                    id: id.into(),
                                    chat: chat_jid.into(),
                                    sender: sender_jid.into(),
                                    timestamp,
                                    quote_id: quote_id.map(|q| q.into()),
                                    is_from_me,
                                    read_by,
                                },
                                message: wr::MessageContent::File(wr::FileContent {
                                    kind: wr::FileKind::from_repr(kind).unwrap(),
                                    path: path.into(),
                                    file_id: file_id.into(),
                                    caption: caption.map(|c| c.into()),
                                }),
                            })
                        })
                        .unwrap()
                        .collect::<Vec<Result<_, _>>>()
                }
            };

            for msg in msgs {
                messages.push(msg.unwrap());
            }
        }

        messages
    }

    pub fn add_contact(&self, jid: &wr::JID, name: &str) {
        self.db
            .execute(
                "INSERT OR REPLACE INTO contacts (jid, name) VALUES (?1, ?2)",
                rusqlite::params![&*jid.0, name],
            )
            .unwrap();
    }

    pub fn get_contacts(&self) -> Vec<(wr::JID, Arc<str>)> {
        let mut stmt = self.db.prepare("SELECT jid, name FROM contacts").unwrap();
        let rows = stmt
            .query_map([], |row| {
                let jid: String = row.get(0).unwrap();
                let name: String = row.get(1).unwrap();
                Ok((jid.into(), Arc::from(name)))
            })
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    pub fn init(&self) {
        self.db
            .execute(
                "CREATE TABLE IF NOT EXISTS chats (
                    jid TEXT PRIMARY KEY
                )",
                [],
            )
            .unwrap();

        self.db
            .execute(
                "CREATE TABLE IF NOT EXISTS contacts (
                    jid TEXT PRIMARY KEY,
                    name TEXT NOT NULL
                )",
                [],
            )
            .unwrap();

        for kind in wr::MessageContent::iter() {
            match kind {
                wr::MessageContent::Text(_) => {
                    self.db
                        .execute(
                            "CREATE TABLE IF NOT EXISTS text_messages (
                                id TEXT PRIMARY KEY,
                                chat_jid TEXT,
                                sender_jid TEXT,
                                timestamp INTEGER,
                                quote_id TEXT,
                                is_from_me INTEGER,
                                read INTEGER,

                                message TEXT
                            )",
                            [],
                        )
                        .unwrap();
                }
                wr::MessageContent::File(_) => {
                    self.db
                        .execute(
                            "CREATE TABLE IF NOT EXISTS file_messages (
                                id TEXT PRIMARY KEY,
                                chat_jid TEXT,
                                sender_jid TEXT,
                                timestamp INTEGER,
                                quote_id TEXT,
                                is_from_me INTEGER,
                                read INTEGER,

                                kind INTEGER,
                                path TEXT,
                                file_id TEXT,
                                caption TEXT
                            )",
                            [],
                        )
                        .unwrap();
                }
            }
        }
    }
}
