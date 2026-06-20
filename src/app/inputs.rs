use log::error;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind};

use crate::app::{App, SelectedWidget};
use crate::key_handler::Key;
use whatsrust as wr;

impl App<'_> {
    pub fn on_terminal_event(&mut self, event: Event) {
        if let Event::Key(key_event) = event
            && key_event.kind == KeyEventKind::Press
        {
            let key = self.kh.pressed_start(&key_event);

            self.handle_input(key);

            self.kh.pressed_end();
        }
    }

    fn handle_input(&mut self, key: Key) {
        if self.kh.kp(&[Key::ctrl('q')]) {
            self.db_handler.stop();
            self.should_quit = true;
            return;
        }

        if self.kh.kp(&[Key::ctrl_shift('l')]) {
            self.show_logs = !self.show_logs;
            return;
        }

        // if self.kh.kp(&[Key::ctrl('p')]) {
        //     let next = {
        //         let mut picker = self.picker.lock().unwrap();
        //         let current = picker.protocol_type();
        //         let next = if current == ProtocolType::Halfblocks {
        //             self.default_protocol_type
        //         } else {
        //             ProtocolType::Halfblocks
        //         };
        //         picker.set_protocol_type(next);
        //         next
        //     };
        //     self.image_cache.clear();
        //     for (message_id, meta) in self.metadata.iter_mut() {
        //         if let Metadata::File(FileMeta::Loaded) = meta {
        //             if let Some(msg) = self.messages.get(message_id) {
        //                 if let wr::MessageContent::File(file) = &msg.message {
        //                     if matches!(
        //                         file.kind,
        //                         wr::FileKind::Image | wr::FileKind::Sticker
        //                     ) {
        //                         *meta = Metadata::File(FileMeta::Downloaded);
        //                     }
        //                 }
        //             }
        //         }
        //     }
        //     debug!("Image protocol: {:?}", next);
        //     return;
        // }

        match self.selected_widget {
            SelectedWidget::ChatList => {
                if self.kh.kp(&[Key::ctrl('l')]) {
                    self.selected_widget = SelectedWidget::MessageList;
                    self.input_widget.select_all();
                    return;
                }
            }
            SelectedWidget::Input => {
                if self.kh.kp(&[Key::ctrl('k')]) {
                    self.selected_widget = SelectedWidget::MessageList;
                    return;
                }
                if self.kh.kp(&[Key::ctrl('h')]) {
                    self.selected_widget = SelectedWidget::ChatList;
                    return;
                }
            }
            SelectedWidget::MessageList => {
                if self.kh.kp(&[Key::ctrl('j')]) {
                    self.selected_widget = SelectedWidget::Input;
                    return;
                }
                if self.kh.kp(&[Key::ctrl('h')]) {
                    self.selected_widget = SelectedWidget::ChatList;
                    return;
                }
            }
            SelectedWidget::MessageView => {
                if self.kh.kp(&[Key::k(KeyCode::Esc)]) {
                    self.selected_widget = SelectedWidget::MessageList;
                    return;
                }
            }
        }

        match self.selected_widget {
            SelectedWidget::ChatList => {
                self.chat_list_on_event(&key);
            }
            SelectedWidget::MessageList => {
                self.message_list_on_event();
            }
            SelectedWidget::Input => {
                self.input_on_event(&key);
            }
            SelectedWidget::MessageView => {}
        }
    }

    fn chat_list_on_event(&mut self, key: &Key) {
        if self.kh.kp(&[Key::k(KeyCode::Esc)]) {
            let chat_jid = self.get_selected_chat();

            self.contact_search_active = false;
            self.contact_search.clean();

            self.select_chat(chat_jid);

            return;
        }

        if !self.contact_search_active {
            let mut moved = false;
            if self.kh.kp(&[Key::c('j')]) {
                self.chat_list_state.select_next();
                moved = true;
            } else if self.kh.kp(&[Key::c('k')]) {
                self.chat_list_state.select_previous();
                moved = true;
            }
            if moved {
                // Bound the selected index to the number of chats
                let len = self.sorted_chats.len();
                if len == 0 {
                    self.chat_list_state.select(None);
                    return;
                } else if let Some(selected) = self.chat_list_state.selected() {
                    if selected >= len {
                        self.chat_list_state.select(Some(len.saturating_sub(1)));
                    }
                }

                self.sort_chat_messages(self.get_selected_chat().unwrap());
                self.message_list_state.reset();
            }

            if self.kh.kp(&[Key::k(KeyCode::Enter)]) {
                if self.chat_list_state.selected().is_some() {
                    self.message_list_state.reset();
                    self.selected_widget = SelectedWidget::Input;
                }
            } else if self.kh.kp(&[Key::c('/')]) {
                self.contact_search_active = true;
            }
        } else {
            match key.code {
                // KeyCode::Enter => self.contact_search_active.submit_message(),
                KeyCode::Char(to_insert) => {
                    self.contact_search.enter_char(to_insert);
                    self.update_filtered_chats();
                }
                KeyCode::Backspace => {
                    self.contact_search.delete_char();
                    self.update_filtered_chats();
                }
                KeyCode::Left => self.contact_search.move_cursor_left(),
                KeyCode::Right => self.contact_search.move_cursor_right(),
                KeyCode::Enter => {
                    self.contact_search_active = false;
                }
                _ => {}
            }
        }
    }

    fn message_list_on_event(&mut self) {
        if self.kh.kp(&[Key::ctrl('e')]) {
            self.message_list_state.offset = self.message_list_state.offset.saturating_sub(1);
        } else if self.kh.kp(&[Key::ctrl('y')]) {
            self.message_list_state.offset = self.message_list_state.offset.saturating_add(1);
        } else if self.kh.kp(&[Key::c('k')]) {
            self.message_list_state.select_next();
        } else if self.kh.kp(&[Key::c('j')]) {
            self.message_list_state.select_previous();
        } else if self.kh.kp(&[Key::c('G')]) {
            self.message_list_state.select_first();
        } else if self.kh.kp(&[Key::c('g'), Key::c('g')]) {
            self.message_list_state.select_last();
        } else if self.kh.kp(&[Key::k(KeyCode::Esc)]) {
            self.message_list_state.reset();
        }

        if let Some(msg_id) = self.message_list_state.get_selected_message()
            && let Some(msg) = self.messages.get(&msg_id).cloned()
        {
            if self.kh.kp(&[Key::c('o')]) {
                match &msg.message {
                    wr::MessageContent::Text(_text) => {
                        // let mut file = tempfile::tempfile().unwrap();
                        //
                        // file.write_all(text.as_bytes()).unwrap();
                        //
                        // open::that(file.).unwrap();
                    }
                    wr::MessageContent::File(content) => {
                        match open::that(self.media_path.join(content.path.as_ref())) {
                            Ok(_) => {}
                            Err(e) => {
                                error!("Failed to open file {}: {:?}", content.path, e);
                            }
                        }
                    }
                }
            } else if self.kh.kp(&[Key::c('r')]) {
                self.quoting_message = Some(msg.clone());
                self.selected_widget = SelectedWidget::Input;
            } else if self.kh.kp(&[Key::k(KeyCode::Enter)]) {
                self.selected_widget = SelectedWidget::MessageView;
            } else if self.kh.kp(&[Key::c('y')]) {
                match &msg.message {
                    wr::MessageContent::Text(text) => {
                        if let Err(e) = self.clipboard.set_text(text.to_string()) {
                            error!("Failed to copy text to clipboard: {:?}", e);
                        }
                    }
                    wr::MessageContent::File(content) => {
                        let path = self.media_path.join(content.path.as_ref());
                        if let Err(e) = self.clipboard.set_text(path.to_string_lossy().into_owned())
                        {
                            error!("Failed to copy file to clipboard: {:?}", e);
                        }
                    }
                }
            }

            if let Some(ref quote_id) = msg.info.quote_id {
                if self.kh.kp(&[Key::c('g'), Key::c('q')]) {
                    self.message_list_state
                        .set_selected_message(quote_id.clone());
                }
            }
        }
    }
}
