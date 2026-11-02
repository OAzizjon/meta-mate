//! Microphone capture while push-to-talk is held (docs/architecture.md section 2).
//!
//! `cpal::Stream` is `!Send` on Windows, so one dedicated thread creates, plays and drops
//! the stream and drains the lock-free ring buffer into `mm_core::mic::Framer`.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self as std_mpsc, RecvTimeoutError, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, InputCallbackInfo, SampleFormat, Stream, StreamConfig};
use mm_core::mic::{Framer, StallWatch, level};
use ringbuf::HeapRb;
use ringbuf::traits::*;
use tokio::sync::mpsc;

const RING_SECONDS: usize = 2;
const DRAIN_INTERVAL: Duration = Duration::from_millis(20);
const DRAIN_CHUNK_SAMPLES: usize = 2_048;
/// A full frame queue is retried this often, so a cancel is seen at once.
const SEND_RETRY: Duration = Duration::from_millis(10);
/// How long a stop may wait for the queue to take its last frames.
const FLUSH_DEADLINE: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MicError {
    NoDevice,
    UnsupportedFormat,
    /// Windows privacy settings refuse desktop apps the microphone (E_ACCESSDENIED).
    Denied(String),
    /// Another program holds the microphone exclusively (AUDCLNT_E_DEVICE_IN_USE).
    Busy(String),
    Device(String),
}

impl fmt::Display for MicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => f.write_str("no default microphone device"),
            Self::UnsupportedFormat => f.write_str("unsupported microphone sample format"),
            Self::Denied(error) => write!(f, "microphone access denied: {error}"),
            Self::Busy(error) => write!(f, "microphone in use by another program: {error}"),
            Self::Device(error) => write!(f, "microphone device error: {error}"),
        }
    }
}

impl std::error::Error for MicError {}

/// Tells apart the failures the person can fix by the HRESULT in the message.
fn device_error(error: impl fmt::Display) -> MicError {
    let text = error.to_string();
    let code = text.to_ascii_lowercase();
    if code.contains("0x80070005") {
        MicError::Denied(text)
    } else if code.contains("0x8889000a") {
        MicError::Busy(text)
    } else {
        MicError::Device(text)
    }
}

enum Command {
    Stop,
    Cancel,
}

pub struct Microphone {
    command: Option<std_mpsc::Sender<Command>>,
    worker: Option<JoinHandle<()>>,
    /// Set by Drop (a cancel): a send waiting on a full queue gives up at once.
    abandon: Arc<AtomicBool>,
}

impl fmt::Debug for Microphone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Microphone").finish_non_exhaustive()
    }
}

