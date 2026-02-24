use wstui::*;

use clap::Parser;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    phone: Option<String>,
}

fn main() {
    let _ = tui_logger::init_logger(tui_logger::LevelFilter::Trace);
    tui_logger::set_default_level(tui_logger::LevelFilter::Trace);

    // let res = simple_logger::SimpleLogger::new().init();
    // if let Err(err) = res {
    //     panic!("Failed to initialize logger: {}", err);
    // }

    let args = Args::parse();

    let mut app = App::default();
    app.run(args.phone);
}
