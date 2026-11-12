//! The worker thread: owns the `MediaSource` and does all blocking work.
//!
//! ```text
//!  UI (iced)  --Command (std sync_channel)-->  worker thread  --Event (tokio mpsc)-->  UI
//! ```
//!
//! Commands use `std::sync::mpsc::sync_channel` because the worker needs
//! `recv_timeout` to wake up and poll for the phone. Events use `tokio::sync::mpsc`
//! so the iced subscription can `.await` them. Both channels are bounded.
//!
//! The source is *created* on the worker thread (via `make_source`) because the real
//! Windows device objects must stay on the thread that created them.

use std::collections::VecDeque;
use std::io;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use iced::futures::{SinkExt, Stream};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::TransferError;
use super::fake::FakeSource;
use super::source::{DeviceStatus, MediaId, MediaItem, MediaSource};

const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Requests from the UI.
#[derive(Debug, Clone)]
pub enum Command {
    LoadThumbnails(Vec<MediaId>),
    Transfer(Vec<MediaItem>),
    Cancel,
}

/// Reports to the UI.
#[derive(Debug, Clone)]
pub enum Event {
    /// First event: the handle the UI uses to send commands.
    Ready(SyncSender<Command>),
    Status(DeviceStatus),
    MediaListed(Vec<MediaItem>),
    ScanFailed(String),
    Thumbnail(MediaId, Vec<u8>),
    Progress {
        done: usize,
        total: usize,
    },
    ItemFailed {
        file_name: String,
        error: String,
    },
    TransferDone {
        copied: usize,
        failed: usize,
        cancelled: bool,
    },
}

/// Event stream backed by the demo phone. Used with `Subscription::run`.
pub fn demo_events() -> impl Stream<Item = Event> {
    events(FakeSource::demo)
}

/// Starts a worker thread and turns its events into a stream for iced.
fn events<S: MediaSource + 'static>(make_source: fn() -> S) -> impl Stream<Item = Event> {
    iced::stream::channel(64, async move |mut output| {
        let (command_tx, command_rx) = sync_channel(16);
        let (event_tx, mut event_rx) = mpsc::channel(64);

        let spawned = thread::Builder::new()
            .name("transfer-worker".into())
            .spawn(move || run(make_source(), &command_rx, &event_tx, POLL_INTERVAL));
        if let Err(error) = spawned {
            warn!(%error, "could not start worker thread");
            return;
        }

        if output.send(Event::Ready(command_tx)).await.is_err() {
            return;
        }
        while let Some(event) = event_rx.recv().await {
            if output.send(event).await.is_err() {
                break; // UI is gone
            }
        }
    })
}

/// Marker meaning "the UI has gone away, stop the worker".
struct Disconnected;

fn send(events: &mpsc::Sender<Event>, event: Event) -> Result<(), Disconnected> {
    events.blocking_send(event).map_err(|_| Disconnected)
}