impl Microphone {
    /// Opens the default input device and starts capturing at once (called on PttEvent::Start).
    /// `on_level` receives RMS in 0..=1 for each 80 ms PCM frame (12.5 Hz),
    /// including the final padded frame. It runs on the capture worker.
    /// `on_lost` runs there once if the device is lost or stops delivering samples for
    /// over a second (`mm_core::mic::STALL_MS`): the recording ended by itself and `frames`
    /// closes.
    pub fn start(
        frames: mpsc::Sender<Vec<u8>>,
        on_level: impl FnMut(f32) + Send + 'static,
        on_lost: impl FnOnce() + Send + 'static,
    ) -> Result<Microphone, MicError> {
        let (ready_tx, ready_rx) = std_mpsc::sync_channel(1);
        let (command_tx, command_rx) = std_mpsc::channel();
        let abandon = Arc::new(AtomicBool::new(false));
        let worker_abandon = Arc::clone(&abandon);
        let worker = thread::Builder::new()
            .name("metamate-mic".into())
            .spawn(move || {
                session(
                    frames,
                    on_level,
                    on_lost,
                    ready_tx,
                    command_rx,
                    &worker_abandon,
                )
            })
            .map_err(device_error)?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                command: Some(command_tx),
                worker: Some(worker),
                abandon,
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err(MicError::Device(
                    "microphone session exited before start completed".into(),
                ))
            }
        }
    }

    /// Stops capturing (PttEvent::Stop): the last partial frame is flushed, then the sender is dropped so the receiver sees the end.
    pub fn stop(mut self) {
        if let Some(command) = self.command.take() {
            let _ = command.send(Command::Stop);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        self.abandon.store(true, Ordering::Release);
        if let Some(command) = self.command.take() {
            let _ = command.send(Command::Cancel);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn session(
    frames: mpsc::Sender<Vec<u8>>,
    mut on_level: impl FnMut(f32) + Send + 'static,
    on_lost: impl FnOnce() + Send + 'static,
    ready: SyncSender<Result<(), MicError>>,
    commands: std_mpsc::Receiver<Command>,
    abandon: &AtomicBool,
) {
    let opened = open_stream();
    let (stream, mut consumer, mut framer, device_lost) = match opened {
        Ok(parts) => {
            let _ = ready.send(Ok(()));
            parts
        }
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };

    let mut scratch = vec![0.0_f32; DRAIN_CHUNK_SAMPLES];
    let started = Instant::now();
    let mut watch = StallWatch::default();
    loop {
        match commands.recv_timeout(DRAIN_INTERVAL) {
            Ok(Command::Stop) => {
                drop(stream);
                if !device_lost.load(Ordering::Acquire) {
                    let outlet = Outlet {
                        frames: &frames,
                        abandon,
                        deadline: Some(Instant::now() + FLUSH_DEADLINE),
                    };
                    let _ = flush_ring(
                        &mut consumer,
                        &mut scratch,
                        &mut framer,
                        &outlet,
                        &mut on_level,
                    );
                }
                break;
            }
            Ok(Command::Cancel) | Err(RecvTimeoutError::Disconnected) => {
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
                if device_lost.load(Ordering::Acquire) {
                    on_lost();
                    break;
                }
                let outlet = Outlet {
                    frames: &frames,
                    abandon,
                    deadline: None,
                };
                let Some(popped) = drain_ring(
                    &mut consumer,
                    &mut scratch,
                    &mut framer,
                    &outlet,
                    &mut on_level,
                ) else {
                    break;
                };
                let now_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                if popped > 0 {
                    watch.fed(now_ms);
                } else if watch.stalled(now_ms) {
                    // A Bluetooth headset switching profile or a changed default device can
                    // leave the stream silent: the recording ends as if released.
                    let outlet = Outlet {
                        frames: &frames,
                        abandon,
                        deadline: Some(Instant::now() + FLUSH_DEADLINE),
                    };
                    let _ = flush_ring(
                        &mut consumer,
                        &mut scratch,
                        &mut framer,
                        &outlet,
                        &mut on_level,
                    );
                    on_lost();
                    break;
                }
            }
        }
    }
}

fn open_stream() -> Result<(Stream, impl Consumer<Item = f32>, Framer, Arc<AtomicBool>), MicError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(MicError::NoDevice)?;
    let supported = device.default_input_config().map_err(device_error)?;
    let format = supported.sample_format();
    if !matches!(
        format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
    ) {
        return Err(MicError::UnsupportedFormat);
    }

    let rate = supported.sample_rate();
    let channels = supported.channels();
    let framer = Framer::new(rate, channels).map_err(|_| MicError::UnsupportedFormat)?;
    let rb = HeapRb::<f32>::new(ring_capacity(rate, channels));
    let (mut producer, consumer) = rb.split();
    let config: StreamConfig = supported.into();
    let device_lost = Arc::new(AtomicBool::new(false));
    let lost_flag = Arc::clone(&device_lost);
    // Glitches (Xrun), a refused real-time boost and a followed default-device change leave
    // the stream running; anything else ends the session.
    let err_fn = move |error: cpal::Error| {
        if !matches!(
            error.kind(),
            ErrorKind::Xrun | ErrorKind::RealtimeDenied | ErrorKind::DeviceChanged
        ) {
            lost_flag.store(true, Ordering::Release);
        }
    };

    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _: &InputCallbackInfo| push_f32(&mut producer, data),
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _: &InputCallbackInfo| push_i16(&mut producer, data),
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _: &InputCallbackInfo| push_u16(&mut producer, data),
            err_fn,
            None,
        ),
        _ => return Err(MicError::UnsupportedFormat),
    }
    .map_err(device_error)?;

    stream.play().map_err(device_error)?;
    Ok((stream, consumer, framer, device_lost))
}

