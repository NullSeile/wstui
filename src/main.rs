// use list::{WidgetList, WidgetListState};
use log::info;
use ratatui::crossterm::event;
use wstui::*;

use clap::Parser;
use std::{
    ffi::c_char,
    sync::{Arc, Mutex},
};
use whatsrust as wr;

#[derive(Parser)]
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
    let _ = tui_logger::init_logger(tui_logger::LevelFilter::Debug);
    tui_logger::set_default_level(tui_logger::LevelFilter::Debug);

    let mut app = App::default();
    app.init();

    let args = Args::parse();
    let ws_database_path = "examplestore.db";

    wr::set_log_handler(log_handler);

    let history_sync_percent_clone = Arc::clone(&app.history_sync_percent);
    wr::set_history_sync_handler(move |percent| {
        let mut history_sync_percent = history_sync_percent_clone.lock().unwrap();
        *history_sync_percent = Some(percent);
    });

    let event_queue_clone: Arc<Mutex<Vec<AppEvent>>> = Arc::clone(&app.event_queue);
    wr::set_state_sync_complete_handler(move || {
        let mut event_queue = event_queue_clone.lock().unwrap();
        event_queue.push(AppEvent::StateSyncComplete);
        info!("State sync complete");
    });

    let message_queue_clone = Arc::clone(&app.message_queue);
    wr::set_message_handler(move |message| {
        let mut message_queue = message_queue_clone.lock().unwrap();
        // info!("Eventttt: {message:?}");
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
    // app.chats = get_chats();
    // app.sorted_chats = get_sorted_chats(&app.chats);

    wr::add_event_handlers();

    let mut terminal = ratatui::init();

    loop {
        app.tick();

        terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();

        if event::poll(std::time::Duration::from_millis(100)).unwrap() {
            let event = event::read().unwrap();
            app.on_event(event);
        }

        if app.should_quit {
            break;
        }
    }

    ratatui::restore();
    wr::disconnect();
}
