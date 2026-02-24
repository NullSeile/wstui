use std::{
    cmp::{max, min},
    sync::Arc,
};

use chrono::{DateTime, Datelike, Local};
use log::trace;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::Line,
    widgets::{Block, Borders, Paragraph, StatefulWidget, Widget},
};
use ratatui_image::StatefulImage;
use textwrap;
use whatsrust::{self as wr, FileKind};

use crate::{App, AppEvent, AppInput, FileMeta, Metadata, SelectedWidget};

pub const IMAGE_HEIGHT: usize = 12;
pub const IMAGE_WIDTH: usize = IMAGE_HEIGHT * 3;

fn file_content_height(id: &wr::MessageId, file: &wr::FileContent, app: &mut App) -> usize {
    match file.kind {
        FileKind::Image | FileKind::Sticker => match app.metadata.get(id) {
            None => 1,
            Some(Metadata::File(meta)) => match meta {
                FileMeta::Downloading
                | FileMeta::DownloadFailed
                | FileMeta::Downloaded
                | FileMeta::LoadFailed
                | FileMeta::Loading => 1,
                FileMeta::Loaded => IMAGE_HEIGHT,
            },
        },
        FileKind::Video => 1,
        FileKind::Audio => 1,
        FileKind::Document => 1,
    }
}

fn message_height(message: &wr::Message, width: usize, app: &mut App) -> usize {
    let header_height = if message.info.quote_id.is_some() {
        2
    } else {
        1
    };

    let content_height = match &message.message {
        wr::MessageContent::Text(text) => {
            let lines = textwrap::wrap(text, width);
            lines.len()
        }
        wr::MessageContent::File(data) => {
            let lines = if let Some(caption) = &data.caption {
                textwrap::wrap(caption, width).len()
            } else {
                0
            };

            let content_height = file_content_height(&message.info.id, data, app);
            content_height + lines
        }
    };

    header_height + content_height
}