fn ring_capacity(rate: u32, channels: u16) -> usize {
    usize::try_from(rate)
        .unwrap_or(48_000)
        .saturating_mul(usize::from(channels))
        .saturating_mul(RING_SECONDS)
        .max(1)
}

#[inline]
fn i16_to_f32(sample: i16) -> f32 {
    f32::from(sample) / 32_768.0
}

#[inline]
fn u16_to_f32(sample: u16) -> f32 {
    (f32::from(sample) - 32_768.0) / 32_768.0
}

#[inline]
fn push_f32(producer: &mut impl Producer<Item = f32>, data: &[f32]) {
    let _ = producer.push_slice(data);
}

#[inline]
fn push_i16(producer: &mut impl Producer<Item = f32>, data: &[i16]) {
    for &sample in data {
        if producer.try_push(i16_to_f32(sample)).is_err() {
            break;
        }
    }
}

#[inline]
fn push_u16(producer: &mut impl Producer<Item = f32>, data: &[u16]) {
    for &sample in data {
        if producer.try_push(u16_to_f32(sample)).is_err() {
            break;
        }
    }
}

/// Where frames go. A send never blocks past a cancel, nor past a stop's deadline: the
/// queue's reader may be stuck (a stalled speech socket), the person must not be.
struct Outlet<'a> {
    frames: &'a mpsc::Sender<Vec<u8>>,
    abandon: &'a AtomicBool,
    deadline: Option<Instant>,
}

/// The samples taken from the ring, or None once frames can no longer be sent.
fn drain_ring(
    consumer: &mut impl Consumer<Item = f32>,
    scratch: &mut [f32],
    framer: &mut Framer,
    frames: &Outlet<'_>,
    on_level: &mut (impl FnMut(f32) + Send),
) -> Option<usize> {
    let mut popped = 0;
    loop {
        let count = consumer.pop_slice(scratch);
        if count == 0 {
            return Some(popped);
        }
        popped += count;
        for frame in framer.push(&scratch[..count]) {
            if !send_frame(frame, frames, on_level) {
                return None;
            }
        }
    }
}

fn flush_ring(
    consumer: &mut impl Consumer<Item = f32>,
    scratch: &mut [f32],
    framer: &mut Framer,
    frames: &Outlet<'_>,
    on_level: &mut (impl FnMut(f32) + Send),
) -> bool {
    if drain_ring(consumer, scratch, framer, frames, on_level).is_none() {
        return false;
    }
    if let Some(last) = framer.finish() {
        return send_frame(last, frames, on_level);
    }
    true
}

