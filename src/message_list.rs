use std::sync::Arc;

use chrono::{DateTime, Datelike, Local};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::Line,
    widgets::{Block, Paragraph},
};
use ratatui_image::StatefulImage;
use textwrap;
use whatsrust as wr;

use crate::{App, AppEvent, FileMeta, Metadata};

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
        wr::MessageContent::Image(data) => {
            let lines = if let Some(caption) = &data.caption {
                textwrap::wrap(caption, width).len()
            } else {
                0
            };

            let content_height = match app.metadata.get(&message.info.id) {
                None => 1,
                Some(Metadata::File(meta)) => match meta {
                    FileMeta::Failed => 1,
                    FileMeta::Downloaded => 12,
                },
            };

            content_height + lines
        }
    };

    header_height + content_height + 1
}

fn render_message(
    frame: &mut Frame,
    message: &wr::Message,
    is_selected: bool,
    app: &mut App,
    area: Rect,
) {
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

    let sender_name = if let Some(sender_chat) = app.chats.get(&message.info.sender) {
        sender_chat.get_name()
    } else {
        message.info.sender.clone().into()
    };

    let mut header = vec![
        sender_name.to_string().into(),
        " (".into(),
        timestamp,
        ")".into(),
    ];
    if message.info.is_read {
        header.push(" ✓".into());
    }
    let sender_widget = Line::from_iter(header).bold();

    let quoted_text = message.info.quote_id.as_ref().and_then(|quote_id| {
        let chat_messages = app.messages.get(app.selected_chat_jid.as_ref().unwrap());
        chat_messages.and_then(|chat_messages| chat_messages.get(quote_id).map(get_quoted_text))
    });
    let quote_widget = message.info.quote_id.as_ref().map(|_quote_id| {
        let quoted_text = quoted_text.unwrap_or_else(|| "not found".into());

        let line = Line::from(format!("> {quoted_text}"));
        if is_selected {
            line.dark_gray()
        } else {
            line.gray()
        }
    });

    let [_padding, sender_area, quoted_area, content_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(if quote_widget.is_some() { 1 } else { 0 }),
        Constraint::Min(1),
    ])
    .areas(area);

    frame.render_widget(sender_widget, sender_area);
    if let Some(quoted_widget) = quote_widget {
        frame.render_widget(quoted_widget, quoted_area);
    }

    match &message.message {
        wr::MessageContent::Text(text) => {
            let lines = textwrap::wrap(text, content_area.width as usize)
                .iter()
                .map(|line| Line::raw(line.to_string()))
                .collect::<Vec<_>>();
            frame.render_widget(Paragraph::new(lines), content_area);
        }
        wr::MessageContent::Image(data) => {
            let content_height = match app.metadata.get(&message.info.id) {
                None => 1,
                Some(Metadata::File(meta)) => match meta {
                    FileMeta::Failed => 1,
                    FileMeta::Downloaded => 12,
                },
            };

            let [img_area, caption_area] =
                Layout::vertical([Constraint::Length(content_height), Constraint::Min(0)])
                    .areas(content_area);

            match app.metadata.get(&message.info.id) {
                None => {
                    frame
                        .render_widget(Paragraph::new(format!("🔗 {} +", data.path)), content_area);
                    app.event_queue.lock().unwrap().push(AppEvent::DownloadFile(
                        message.info.id.clone(),
                        data.file_id.clone(),
                    ));
                }
                Some(Metadata::File(meta)) => match meta {
                    FileMeta::Failed => {
                        frame.render_widget(
                            Paragraph::new("🔗 Failed to download image"),
                            content_area,
                        );
                    }
                    FileMeta::Downloaded => {
                        let image = app.image_cache.entry(data.path.clone()).or_insert_with(|| {
                            let binding = data.path.to_string();
                            let image_path = std::path::Path::new(&binding);

                            let image_src = image::ImageReader::open(image_path)
                                .unwrap()
                                .decode()
                                .unwrap();

                            app.picker.new_resize_protocol(image_src)
                        });
                        frame.render_stateful_widget(StatefulImage::default(), img_area, image);
                    }
                },
            };

            if let Some(caption) = &data.caption {
                let lines = textwrap::wrap(caption, content_area.width as usize)
                    .iter()
                    .map(|line| Line::raw(line.to_string()))
                    .collect::<Vec<_>>();
                frame.render_widget(Paragraph::new(lines), caption_area);
            }
        }
    };
}

