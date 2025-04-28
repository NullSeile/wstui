use unicode_width::UnicodeWidthStr;

use crate::list::{ListDirection, WidgetList, WidgetListItem, WidgetListState};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{HighlightSpacing, StatefulWidget, Widget, block::BlockExt},
};

impl<T: WidgetListItem> Widget for WidgetList<'_, T> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut state = WidgetListState::default();
        StatefulWidget::render(self, area, buf, &mut state);
    }
}

impl<T: WidgetListItem> StatefulWidget for WidgetList<'_, T> {
    type State = WidgetListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        buf.set_style(area, self.style);
        self.block.render(area, buf);
        let list_area = self.block.inner_if_some(area);

        if list_area.is_empty() {
            return;
        }

        if self.items.is_empty() {
            state.select(None);
            return;
        }

        // If the selected index is out of bounds, set it to the last item
        if state.selected.is_some_and(|s| s >= self.items.len()) {
            state.select(Some(self.items.len().saturating_sub(1)));
        }

        let list_height = list_area.height as usize;
        let list_width = list_area.width as usize;

        let (first_visible_index, last_visible_index) =
            self.get_items_bounds(state.selected, state.offset, list_height, list_width);

        // Important: this changes the state's offset to be the beginning of the now viewable items
        state.offset = first_visible_index;

        // Get our set highlighted symbol (if one was set)
        let highlight_symbol = self.highlight_symbol.unwrap_or("");
        let blank_symbol = " ".repeat(highlight_symbol.width());

        let mut current_height = 0;

        let selection_spacing = match self.highlight_spacing {
            HighlightSpacing::Always => true,
            HighlightSpacing::WhenSelected => state.selected.is_some(),
            HighlightSpacing::Never => false,
        };
        for (i, item) in self
            .items
            .iter()
            .enumerate()
            .skip(state.offset)
            .take(last_visible_index - first_visible_index)
        {
            let (x, y) = if self.direction == ListDirection::BottomToTop {
                current_height += item.height(list_width) as u16;
                (list_area.left(), list_area.bottom() - current_height)
            } else {
                let pos = (list_area.left(), list_area.top() + current_height);
                current_height += item.height(list_width) as u16;
                pos
            };

            let row_area = Rect {
                x,
                y,
                width: list_area.width,
                height: item.height(list_width) as u16,
            };

            // let item_style = self.style.patch(item.style);
            let item_style = self.style;
            buf.set_style(row_area, item_style);

            let is_selected = state.selected.map_or(false, |s| s == i);

            let item_area = if selection_spacing {
                let highlight_symbol_width = self.highlight_symbol.unwrap_or("").width() as u16;
                Rect {
                    x: row_area.x + highlight_symbol_width,
                    width: row_area.width.saturating_sub(highlight_symbol_width),
                    ..row_area
                }
            } else {
                row_area
            };
            item.clone().render(item_area, buf);

            // for j in 0..item.content.height() {
            //     // if the item is selected, we need to display the highlight symbol:
            //     // - either for the first line of the item only,
            //     // - or for each line of the item if the appropriate option is set
            //     let symbol = if is_selected && (j == 0 || self.repeat_highlight_symbol) {
            //         highlight_symbol
            //     } else {
            //         &blank_symbol
            //     };
            //     if selection_spacing {
            //         buf.set_stringn(
            //             x,
            //             y + j as u16,
            //             symbol,
            //             list_area.width as usize,
            //             item_style,
            //         );
            //     }
            // }

            if is_selected {
                buf.set_style(row_area, self.highlight_style);
            }
        }
    }
}

impl<T: WidgetListItem> WidgetList<'_, T> {
    /// Given an offset, calculate which items can fit in a given area
    fn get_items_bounds(
        &self,
        selected: Option<usize>,
        offset: usize,
        max_height: usize,
        list_width: usize,
    ) -> (usize, usize) {
        let offset = offset.min(self.items.len().saturating_sub(1));

        // Note: visible here implies visible in the given area
        let mut first_visible_index = offset;
        let mut last_visible_index = offset;

        // Current height of all items in the list to render, beginning at the offset
        let mut height_from_offset = 0;

        // Calculate the last visible index and total height of the items
        // that will fit in the available space
        for item in self.items.iter().skip(offset) {
            if height_from_offset + item.height(list_width) > max_height {
                break;
            }

            height_from_offset += item.height(list_width);

            last_visible_index += 1;
        }

        // Get the selected index and apply scroll_padding to it, but still honor the offset if
        // nothing is selected. This allows for the list to stay at a position after select()ing
        // None.
        let index_to_display = self
            .apply_scroll_padding_to_selected_index(
                selected,
                max_height,
                list_width,
                first_visible_index,
                last_visible_index,
            )
            .unwrap_or(offset);

        // Recall that last_visible_index is the index of what we
        // can render up to in the given space after the offset
        // If we have an item selected that is out of the viewable area (or
        // the offset is still set), we still need to show this item
        while index_to_display >= last_visible_index {
            height_from_offset = height_from_offset
                .saturating_add(self.items[last_visible_index].height(list_width));

            last_visible_index += 1;

            // Now we need to hide previous items since we didn't have space
            // for the selected/offset item
            while height_from_offset > max_height {
                height_from_offset = height_from_offset
                    .saturating_sub(self.items[first_visible_index].height(list_width));

                // Remove this item to view by starting at the next item index
                first_visible_index += 1;
            }
        }

        // Here we're doing something similar to what we just did above
        // If the selected item index is not in the viewable area, let's try to show the item
        while index_to_display < first_visible_index {
            first_visible_index -= 1;

            height_from_offset = height_from_offset
                .saturating_add(self.items[first_visible_index].height(list_width));

            // Don't show an item if it is beyond our viewable height
            while height_from_offset > max_height {
                last_visible_index -= 1;

                height_from_offset = height_from_offset
                    .saturating_sub(self.items[last_visible_index].height(list_width));
            }
        }

        (first_visible_index, last_visible_index)
    }

    /// Applies scroll padding to the selected index, reducing the padding value to keep the
    /// selected item on screen even with items of inconsistent sizes
    ///
    /// This function is sensitive to how the bounds checking function handles item height
    fn apply_scroll_padding_to_selected_index(
        &self,
        selected: Option<usize>,
        max_height: usize,
        list_width: usize,
        first_visible_index: usize,
        last_visible_index: usize,
    ) -> Option<usize> {
        let last_valid_index = self.items.len().saturating_sub(1);
        let selected = selected?.min(last_valid_index);

        // The bellow loop handles situations where the list item sizes may not be consistent,
        // where the offset would have excluded some items that we want to include, or could
        // cause the offset value to be set to an inconsistent value each time we render.
        // The padding value will be reduced in case any of these issues would occur
        let mut scroll_padding = self.scroll_padding;
        while scroll_padding > 0 {
            let mut height_around_selected = 0;
            for index in selected.saturating_sub(scroll_padding)
                ..=selected
                    .saturating_add(scroll_padding)
                    .min(last_valid_index)
            {
                height_around_selected += self.items[index].height(list_width);
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
}