/// When `render_image` is false (partial path and image fully off-screen), show a placeholder
/// instead of StatefulImage so we don't mark the protocol as "transmitted" until we actually
/// send at least one row to the frame.
fn render_message(
    buf: &mut Buffer,
    message: &wr::Message,
    is_selected: bool,
    app: &mut App,
    area: Rect,
    render_image: bool,
) {
    if is_selected {
        let style = Style::default()
            .bg(ratatui::style::Color::Gray)
            .fg(ratatui::style::Color::Black);
        buf.set_style(area, style);
    }

    let alignment = ratatui::layout::Alignment::Left;
    // let alignment = if message.info.is_from_me {
    //     ratatui::layout::Alignment::Right
    // } else {
    //     ratatui::layout::Alignment::Left
    // };

    let timestamp = {
        let local_time: DateTime<Local> = DateTime::from_timestamp(message.info.timestamp, 0)
            .unwrap()
            .into();
        if local_time.date_naive() == Local::now().date_naive() {
            local_time.format("%H:%M").to_string()
        } else if local_time.date_naive() == (Local::now() - chrono::Duration::days(1)).date_naive()
        {
            local_time.format("Yesterday %H:%M").to_string()
        } else if local_time > (Local::now() - chrono::Duration::days(7)) {
            local_time.format("%a %H:%M").to_string()
        } else if local_time.year() == Local::now().year() {
            local_time.format("%d %b %H:%M").to_string()
        } else {
            local_time.format("%Y %d %b %H:%M").to_string()
        }
    }
    .italic();

    let sender_name = app.contact_name(&message.info.sender);

    let mut header = vec![
        sender_name.to_string().bold(),
        " (".into(),
        timestamp,
        ")".into(),
    ];
    if message.info.read_by >= 1 {
        header.push(" ✓".into());
    }
    header.push(" ".into());
    let msg_block = Block::default().borders(Borders::NONE).title(header);

    // let sender_widget = Line::from_iter(header).alignment(alignment).bold();

    let quoted_text = message
        .info
        .quote_id
        .as_ref()
        .and_then(|quote_id| app.messages.get(quote_id).map(get_quoted_text));

    let quote_widget = message.info.quote_id.as_ref().map(|_quote_id| {
        let quoted_text = quoted_text.unwrap_or_else(|| "not found".into());

        let line = Line::from(format!("> {quoted_text}")).alignment(alignment);
        if is_selected {
            line.dark_gray()
        } else {
            line.dark_gray()
        }
    });

    let msg_area = msg_block.inner(area);
    // let msg_area = area;
    // let msg_area = if message.info.is_from_me {
    //     let [_, b] = Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)])
    //         .areas(area);
    //     b
    // } else {
    //     let [a, _] = Layout::horizontal([Constraint::Percentage(70), Constraint::Percentage(30)])
    //         .areas(area);
    //     a
    // };

    // let [sender_area, quoted_area, content_area] = Layout::vertical([
    //     Constraint::Length(1),
    //     Constraint::Length(if quote_widget.is_some() { 1 } else { 0 }),
    //     Constraint::Min(1),
    // ])
    let [quoted_area, content_area] = Layout::vertical([
        Constraint::Length(if quote_widget.is_some() { 1 } else { 0 }),
        Constraint::Min(1),
    ])
    .areas(msg_area);

    msg_block.render(area, buf);
    // sender_widget.render(sender_area, buf);
    if let Some(quoted_widget) = quote_widget {
        quoted_widget.render(quoted_area, buf);
    }

    match &message.message {
        wr::MessageContent::Text(text) => {
            let lines = textwrap::wrap(text, content_area.width as usize)
                .iter()
                .map(|line| Line::raw(line.to_string()))
                .collect::<Vec<_>>();
            Paragraph::new(lines)
                .alignment(alignment)
                .render(content_area, buf);
        }
        wr::MessageContent::File(data) => {
            let content_height = file_content_height(&message.info.id, data, app);

            let [media_area, caption_area] = Layout::vertical([
                Constraint::Length(content_height as u16),
                Constraint::Min(0),
            ])
            .areas(content_area);

            match app.metadata.get(&message.info.id) {
                None => {
                    Paragraph::new(format!("🔗 {} +", data.path))
                        .alignment(alignment)
                        .render(media_area, buf);
                    app.tx
                        .send(AppInput::App(AppEvent::DownloadFile(
                            message.info.id.clone(),
                            data.file_id.clone(),
                        )))
                        .unwrap();
                }
                Some(Metadata::File(meta)) => match meta {
                    FileMeta::Downloaded => {
                        Paragraph::new(format!("🔗 {} ✓", data.path))
                            .alignment(alignment)
                            .render(media_area, buf);

                        if let FileKind::Image | FileKind::Sticker = data.kind {
                            let already_loading = matches!(
                                app.metadata.get(&message.info.id),
                                Some(Metadata::File(FileMeta::Loading))
                            );
                            if !already_loading {
                                app.tx
                                    .send(AppInput::App(AppEvent::LoadFilePreview(
                                        message.info.id.clone(),
                                    )))
                                    .unwrap();
                            }
                        }
                    }
                    FileMeta::Downloading => {
                        Paragraph::new(format!("🔗 {} downloading", data.path))
                            .alignment(alignment)
                            .render(media_area, buf);
                    }
                    FileMeta::DownloadFailed => {
                        Paragraph::new(format!("🔗 Failed to download {}", data.path))
                            .alignment(alignment)
                            .render(media_area, buf);
                    }
                    FileMeta::LoadFailed => {
                        Paragraph::new(format!("🔗 Failed to load {}", data.path))
                            .alignment(alignment)
                            .render(media_area, buf);
                    }
                    FileMeta::Loading => {
                        trace!("Rendering loading for {}", &message.info.id);
                        Paragraph::new(format!("🔗 {} loading", data.path))
                            .alignment(alignment)
                            .render(media_area, buf);
                    }
                    FileMeta::Loaded => match data.kind {
                        FileKind::Image | FileKind::Sticker => {
                            if !render_image || app.image_cache.get_mut(&data.path).is_none() {
                                Paragraph::new("🖼")
                                    .alignment(alignment)
                                    .render(media_area, buf);
                            } else if let Some(image) = app.image_cache.get_mut(&data.path) {
                                StatefulImage::default().render(media_area, buf, image);
                            } else {
                                Paragraph::new("🖼")
                                    .alignment(alignment)
                                    .render(media_area, buf);
                            }
                        }
                        FileKind::Video | FileKind::Audio | FileKind::Document => {
                            Paragraph::new(format!("🔗 {} ✓", data.path))
                                .alignment(alignment)
                                .render(media_area, buf);
                        }
                    },
                },
            };

            if let Some(caption) = &data.caption {
                let lines = textwrap::wrap(caption, content_area.width as usize)
                    .iter()
                    .map(|line| Line::raw(line.to_string()))
                    .collect::<Vec<_>>();
                Paragraph::new(lines)
                    .alignment(alignment)
                    .render(caption_area, buf);
            }
        }
    };
}

