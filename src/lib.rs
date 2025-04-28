use std::collections::HashMap;
use std::rc::Rc;

pub mod list;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    widgets::{Block, Borders, StatefulWidget, Widget},
};
use tui_logger::TuiLoggerWidget;
use whatsrust::{self as wr};

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

pub fn get_sorted_chats(chats: &ChatList) -> Vec<(&wr::JID, &ChatEntry)> {
    let mut entries: Vec<_> = chats.iter().collect();
    entries.sort_by(|a, b| {
        let a_time = a.1.last_message_time.unwrap_or_default();
        let b_time = b.1.last_message_time.unwrap_or_default();
        b_time.cmp(&a_time)
    });
    entries
}

pub type ChatMessages = HashMap<Rc<str>, wr::Message>;

pub type MessagesStorage = HashMap<wr::JID, ChatMessages>;

pub enum AppEvent {
    StateSyncComplete,
}

pub struct LogsWidgets;
impl Widget for LogsWidgets {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let log_widget =
            TuiLoggerWidget::default().block(Block::default().title("Logs").borders(Borders::ALL));
        log_widget.render(area, buf);
    }
}

pub struct ContactsWidget;
impl StatefulWidget for ContactsWidget {
    type State = ();
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        // let items = contacts .iter()
        //     .map(|contact| contact.name.to_string())
        //     // .map(|contact| contact.name.as_str())
        //     .collect::<Vec<_>>();
        //
        // let mut list_state = ListState::default().with_selected(Some(0));
        // let list = List::new(items)
        //     .block(Block::bordered().title("Contacts"))
        //     .highlight_style(ratatui::style::Style::default().fg(ratatui::style::Color::Green));
        // list.render(area, buf, list_state);
    }
}
