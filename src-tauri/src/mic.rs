//! Microphone capture while push-to-talk is held (docs/architecture.md section 2).
//!
//! `cpal::Stream` is `!Send` on Windows, so one dedicated thread creates, plays and drops
//! the stream and drains the lock-free ring buffer into `mm_core::mic::Framer`.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self as std_mpsc, RecvTimeoutError, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, InputCallbackInfo, SampleFormat, Stream, StreamConfig};
use mm_core::mic::{Framer, level};
use ringbuf::HeapRb;
use ringbuf::traits::*;
use tokio::sync::mpsc;

const RING_SECONDS: usize = 2;
const DRAIN_INTERVAL: Duration = Duration::from_millis(20);
const DRAIN_CHUNK_SAMPLES: usize = 2_048;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MicError {
    NoDevice,
    UnsupportedFormat,
    Device(String),
}

impl fmt::Display for MicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => f.write_str("no default microphone device"),
            Self::UnsupportedFormat => f.write_str("unsupported microphone sample format"),
            Self::Device(error) => write!(f, "microphone device error: {error}"),
        }
    }
}

impl std::error::Error for MicError {}

fn device_error(error: impl fmt::Display) -> MicError {
    MicError::Device(error.to_string())
}

enum Command {
    Stop,
    Cancel,
}

pub struct Microphone {
    command: Option<std_mpsc::Sender<Command>>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for Microphone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Microphone").finish_non_exhaustive()
    }
}

impl Microphone {
    /// Opens the default input device and starts capturing at once (called on PttEvent::Start).
    pub fn start(
        frames: mpsc::Sender<Vec<u8>>,
        on_level: impl FnMut(f32) + Send + 'static,
    ) -> Result<Microphone, MicError> {
        let (ready_tx, ready_rx) = std_mpsc::sync_channel(1);
        let (command_tx, command_rx) = std_mpsc::channel();
        let worker = thread::Builder::new()
            .name("metamate-mic".into())
            .spawn(move || session(frames, on_level, ready_tx, command_rx))
            .map_err(device_error)?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                command: Some(command_tx),
                worker: Some(worker),
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
    ready: SyncSender<Result<(), MicError>>,
    commands: std_mpsc::Receiver<Command>,
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
    loop {
        match commands.recv_timeout(DRAIN_INTERVAL) {
            Ok(Command::Stop) => {
                drop(stream);
                if !device_lost.load(Ordering::Acquire) {
                    let _ = flush_ring(
                        &mut consumer,
                        &mut scratch,
                        &mut framer,
                        &frames,
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
                    break;
                }
                if !drain_ring(
                    &mut consumer,
                    &mut scratch,
                    &mut framer,
                    &frames,
                    &mut on_level,
                ) {
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

fn drain_ring(
    consumer: &mut impl Consumer<Item = f32>,
    scratch: &mut [f32],
    framer: &mut Framer,
    frames: &mpsc::Sender<Vec<u8>>,
    on_level: &mut (impl FnMut(f32) + Send),
) -> bool {
    loop {
        let count = consumer.pop_slice(scratch);
        if count == 0 {
            return true;
        }
        for frame in framer.push(&scratch[..count]) {
            if !send_frame(frame, frames, on_level) {
                return false;
            }
        }
    }
}

fn flush_ring(
    consumer: &mut impl Consumer<Item = f32>,
    scratch: &mut [f32],
    framer: &mut Framer,
    frames: &mpsc::Sender<Vec<u8>>,
    on_level: &mut (impl FnMut(f32) + Send),
) -> bool {
    if !drain_ring(consumer, scratch, framer, frames, on_level) {
        return false;
    }
    if let Some(last) = framer.finish() {
        return send_frame(last, frames, on_level);
    }
    true
}

fn send_frame(
    frame: Vec<u8>,
    frames: &mpsc::Sender<Vec<u8>>,
    on_level: &mut (impl FnMut(f32) + Send),
) -> bool {
    let rms = level(&frame);
    if frames.blocking_send(frame).is_err() {
        return false;
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
        let mut levels = Vec::new();

        assert!(drain_ring(
            &mut consumer,
            &mut scratch,
            &mut framer,
            &tx,
            &mut |rms| levels.push(rms),
        ));
        assert_eq!(levels.len(), 1);
        assert!((levels[0] - 0.5).abs() < 0.01);
        assert_eq!(rx.try_recv().unwrap().len(), FRAME_BYTES);
        assert!(rx.try_recv().is_err());

        assert!(flush_ring(
            &mut consumer,
            &mut scratch,
            &mut framer,
            &tx,
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
        let mut levels = Vec::new();

        assert!(!drain_ring(
            &mut consumer,
            &mut scratch,
            &mut framer,
            &tx,
            &mut |rms| levels.push(rms),
        ));
        assert!(levels.is_empty());
    }
}
