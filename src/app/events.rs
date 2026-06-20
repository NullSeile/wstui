use core::fmt;
use std::sync::Arc;

use ratatui::crossterm::event::Event;

use ratatui_image::protocol::StatefulProtocol;

use whatsrust as wr;

use crate::app::{App, FileMeta};


pub enum AppEvent {
    DownloadFile(wr::MessageId, wr::FileId),
    DownloadFileDone(wr::MessageId, FileMeta),
    LoadFilePreview(wr::MessageId),
    SetFilePreview(wr::MessageId, Arc<str>, StatefulProtocol),
    SetFileState(wr::MessageId, FileMeta),
    EditWithExternalEditor,
}

#[derive(Debug)]
pub enum AppInput {
    Draw,
    App(AppEvent),
    Message {
        message: wr::Message,
        is_sync: bool,
    },
    WhatsApp(wr::Event),
    Terminal(Event),
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
            AppEvent::EditWithExternalEditor => f.debug_tuple("EditWithExternalEditor").finish(),
        }
    }
}

impl App<'_> {

}