/// Worker main loop. Returns when either channel closes.
pub fn run<S: MediaSource>(
    mut source: S,
    commands: &Receiver<Command>,
    events: &mpsc::Sender<Event>,
    poll_interval: Duration,
) {
    let mut last_status: Option<DeviceStatus> = None;
    let mut last_poll: Option<Instant> = None;
    // Commands that arrived during a transfer and still need handling.
    let mut deferred: VecDeque<Command> = VecDeque::new();

    loop {
        if last_poll.is_none_or(|t| t.elapsed() >= poll_interval) {
            last_poll = Some(Instant::now());
            if poll_device(&mut source, &mut last_status, events).is_err() {
                return;
            }
        }

        let command = match deferred.pop_front() {
            Some(command) => command,
            None => {
                let waited = last_poll.map_or(Duration::ZERO, |t| t.elapsed());
                match commands.recv_timeout(poll_interval.saturating_sub(waited)) {
                    Ok(command) => command,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        };

        let result = match command {
            Command::LoadThumbnails(ids) => load_thumbnails(&mut source, ids, events),
            Command::Transfer(items) => {
                transfer(&mut source, items, commands, &mut deferred, events)
            }
            Command::Cancel => Ok(()), // nothing running
        };
        if result.is_err() {
            return;
        }
    }
}

fn poll_device<S: MediaSource>(
    source: &mut S,
    last_status: &mut Option<DeviceStatus>,
    events: &mpsc::Sender<Event>,
) -> Result<(), Disconnected> {
    let status = source.status();
    if last_status.as_ref() == Some(&status) {
        return Ok(());
    }
    info!(?status, "device status changed");
    *last_status = Some(status.clone());
    let ready = matches!(status, DeviceStatus::Ready { .. });
    send(events, Event::Status(status))?;

    if ready {
        match source.list_media() {
            Ok(mut items) => {
                // Newest first; items without a date go last.
                items.sort_by_key(|item| std::cmp::Reverse(item.created));
                info!(count = items.len(), "listed media");
                send(events, Event::MediaListed(items))?;
            }
            Err(error) => {
                warn!(%error, "listing media failed");
                send(events, Event::ScanFailed(error.to_string()))?;
            }
        }
    }
    Ok(())
}

fn load_thumbnails<S: MediaSource>(
    source: &mut S,
    ids: Vec<MediaId>,
    events: &mpsc::Sender<Event>,
) -> Result<(), Disconnected> {
    for id in ids {
        match source.thumbnail(&id) {
            Ok(bytes) => send(events, Event::Thumbnail(id, bytes))?,
            // A missing thumbnail is not worth bothering the user about.
            Err(error) => warn!(%error, id = id.0, "thumbnail failed"),
        }
    }
    Ok(())
}

fn transfer<S: MediaSource>(
    source: &mut S,
    items: Vec<MediaItem>,
    commands: &Receiver<Command>,
    deferred: &mut VecDeque<Command>,
    events: &mpsc::Sender<Event>,
) -> Result<(), Disconnected> {
    let total = items.len();
    let (mut copied, mut failed, mut cancelled) = (0, 0, false);
    info!(total, "transfer started");

    for (index, item) in items.iter().enumerate() {
        // Check for Cancel between files; keep any other command for later.
        loop {
            match commands.try_recv() {
                Ok(Command::Cancel) => cancelled = true,
                Ok(other) => deferred.push_back(other),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Err(Disconnected),
            }
        }
        if cancelled {
            break;
        }

        match copy_placeholder(source, item) {
            Ok(()) => copied += 1,
            Err(error) => {
                failed += 1;
                warn!(%error, file = item.file_name, "copy failed");
                send(
                    events,
                    Event::ItemFailed {
                        file_name: item.file_name.clone(),
                        error: error.to_string(),
                    },
                )?;
            }
        }
        send(
            events,
            Event::Progress {
                done: index + 1,
                total,
            },
        )?;
    }

    info!(copied, failed, cancelled, "transfer finished");
    send(
        events,
        Event::TransferDone {
            copied,
            failed,
            cancelled,
        },
    )
}

/// MILESTONE 1 PLACEHOLDER: reads the file and throws the bytes away.
/// Milestone 2 replaces this with the real copy engine (`copy.rs`).
fn copy_placeholder<S: MediaSource>(source: &mut S, item: &MediaItem) -> Result<(), TransferError> {
    let mut reader = source.open(&item.id)?;
    io::copy(&mut reader, &mut io::sink())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the worker on a thread against an instant fake phone.
    fn start(count: usize) -> (SyncSender<Command>, mpsc::Receiver<Event>) {
        let (command_tx, command_rx) = sync_channel(16);
        let (event_tx, event_rx) = mpsc::channel(256);
        thread::spawn(move || {
            run(
                FakeSource::ready(count),
                &command_rx,
                &event_tx,
                Duration::from_millis(10),
            );
        });
        (command_tx, event_rx)
    }

    fn next(events: &mut mpsc::Receiver<Event>) -> Event {
        events.blocking_recv().expect("worker stopped")
    }

    fn wait_for_listing(events: &mut mpsc::Receiver<Event>) -> Vec<MediaItem> {
        assert!(matches!(
            next(events),
            Event::Status(DeviceStatus::Ready { .. })
        ));
        match next(events) {
            Event::MediaListed(items) => items,
            other => panic!("expected MediaListed, got {other:?}"),
        }
    }

    #[test]
    fn lists_media_newest_first() {
        let (_commands, mut events) = start(5);
        let items = wait_for_listing(&mut events);
        assert_eq!(items.len(), 5);
        assert!(items.windows(2).all(|w| w[0].created >= w[1].created));
    }

    #[test]
    fn loads_requested_thumbnails() {
        let (commands, mut events) = start(3);
        let items = wait_for_listing(&mut events);
        let ids: Vec<_> = items.iter().map(|i| i.id.clone()).collect();
        commands.send(Command::LoadThumbnails(ids.clone())).unwrap();
        for id in ids {
            match next(&mut events) {
                Event::Thumbnail(got, bytes) => {
                    assert_eq!(got, id);
                    assert!(!bytes.is_empty());
                }
                other => panic!("expected Thumbnail, got {other:?}"),
            }
        }
    }

    #[test]
    fn transfer_reports_progress_then_done() {
        let (commands, mut events) = start(4);
        let items = wait_for_listing(&mut events);
        commands.send(Command::Transfer(items)).unwrap();
        for expected in 1..=4 {
            match next(&mut events) {
                Event::Progress { done, total } => assert_eq!((done, total), (expected, 4)),
                other => panic!("expected Progress, got {other:?}"),
            }
        }
        assert!(matches!(
            next(&mut events),
            Event::TransferDone {
                copied: 4,
                failed: 0,
                cancelled: false
            }
        ));
    }
}