pub fn render_messages(frame: &mut Frame, app: &mut App, area: Rect) -> Option<()> {
    let chat_jid = app.selected_chat_jid.as_ref()?;

    let block = Block::bordered()
        .title(format!("Chat with {}", app.contact_name(chat_jid)))
        .title_bottom(format!("{:?}", app.key_buffer))
        .border_style(Style::default().fg(
            if let SelectedWidget::MessageList = app.selected_widget {
                ratatui::style::Color::Green
            } else {
                ratatui::style::Color::White
            },
        ));
    frame.render_widget(&block, area);

    let list_area = block.inner(area);
    if list_area.is_empty() {
        return Some(());
    }

    let items: Vec<_> = app
        .chat_messages
        .get(chat_jid)?
        .iter()
        .rev()
        .filter_map(|msg_id| app.messages.get(msg_id).cloned())
        .collect();

    if items.is_empty() {
        app.message_list_state.select(None);
        return Some(());
    }

    if app.message_list_state.selected.is_none()
        && app.message_list_state.selected_message.is_some()
    {
        let selected_message = app.message_list_state.selected_message.clone().unwrap();
        if let Some(idx) = items
            .iter()
            .position(|item| item.info.id == selected_message)
        {
            app.message_list_state.select(Some(idx));
        } else {
            app.message_list_state.select(None);
        }
    }

    if let Some(idx) = app.message_list_state.selected {
        if idx >= items.len() {
            app.message_list_state.selected = Some(items.len() - 1);
        }
    }

    let width = list_area.width as isize;
    let padding = 4;
    let gap = 1;

    app.message_list_state.selected_message = app
        .message_list_state
        .selected
        .map(|selected| items[selected].info.id.clone());

    if app.message_list_state.selected.is_some() && app.message_list_state.update_selected {
        let selected = app.message_list_state.selected.unwrap();
        app.message_list_state.update_selected = false;

        // Height to the bottom of selected msg
        let acc_height = items
            .iter()
            .take(selected)
            .map(|item| message_height(item, width as usize, app))
            .sum::<usize>()
            + gap * selected;

        let selected_height = message_height(&items[selected], width as usize, app);

        let low = acc_height < app.message_list_state.offset + padding;
        let high = acc_height + selected_height
            > app.message_list_state.offset + list_area.height as usize - padding;

        // if low && high {
        //     info!("idk");
        // } else if low {
        if low {
            app.message_list_state.offset = acc_height.saturating_sub(padding);
        } else if high {
            app.message_list_state.offset =
                (acc_height + selected_height + padding).saturating_sub(list_area.height as usize);
        }
    }

    let mut y = list_area.bottom() as isize + app.message_list_state.offset as isize;
    for (i, item) in items.iter().enumerate() {
        let height = message_height(item, width as usize, app) as isize;

        let bottom = y;
        let top = y - height;

        if bottom < list_area.top() as isize {
            break;
        }

        if top <= list_area.bottom() as isize {
            let is_selected = app.message_list_state.selected == Some(i);

            let too_low = top < list_area.top() as isize;
            let too_high = bottom > list_area.bottom() as isize;

            if too_low || too_high {
                let item_area = Rect::new(0, 0, width as u16, height as u16);
                let mut buf = Buffer::empty(item_area);

                let available_top = max(top, list_area.top() as isize) as u16;
                let available_bottom = min(bottom, list_area.bottom() as isize) as u16;
                let visible_buf_top = (available_top as isize - top) as u16;
                let visible_buf_height = available_bottom - available_top;

                // -- BEGIN AI IMPRESSIVE HACK --
                // Only render the image (and thus touch protocol state) when at least one image
                // row is in the visible slice. Otherwise we'd set "transmitted" but never send
                // any cell to the frame, and the image would never show when scrolled into view.
                let render_image = match &item.message {
                    wr::MessageContent::File(data)
                        if matches!(
                            app.metadata.get(&item.info.id),
                            Some(Metadata::File(FileMeta::Loaded))
                        ) && matches!(data.kind, FileKind::Image | FileKind::Sticker) =>
                    {
                        let image_top = 1 + if item.info.quote_id.is_some() { 1 } else { 0 };
                        let image_bottom = image_top + IMAGE_HEIGHT as u16;
                        let visible_buf_bottom = visible_buf_top + visible_buf_height;
                        visible_buf_top < image_bottom && visible_buf_bottom > image_top
                    }
                    _ => true,
                };
                // -- END AI IMPRESSIVE HACK --

                render_message(&mut buf, item, is_selected, app, item_area, render_image);

                let buf_area = Rect::new(
                    list_area.left(),
                    available_top,
                    width as u16,
                    visible_buf_height,
                );

                if !buf_area.is_empty() {
                    let mut mapped_area = buf_area;
                    mapped_area.y = visible_buf_top;
                    mapped_area.x = 0;

                    // -- BEGIN AI IMPRESSIVE HACK --
                    // When the visible slice doesn't include the image's first row, Kitty never
                    // receives the image transmit (it's in that first row's cell). Inject it into
                    // the first visible row's left cell so the image displays.
                    let (inject_transmit, media_first_row) = match &item.message {
                        wr::MessageContent::File(data)
                            if matches!(
                                app.metadata.get(&item.info.id),
                                Some(Metadata::File(FileMeta::Loaded))
                            ) && matches!(data.kind, FileKind::Image | FileKind::Sticker) =>
                        {
                            let first_row = 1 + if item.info.quote_id.is_some() { 1 } else { 0 };
                            let inject = mapped_area.y > first_row
                                && mapped_area.y < first_row + IMAGE_HEIGHT as u16;
                            (inject, first_row)
                        }
                        _ => (false, 0),
                    };

                    for (row_idx, (screen_row, msg_row)) in
                        buf_area.rows().zip(mapped_area.rows()).enumerate()
                    {
                        for (screen_col, msg_col) in screen_row.columns().zip(msg_row.columns()) {
                            let mut cell = buf[msg_col].clone();
                            if inject_transmit && row_idx == 0 && screen_col.x == list_area.left() {
                                let first_sym = buf[(0, media_first_row)].symbol();
                                if let Some(pos) = first_sym.find("\x1b[s") {
                                    let merged = format!("{}{}", &first_sym[..pos], cell.symbol());
                                    cell.set_symbol(&merged);
                                }
                            }
                            frame.buffer_mut()[screen_col] = cell;
                        }
                    }
                    // -- END AI IMPRESSIVE HACK --
                }
            } else {
                let item_area = Rect {
                    x: list_area.left(),
                    y: top as u16,
                    width: width as u16,
                    height: height as u16,
                };

                render_message(frame.buffer_mut(), item, is_selected, app, item_area, true);
            }
        }

        y -= height + gap as isize;
    }

    None
}

