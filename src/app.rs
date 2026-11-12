//! The Elm architecture: `App` (state) + `Message` -> `update` -> `view`.
//!
//! `update` never blocks: anything slow is a `Command` sent to the worker thread,
//! and results come back as `Message::Worker(Event)` through the subscription.

use std::collections::HashMap;
use std::sync::mpsc::SyncSender;

use iced::widget::{
    center, column, container, grid, image, progress_bar, row, scrollable, space, stack, text,
};
use iced::{Alignment, Element, Length, Subscription, Theme};
use tracing::warn;

use crate::transfer::source::{DeviceStatus, MediaId, MediaItem, MediaKind};
use crate::transfer::worker::{self, Command, Event};
use crate::ui;

pub struct App {
    commands: Option<SyncSender<Command>>,
    screen: Screen,
    device_name: String,
    entries: Vec<Entry>,
    thumbnails: HashMap<MediaId, image::Handle>,
}

struct Entry {
    item: MediaItem,
    selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Screen {
    Waiting,
    NeedsFileTransfer,
    Loading,
    Choose,
    Copying {
        done: usize,
        total: usize,
    },
    Done {
        copied: usize,
        failed: usize,
        cancelled: bool,
    },
    Problem(String),
}

#[derive(Debug, Clone)]
pub enum Message {
    Worker(Event),
    Toggle(usize),
    SelectAll,
    SelectNone,
    StartCopy,
    Cancel,
    CopyMore,
}

impl App {
    pub fn new() -> Self {
        Self {
            commands: None,
            screen: Screen::Waiting,
            device_name: String::new(),
            entries: Vec::new(),
            thumbnails: HashMap::new(),
        }
    }

    pub fn title(&self) -> String {
        "Copy photos from phone".to_string()
    }

    pub fn theme(&self) -> Theme {
        Theme::Light
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::run(worker::demo_events).map(Message::Worker)
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Worker(event) => self.on_worker_event(event),
            Message::Toggle(index) => {
                if let Some(entry) = self.entries.get_mut(index) {
                    entry.selected = !entry.selected;
                }
            }
            Message::SelectAll => self.entries.iter_mut().for_each(|e| e.selected = true),
            Message::SelectNone => self.entries.iter_mut().for_each(|e| e.selected = false),
            Message::StartCopy => {
                let chosen: Vec<MediaItem> = self
                    .entries
                    .iter()
                    .filter(|e| e.selected)
                    .map(|e| e.item.clone())
                    .collect();
                if !chosen.is_empty() {
                    self.screen = Screen::Copying {
                        done: 0,
                        total: chosen.len(),
                    };
                    self.send(Command::Transfer(chosen));
                }
            }
            Message::Cancel => self.send(Command::Cancel),
            Message::CopyMore => {
                self.screen = if self.entries.is_empty() {
                    Screen::Waiting
                } else {
                    Screen::Choose
                };
            }
        }
    }

    fn on_worker_event(&mut self, event: Event) {
        match event {
            Event::Ready(commands) => self.commands = Some(commands),
            Event::Status(status) => self.on_status(status),
            Event::MediaListed(items) => {
                let ids = items.iter().map(|item| item.id.clone()).collect();
                self.entries = items
                    .into_iter()
                    .map(|item| Entry {
                        item,
                        selected: true,
                    })
                    .collect();
                self.screen = Screen::Choose;
                self.send(Command::LoadThumbnails(ids));
            }
            Event::ScanFailed(error) => {
                warn!(error, "scan failed");
                self.screen = Screen::Problem(
                    "The photos on the phone could not be read. Unplug the phone and plug it in again."
                        .to_string(),
                );
            }
            Event::Thumbnail(id, bytes) => {
                self.thumbnails.insert(id, image::Handle::from_bytes(bytes));
            }
            Event::Progress { done, total } => {
                if matches!(self.screen, Screen::Copying { .. }) {
                    self.screen = Screen::Copying { done, total };
                }
            }
            Event::ItemFailed { file_name, error } => {
                warn!(file_name, error, "item failed");
            }
            Event::TransferDone {
                copied,
                failed,
                cancelled,
            } => {
                self.screen = Screen::Done {
                    copied,
                    failed,
                    cancelled,
                };
            }
        }
    }

    fn on_status(&mut self, status: DeviceStatus) {
        // Keep the "All done" message on screen even if the phone is unplugged.
        let keep_screen = matches!(self.screen, Screen::Done { .. } | Screen::Copying { .. });
        match status {
            DeviceStatus::NotConnected => {
                self.entries.clear();
                self.thumbnails.clear();
                if !keep_screen {
                    self.screen = Screen::Waiting;
                }
            }
            DeviceStatus::NeedsFileTransferMode { name } => {
                self.device_name = name;
                if !keep_screen {
                    self.screen = Screen::NeedsFileTransfer;
                }
            }
            DeviceStatus::Ready { name } => {
                self.device_name = name;
                if !keep_screen {
                    self.screen = Screen::Loading;
                }
            }
        }
    }

