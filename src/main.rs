use log::info;
use ratatui::{
    Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Layout, Rect},
    style::Style,
    widgets::{Block, List, ListState},
};
use std::{
    ffi::c_char,
    sync::{Arc, Mutex},
};
use tui_logger::{self, TuiLoggerWidget};
use tui_textarea::TextArea;
use whatsrust as wr;

struct ContactEntry {
    name: String,
    jid: wr::JID,
    _contact: wr::Contact,
    _last_message_time: Option<i64>,
}

fn get_contact_name(contact: &wr::Contact) -> Option<String> {
    if !contact.full_name.is_empty() {
        Some(contact.full_name.clone())
    } else if !contact.first_name.is_empty() {
        Some(contact.first_name.clone())
    } else if !contact.push_name.is_empty() {
        Some(format!("~ {}", contact.push_name))
    } else if !contact.business_name.is_empty() {
        Some(format!("+ {}", contact.business_name.clone()))
    } else {
        None
    }
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
    log::log!(level, "{}", msg);
}

fn main() {
    let db_path = "examplestore.db";

    // let test = Arc::new(Mutex::new("puto".to_string()));
    //
    // let test_clone = Arc::clone(&test);
    // wr::test(move |str| {
    //     let mut test = test_clone.lock().unwrap();
    //     test.push_str(str.as_str());
    // });
    // println!("{}", *test.lock().unwrap());

    let _ = tui_logger::init_logger(tui_logger::LevelFilter::Debug);
    tui_logger::set_default_level(tui_logger::LevelFilter::Debug);

    unsafe {
        wr::C_SetLogHandler(log_handler);
    }

    info!("Starting WhatsRust...");

    wr::new_client(db_path);
    wr::connect(|data| qr2term::print_qr(data).unwrap());

    let event_queue = Arc::new(Mutex::new(Vec::<String>::new()));

    let event_queue_clone = Arc::clone(&event_queue);
    wr::add_event_handler(move |msg| {
        let mut event_queue = event_queue_clone.lock().unwrap();
        // println!("Queue Event: {}", msg);
        // warn!("Event: {}", msg);
        event_queue.push(msg);
    });

    let all_contacts = wr::get_all_contacts();
    let contacts_list: Vec<_> = all_contacts
        .iter()
        .filter_map(|(jid, contact)| {
            let name_opt = get_contact_name(contact);
            name_opt.map(|name| ContactEntry {
                name,
                jid: jid.clone(),
                _contact: contact.clone(),
                _last_message_time: None,
            })
        })
        .collect();

    let mut msgs: Vec<String> = vec!["puto".to_string()];

    let mut terminal = ratatui::init();

    let mut input_widget = TextArea::default();
    input_widget.set_cursor_line_style(Style::default());
    input_widget.set_placeholder_text("Type a message...");
    input_widget.set_block(
        Block::default()
            .title("Input")
            .borders(ratatui::widgets::Borders::ALL),
    );
    let contact = contacts_list
        .iter()
        .find(|contact| contact.jid.user == "34693729055");

    loop {
        {
            let mut event_queue = event_queue.lock().unwrap();
            while let Some(msg) = event_queue.pop() {
                info!("Event: {}", msg);
                msgs.push(msg);
            }
        }

        terminal
            .draw(|frame| {
                let layout = Layout::horizontal([
                    Constraint::Min(40),
                    Constraint::Percentage(50),
                    Constraint::Percentage(50),
                ])
                .split(frame.area());
                let contacts_area = layout[0];
                let chat_area = layout[1];
                let logs_area = layout[2];
                render_contacts(frame, &contacts_list, contacts_area);
                render_chat(frame, contact, &msgs, &mut input_widget, chat_area);
                render_logs(frame, logs_area);
            })
            .unwrap();

        if event::poll(std::time::Duration::from_millis(100)).unwrap() {
            if let Event::Key(key) = event::read().unwrap() {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => break,
                        KeyCode::Enter | KeyCode::Char('\n') => {
                            if let Some(c) = contact {
                                let msg = input_widget.lines().join("\n");
                                wr::send_message(&c.jid, msg.as_str());
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

fn render_logs(frame: &mut Frame, area: Rect) {
    let log_widget = TuiLoggerWidget::default().block(
        Block::default()
            .title("Logs")
            .borders(ratatui::widgets::Borders::ALL),
    );
    frame.render_widget(log_widget, area);
}

fn render_chat(
    frame: &mut Frame,
    contact_opt: Option<&ContactEntry>,
    _msgs: &[String],
    input_widget: &mut TextArea,
    area: Rect,
) {
    // println!("Render Chat");
    let layout =
        Layout::vertical([Constraint::Percentage(100), Constraint::Min(1 + 2)]).split(area);

    let chat_area = layout[0];
    let input_area = layout[1];

    if let Some(contact) = contact_opt {
        let chat_history = [
            "Hello, how are you?",
            "I'm fine, thank you!",
            "What about you?",
        ];

        let items = chat_history
            .iter()
            .map(|msg| format!("{:?}", msg))
            .collect::<Vec<_>>();

        let mut list_state = ListState::default();
        let list = List::new(items)
            .block(Block::bordered().title(format!("Chat with {}", contact.name)))
            .highlight_style(ratatui::style::Style::default().fg(ratatui::style::Color::Green));
        frame.render_stateful_widget(list, chat_area, &mut list_state);
    }

    frame.render_widget(&*input_widget, input_area);
}

fn render_contacts(frame: &mut Frame, contacts: &[ContactEntry], area: Rect) {
    let items = contacts
        .iter()
        .map(|contact| contact.name.to_string())
        // .map(|contact| contact.name.as_str())
        .collect::<Vec<_>>();

    let mut list_state = ListState::default().with_selected(Some(0));
    let list = List::new(items)
        .block(Block::bordered().title("Contacts"))
        .highlight_style(ratatui::style::Style::default().fg(ratatui::style::Color::Green));
    frame.render_stateful_widget(list, area, &mut list_state);
}