#[derive(Debug, Default, Clone, Eq, PartialEq, Hash)]
pub struct MessageListState {
    pub selected: Option<usize>,
    pub offset: usize,
    selected_message: Option<wr::MessageId>,
    pub update_selected: bool,
}

impl MessageListState {
    pub fn get_selected_message(&self) -> Option<wr::MessageId> {
        self.selected_message.clone()
    }
    pub fn set_selected_message(&mut self, msg_id: wr::MessageId) {
        self.selected_message = Some(msg_id);
        self.selected = None;
        self.update_selected = false;
    }
}

impl MessageListState {
    pub fn reset(&mut self) {
        self.selected = None;
        self.offset = 0;
        self.selected_message = None;
        self.update_selected = false;
    }

    pub fn select(&mut self, index: Option<usize>) {
        self.selected = index;
        if index.is_none() {
            self.offset = 0;
        } else {
            self.update_selected = true;
        }
    }
    pub fn select_next(&mut self) {
        let next = self.selected.map_or(0, |i| i.saturating_add(1));
        self.select(Some(next));
    }

    pub fn select_previous(&mut self) {
        let previous = self.selected.map_or(usize::MAX, |i| i.saturating_sub(1));
        self.select(Some(previous));
    }

    pub fn select_first(&mut self) {
        self.select(Some(0));
    }

    pub fn select_last(&mut self) {
        self.select(Some(usize::MAX));
    }

    pub fn scroll_down_by(&mut self, amount: u16) {
        let selected = self.selected.unwrap_or_default();
        self.select(Some(selected.saturating_add(amount as usize)));
    }

    pub fn scroll_up_by(&mut self, amount: u16) {
        let selected = self.selected.unwrap_or_default();
        self.select(Some(selected.saturating_sub(amount as usize)));
    }
}

pub fn get_quoted_text(msg: &wr::Message) -> Arc<str> {
    match &msg.message {
        wr::MessageContent::Text(text) => text.clone(),
        wr::MessageContent::File(data) => {
            format!("{}: {}", data.path, data.caption.as_deref().unwrap_or("")).into()
        }
    }
}