fn send_frame(
    frame: Vec<u8>,
    outlet: &Outlet<'_>,
    on_level: &mut (impl FnMut(f32) + Send),
) -> bool {
    let rms = level(&frame);
    let mut frame = frame;
    loop {
        match outlet.frames.try_send(frame) {
            Ok(()) => break,
            Err(mpsc::error::TrySendError::Closed(_)) => return false,
            Err(mpsc::error::TrySendError::Full(back)) => {
                let late = outlet
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline);
                if outlet.abandon.load(Ordering::Acquire) || late {
                    return false;
                }
                frame = back;
                thread::sleep(SEND_RETRY);
            }
        }
    }
    on_level(rms);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_core::mic::{FRAME_BYTES, FRAME_SAMPLES, OUTPUT_RATE};

    #[test]
    fn converts_i16_and_u16_endpoints_and_silence() {
        assert_eq!(i16_to_f32(i16::MIN), -1.0);
        assert_eq!(i16_to_f32(0), 0.0);
        assert!((i16_to_f32(i16::MAX) - 32_767.0 / 32_768.0).abs() < f32::EPSILON);

        assert_eq!(u16_to_f32(0), -1.0);
        assert_eq!(u16_to_f32(32_768), 0.0);
        assert!((u16_to_f32(u16::MAX) - 32_767.0 / 32_768.0).abs() < f32::EPSILON);
    }

    #[test]
    fn callback_helpers_push_and_drop_excess_without_blocking() {
        let rb = HeapRb::<f32>::new(4);
        let (mut producer, mut consumer) = rb.split();
        push_i16(&mut producer, &[0, 16_384, -16_384]);
        push_u16(&mut producer, &[32_768, 49_152]);

        let mut out = [0.0_f32; 4];
        assert_eq!(consumer.pop_slice(&mut out), 4);
        assert_eq!(out, [0.0, 0.5, -0.5, 0.0]);
    }

    #[test]
    fn drain_and_flush_send_frames_and_levels() {
        let rb = HeapRb::<f32>::new(ring_capacity(OUTPUT_RATE, 1));
        let (mut producer, mut consumer) = rb.split();
        push_f32(&mut producer, &vec![0.5_f32; FRAME_SAMPLES + 100]);

        let mut framer = Framer::new(OUTPUT_RATE, 1).unwrap();
        let mut scratch = vec![0.0_f32; 512];
        let (tx, mut rx) = mpsc::channel(8);
        let abandon = AtomicBool::new(false);
        let outlet = Outlet {
            frames: &tx,
            abandon: &abandon,
            deadline: None,
        };
        let mut levels = Vec::new();

        assert_eq!(
            drain_ring(
                &mut consumer,
                &mut scratch,
                &mut framer,
                &outlet,
                &mut |rms| levels.push(rms),
            ),
            Some(FRAME_SAMPLES + 100)
        );
        assert_eq!(levels.len(), 1);
        assert!((levels[0] - 0.5).abs() < 0.01);
        assert_eq!(rx.try_recv().unwrap().len(), FRAME_BYTES);
        assert!(rx.try_recv().is_err());

        assert!(flush_ring(
            &mut consumer,
            &mut scratch,
            &mut framer,
            &outlet,
            &mut |rms| levels.push(rms),
        ));
        drop(tx);
        assert_eq!(levels.len(), 2);
        assert_eq!(rx.try_recv().unwrap().len(), FRAME_BYTES);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn drain_stops_quietly_when_receiver_is_dropped() {
        let rb = HeapRb::<f32>::new(ring_capacity(OUTPUT_RATE, 1));
        let (mut producer, mut consumer) = rb.split();
        push_f32(&mut producer, &vec![0.25_f32; FRAME_SAMPLES]);

        let mut framer = Framer::new(OUTPUT_RATE, 1).unwrap();
        let mut scratch = vec![0.0_f32; 512];
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        let abandon = AtomicBool::new(false);
        let outlet = Outlet {
            frames: &tx,
            abandon: &abandon,
            deadline: None,
        };
        let mut levels = Vec::new();

        assert_eq!(
            drain_ring(
                &mut consumer,
                &mut scratch,
                &mut framer,
                &outlet,
                &mut |rms| levels.push(rms),
            ),
            None
        );
        assert!(levels.is_empty());
    }

    #[test]
    fn windows_failures_the_person_can_fix_are_told_apart() {
        assert!(matches!(
            device_error("Access is denied. (0x80070005)"),
            MicError::Denied(_)
        ));
        assert!(matches!(
            device_error("HRESULT(0x8889000A)"),
            MicError::Busy(_)
        ));
        assert!(matches!(
            device_error("The requested device is no longer available."),
            MicError::Device(_)
        ));
    }

    #[test]
    fn a_full_queue_gives_up_on_cancel_and_after_the_deadline() {
        let (tx, _rx) = mpsc::channel(1);
        tx.try_send(vec![0; FRAME_BYTES]).unwrap();
        let abandon = AtomicBool::new(true);
        let cancelled = Outlet {
            frames: &tx,
            abandon: &abandon,
            deadline: None,
        };
        assert!(!send_frame(vec![0; FRAME_BYTES], &cancelled, &mut |_| {}));
        let calm = AtomicBool::new(false);
        let late = Outlet {
            frames: &tx,
            abandon: &calm,
            deadline: Some(Instant::now()),
        };
        assert!(!send_frame(vec![0; FRAME_BYTES], &late, &mut |_| {}));
    }
}