pub fn render_messages(frame: &mut Frame, app: &mut App, area: Rect) -> Option<()> {
    let chat_jid = app.selected_chat_jid.as_ref()?;

    let contact = app.chats.get(chat_jid).unwrap();
    let block = Block::bordered().title(format!("Chat with {}", contact.get_name()));
    frame.render_widget(&block, area);

    let list_area = block.inner(area);
    if list_area.is_empty() {
        return Some(());
    }

    let chat_messages = app.messages.get(chat_jid)?;

    let mut items = chat_messages.values().cloned().collect::<Vec<_>>();
    items.sort_by(|a, b| b.info.timestamp.cmp(&a.info.timestamp));

    if items.is_empty() {
        app.message_list_state.select(None);
        return Some(());
    }

    // If the selected index is out of bounds, set it to the last item
    if app
        .message_list_state
        .selected
        .is_some_and(|s| s >= items.len())
    {
        app.message_list_state
            .select(Some(items.len().saturating_sub(1)));
    }

    let list_height = list_area.height as usize;
    let list_width = list_area.width as usize;

    let (first_visible_index, last_visible_index) = get_items_bounds(
        app.message_list_state.selected,
        app.message_list_state.offset,
        list_height,
        list_width,
        app,
        &items,
    );

    // Important: this changes the state's offset to be the beginning of the now viewable items
    app.message_list_state.offset = first_visible_index;

    let mut current_height = 0;

    for (i, item) in items
        .iter()
        .enumerate()
        .skip(app.message_list_state.offset)
        .take(last_visible_index - first_visible_index)
    {
        let item_height = message_height(item, list_width, app);

        let (x, y) = {
            current_height += item_height as u16;
            (
                list_area.left(),
                // list_area.bottom().saturating_sub(current_height),
                list_area.bottom() - current_height,
            )
        };

        let row_area = Rect {
            x,
            y,
            width: list_area.width,
            // height: (item_height as u16).min(y),
            height: item_height as u16,
        };

        if app.message_list_state.selected == Some(i)
            && app.message_list_state.selected_message == Some(item.info.id.clone())
        {
            app.message_list_state.offset = i;
        }

        let is_selected = app.message_list_state.selected == Some(i);
        if is_selected {
            app.message_list_state.selected_message = Some(item.info.id.clone())
        };

        let item_area = row_area;

        if is_selected {
            let style = Style::default()
                .bg(ratatui::style::Color::Gray)
                .fg(ratatui::style::Color::Black);
            let mut message_area = item_area;
            message_area.y += 1;
            message_area.height -= 1;
            frame.buffer_mut().set_style(message_area, style);
        }

        render_message(frame, item, is_selected, app, item_area);
    }
    None
}