    /// Hands a command to the worker without blocking the UI.
    fn send(&self, command: Command) {
        match &self.commands {
            Some(commands) => {
                if let Err(error) = commands.try_send(command) {
                    warn!(%error, "could not send command to worker");
                }
            }
            None => warn!("worker not ready yet"),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let content: Element<'_, Message> = match &self.screen {
            Screen::Waiting => message_screen(
                "Plug your phone into the computer",
                "Use the phone's charging cable.",
            ),
            Screen::NeedsFileTransfer => message_screen(
                "Almost there!",
                "On the phone:\n\n1.  Unlock the screen.\n2.  Swipe down from the top.\n3.  Tap the message about USB.\n4.  Choose \"File transfer\".",
            ),
            Screen::Loading => message_screen(
                format!("Looking for photos on {}…", self.device_name),
                "This can take a moment.",
            ),
            Screen::Choose => self.choose_screen(),
            Screen::Copying { done, total } => copying_screen(*done, *total),
            Screen::Done {
                copied,
                failed,
                cancelled,
            } => done_screen(*copied, *failed, *cancelled),
            Screen::Problem(message) => message_screen("Something went wrong", message.as_str()),
        };
        container(content)
            .padding(40)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn choose_screen(&self) -> Element<'_, Message> {
        let selected = self.entries.iter().filter(|e| e.selected).count();

        let tiles = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| self.tile(index, entry));
        let grid = grid(tiles).fluid(260).spacing(ui::SPACING);

        let copy_label = if selected == 0 {
            "Click the photos you want to copy".to_string()
        } else {
            format!("Copy {} to the computer", ui::photos(selected))
        };

        column![
            ui::title("Choose the photos to copy"),
            ui::body(
                "Click a photo to choose it or leave it out. Chosen photos have a green border."
            ),
            row![
                ui::secondary_button("Choose all", Some(Message::SelectAll)).width(Length::Shrink),
                ui::secondary_button("Choose none", Some(Message::SelectNone))
                    .width(Length::Shrink),
            ]
            .spacing(ui::SPACING),
            scrollable(grid).height(Length::Fill).spacing(12),
            ui::main_button(copy_label, (selected > 0).then_some(Message::StartCopy)),
        ]
        .spacing(ui::SPACING)
        .into()
    }

    fn tile<'a>(&'a self, index: usize, entry: &'a Entry) -> Element<'a, Message> {
        let picture: Element<'a, Message> = match self.thumbnails.get(&entry.item.id) {
            Some(handle) => image(handle.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            None => center(ui::body("…")).into(),
        };

        let mut labels = column![].spacing(4);
        if entry.item.kind == MediaKind::Video {
            labels = labels.push(badge("Video"));
        }
        if entry.selected {
            labels = labels.push(badge("Chosen"));
        }

        let selected = entry.selected;
        iced::widget::button(stack![picture, container(labels).padding(8)])
            .padding(8)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |theme, status| ui::tile_style(theme, status, selected))
            .on_press(Message::Toggle(index))
            .into()
    }
}

fn badge(label: &str) -> Element<'_, Message> {
    container(text(label).size(22))
        .padding([4, 10])
        .style(container::rounded_box)
        .into()
}

fn message_screen<'a>(
    heading: impl text::IntoFragment<'a>,
    details: impl text::IntoFragment<'a>,
) -> Element<'a, Message> {
    center(
        column![ui::title(heading), ui::body(details)]
            .spacing(ui::SPACING)
            .max_width(800),
    )
    .into()
}

fn copying_screen<'a>(done: usize, total: usize) -> Element<'a, Message> {
    center(
        column![
            ui::title("Copying photos…"),
            progress_bar(0.0..=total as f32, done as f32).girth(40),
            ui::body(format!(
                "{done} of {total} copied. Keep the phone plugged in."
            )),
            ui::secondary_button("Stop", Some(Message::Cancel)).width(Length::Shrink),
        ]
        .spacing(ui::SPACING)
        .max_width(800)
        .align_x(Alignment::Center),
    )
    .into()
}

fn done_screen<'a>(copied: usize, failed: usize, cancelled: bool) -> Element<'a, Message> {
    let heading = if cancelled { "Stopped" } else { "All done!" };
    let mut details = format!("{} copied to the computer.", ui::photos(copied));
    if failed > 0 {
        details.push_str(&format!(
            "\n{} could not be copied. Check the phone is plugged in and try again.",
            ui::photos(failed)
        ));
    }
    center(
        column![
            ui::title(heading),
            ui::body(details),
            space().height(ui::SPACING),
            ui::main_button("Copy more photos", Some(Message::CopyMore)).width(Length::Shrink),
        ]
        .spacing(ui::SPACING)
        .max_width(800)
        .align_x(Alignment::Center),
    )
    .into()
}
