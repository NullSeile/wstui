use wstui::*;

use clap::Parser;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    phone: Option<String>,
}

fn main() {
    let _ = tui_logger::init_logger(tui_logger::LevelFilter::Debug);
    tui_logger::set_default_level(tui_logger::LevelFilter::Debug);

    let args = Args::parse();

    let mut app = App::default();
    app.run(args.phone);
}