fn get_items_bounds(
    selected: Option<usize>,
    offset: usize,
    max_height: usize,
    list_width: usize,
    app: &mut App,
    items: &[wr::Message],
) -> (usize, usize) {
    let offset = offset.min(items.len().saturating_sub(1));

    // Note: visible here implies visible in the given area
    let mut first_visible_index = offset;
    let mut last_visible_index = offset;

    // Current height of all items in the list to render, beginning at the offset
    let mut height_from_offset = 0;

    // Calculate the last visible index and total height of the items
    // that will fit in the available space
    for item in items.iter().skip(offset) {
        let item_height = message_height(item, list_width, app);
        if height_from_offset + item_height > max_height {
            // if height_from_offset > max_height {
            break;
        }

        height_from_offset += item_height;
        last_visible_index += 1;
    }

    // Get the selected index and apply scroll_padding to it, but still honor the offset if
    // nothing is selected. This allows for the list to stay at a position after select()ing
    // None.
    let index_to_display = apply_scroll_padding_to_selected_index(
        selected,
        max_height,
        list_width,
        first_visible_index,
        last_visible_index,
        app,
        items,
    )
    .unwrap_or(offset);

    // Recall that last_visible_index is the index of what we
    // can render up to in the given space after the offset
    // If we have an item selected that is out of the viewable area (or
    // the offset is still set), we still need to show this item
    while index_to_display >= last_visible_index {
        height_from_offset = height_from_offset.saturating_add(message_height(
            &items[last_visible_index],
            list_width,
            app,
        ));

        last_visible_index += 1;

        // Now we need to hide previous items since we didn't have space
        // for the selected/offset item
        while height_from_offset > max_height {
            height_from_offset = height_from_offset.saturating_sub(message_height(
                &items[first_visible_index],
                list_width,
                app,
            ));

            // Remove this item to view by starting at the next item index
            first_visible_index += 1;
        }
    }

    // Here we're doing something similar to what we just did above
    // If the selected item index is not in the viewable area, let's try to show the item
    while index_to_display < first_visible_index {
        first_visible_index -= 1;

        height_from_offset = height_from_offset.saturating_add(message_height(
            &items[first_visible_index],
            list_width,
            app,
        ));

        // Don't show an item if it is beyond our viewable height
        while height_from_offset > max_height {
            last_visible_index -= 1;

            height_from_offset = height_from_offset.saturating_sub(message_height(
                &items[last_visible_index],
                list_width,
                app,
            ));
        }
    }

    (first_visible_index, last_visible_index)
}

fn apply_scroll_padding_to_selected_index(
    selected: Option<usize>,
    max_height: usize,
    list_width: usize,
    first_visible_index: usize,
    last_visible_index: usize,
    app: &mut App,
    items: &[wr::Message],
) -> Option<usize> {
    let last_valid_index = items.len().saturating_sub(1);
    let selected = selected?.min(last_valid_index);

    // The bellow loop handles situations where the list item sizes may not be consistent,
    // where the offset would have excluded some items that we want to include, or could
    // cause the offset value to be set to an inconsistent value each time we render.
    // The padding value will be reduced in case any of these issues would occur
    let mut scroll_padding = 1;
    while scroll_padding > 0 {
        let mut height_around_selected = 0;
        for index in selected.saturating_sub(scroll_padding)
            ..=selected
                .saturating_add(scroll_padding)
                .min(last_valid_index)
        {
            height_around_selected += message_height(&items[index], list_width, app);
        }
        if height_around_selected <= max_height {
            break;
        }
        scroll_padding -= 1;
    }

    Some(
        if (selected + scroll_padding).min(last_valid_index) >= last_visible_index {
            selected + scroll_padding
        } else if selected.saturating_sub(scroll_padding) < first_visible_index {
            selected.saturating_sub(scroll_padding)
        } else {
            selected
        }
        .min(last_valid_index),
    )
}

#[derive(Debug, Default, Clone, Eq, PartialEq, Hash)]
pub struct MessageListState {
    pub selected: Option<usize>,
    pub offset: usize,
    selected_message: Option<wr::MessageId>,
}

impl MessageListState {
    pub fn get_selected_message(&self) -> Option<wr::MessageId> {
        self.selected_message.clone()
    }
}

impl MessageListState {
    pub fn select(&mut self, index: Option<usize>) {
        self.selected = index;
        if index.is_none() {
            self.offset = 0;
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

fn get_quoted_text(msg: &wr::Message) -> Arc<str> {
    match &msg.message {
        wr::MessageContent::Text(text) => text.clone(),
        wr::MessageContent::Image(wr::ImageContent {
            path,
            file_id: _,
            caption,
        }) => format!("Image: {path} Caption: {caption:?}").into(),
    }
}
