//! Windows capture backend: WASAPI loopback of the default output device
//! (other participants) and WASAPI capture of the microphone (the user),
//! both in shared mode.
//!
//! Each channel has a capture thread that only copies device packets into a
//! lock-free ring buffer, and a processing thread that converts them to
//! frames. Capture threads never allocate, lock or touch the network.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use meetcue_core::{clock, codes, Channel, MeetCueError};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::HeapRb;
use wasapi::{
    initialize_mta, AudioCaptureClient, AudioClient, Device, DeviceEnumerator,
    DeviceEventCallbacks, Direction, Role, SampleType, StreamMode, WasapiError, WaveFormat,
};

use crate::convert::{FrameConverter, InputFormat, SampleFormat};
use crate::{looks_bluetooth, AudioSource, CaptureConfig, DeviceInfo, Sink, SourceEvent};

const AUDCLNT_E_DEVICE_IN_USE: u32 = 0x8889_000A;
const AUDCLNT_E_DEVICE_INVALIDATED: u32 = 0x8889_0004;
const E_ACCESSDENIED: u32 = 0x8007_0005;

fn map_err(e: &WasapiError) -> MeetCueError {
    let code = match e {
        WasapiError::Windows(w) => match w.code().0 as u32 {
            AUDCLNT_E_DEVICE_IN_USE => codes::AUD_EXCLUSIVE,
            E_ACCESSDENIED => codes::AUD_MIC_DENIED,
            _ => codes::AUD_NO_DEVICE,
        },
        _ => codes::AUD_NO_DEVICE,
    };
    MeetCueError::new(code, e.to_string())
}

fn is_invalidated(e: &WasapiError) -> bool {
    matches!(e, WasapiError::Windows(w) if w.code().0 as u32 == AUDCLNT_E_DEVICE_INVALIDATED)
}

/// Runs `f` on a fresh thread with COM initialised for the multithreaded
/// apartment, so callers on UI threads are not affected.
fn with_com<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    thread::spawn(move || {
        let _ = initialize_mta();
        f()
    })
    .join()
    .expect("COM worker thread panicked")
}

fn list(direction: Direction) -> Result<Vec<DeviceInfo>, MeetCueError> {
    with_com(move || {
        let enumerator = DeviceEnumerator::new().map_err(|e| map_err(&e))?;
        let default_id = enumerator
            .get_default_device(&direction)
            .and_then(|d| d.get_id())
            .ok();
        let collection = enumerator
            .get_device_collection(&direction)
            .map_err(|e| map_err(&e))?;
        let mut out = Vec::new();
        for device in &collection {
            let Ok(device) = device else { continue };
            let (Ok(id), Ok(name)) = (device.get_id(), device.get_friendlyname()) else {
                continue;
            };
            let iface = device.get_interface_friendlyname().unwrap_or_default();
            out.push(DeviceInfo {
                is_default: default_id.as_deref() == Some(id.as_str()),
                is_bluetooth: looks_bluetooth(&name) || looks_bluetooth(&iface),
                id,
                name,
            });
        }
        Ok(out)
    })
}

fn input_format(fmt: &WaveFormat) -> Option<InputFormat> {
    let format = match (fmt.get_subformat().ok()?, fmt.get_bitspersample()) {
        (SampleType::Float, 32) => SampleFormat::F32,
        (SampleType::Int, 16) => SampleFormat::I16,
        (SampleType::Int, 24) => SampleFormat::I24,
        (SampleType::Int, 32) => SampleFormat::I32,
        _ => return None,
    };
    Some(InputFormat {
        rate: fmt.get_samplespersec(),
        channels: fmt.get_nchannels(),
        format,
    })
}

struct Opened {
    client: AudioClient,
    capture: AudioCaptureClient,
    format: InputFormat,
}

fn open(channel: Channel, mic: Option<&str>) -> Result<Opened, WasapiError> {
    let enumerator = DeviceEnumerator::new()?;
    let device: Device = match channel {
        // Loopback: capture from the default *render* device.
        Channel::Remote => enumerator.get_default_device(&Direction::Render)?,
        Channel::Me => match mic {
            Some(id) => enumerator
                .get_device(id)
                .or_else(|_| enumerator.get_default_device(&Direction::Capture))?,
            None => enumerator.get_default_device(&Direction::Capture)?,
        },
    };
    let mut client = device.get_iaudioclient()?;
    // Use the device's own mix format and convert ourselves (see convert.rs).
    let mix = client.get_mixformat()?;
    let format = input_format(&mix).ok_or(WasapiError::UnsupportedFormat)?;
    let mode = StreamMode::PollingShared {
        autoconvert: false,
        buffer_duration_hns: 2_000_000, // 200 ms
    };
    client.initialize_client(&mix, &Direction::Capture, &mode)?;
    let capture = client.get_audiocaptureclient()?;
    client.start_stream()?;
    Ok(Opened {
        client,
        capture,
        format,
    })
}

/// Chunk metadata passed alongside the bytes: capture time and length.
type Meta = (i64, usize);

struct ChannelCtx {
    channel: Channel,
    mic: Option<String>,
    vad_threshold_db: f32,
    stop: Arc<AtomicBool>,
    restart: Arc<AtomicBool>,
    sink: Arc<Sink>,
}

fn run_channel(ctx: ChannelCtx) {
    let _ = initialize_mta();
    let mut first = true;
    let mut last_error: Option<String> = None;
    while !ctx.stop.load(Ordering::SeqCst) {
        ctx.restart.store(false, Ordering::SeqCst);
        let opened = match open(ctx.channel, ctx.mic.as_deref()) {
            Ok(o) => o,
            Err(e) => {
                let err = map_err(&e);
                // Report each distinct failure once, then keep retrying quietly.
                if last_error.as_deref() != Some(err.code.as_str()) {
                    (ctx.sink)(SourceEvent::Error(ctx.channel, err.clone()));
                    last_error = Some(err.code);
                }
                sleep_unless(&ctx.stop, Duration::from_secs(2));
                continue;
            }
        };
        last_error = None;
        if !first {
            (ctx.sink)(SourceEvent::DeviceChanged(ctx.channel));
        }
        first = false;
        capture_loop(&ctx, opened);
        if !ctx.stop.load(Ordering::SeqCst) {
            sleep_unless(&ctx.stop, Duration::from_millis(300));
        }
    }
}

fn sleep_unless(stop: &AtomicBool, total: Duration) {
    let step = Duration::from_millis(50);
    let mut waited = Duration::ZERO;
    while waited < total && !stop.load(Ordering::SeqCst) {
        thread::sleep(step);
        waited += step;
    }
}

fn capture_loop(ctx: &ChannelCtx, opened: Opened) {
    let Opened {
        client,
        capture,
        format,
    } = opened;
    let block = format.block_align();
    let rate = format.rate as i64;
    // Two seconds of device audio; allocated once, before capture starts.
    let (mut bytes_in, bytes_out) = HeapRb::<u8>::new(format.rate as usize * block * 2).split();
    let (mut meta_in, meta_out) = HeapRb::<Meta>::new(512).split();
    let mut packet = vec![0u8; format.rate as usize * block / 5];

    let done = Arc::new(AtomicBool::new(false));
    let processor = {
        let done = done.clone();
        let sink = ctx.sink.clone();
        let channel = ctx.channel;
        let vad = ctx.vad_threshold_db;
        thread::spawn(move || process_loop(channel, format, vad, bytes_out, meta_out, done, sink))
    };

    loop {
        if ctx.stop.load(Ordering::SeqCst) || ctx.restart.load(Ordering::SeqCst) {
            break;
        }
        let mut failed = None;
        loop {
            match capture.get_next_packet_size() {
                Ok(Some(n)) if n > 0 => {}
                Ok(_) => break,
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
            match capture.read_from_device(&mut packet) {
                Ok((frames, info)) => {
                    if frames == 0 {
                        break;
                    }
                    let len = frames as usize * block;
                    if info.flags.silent {
                        packet[..len].fill(0);
                    }
                    let ts = clock::mono_ms() - frames as i64 * 1000 / rate;
                    if bytes_in.vacant_len() >= len && meta_in.vacant_len() > 0 {
                        bytes_in.push_slice(&packet[..len]);
                        let _ = meta_in.try_push((ts, len));
                    }
                }
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = failed {
            if !is_invalidated(&e) {
                (ctx.sink)(SourceEvent::Error(ctx.channel, map_err(&e)));
            }
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    let _ = client.stop_stream();
    done.store(true, Ordering::SeqCst);
    let _ = processor.join();
}

fn process_loop(
    channel: Channel,
    format: InputFormat,
    vad_threshold_db: f32,
    mut bytes: ringbuf::HeapCons<u8>,
    mut meta: ringbuf::HeapCons<Meta>,
    done: Arc<AtomicBool>,
    sink: Arc<Sink>,
) {
    let mut converter = FrameConverter::new(channel, format, vad_threshold_db);
    let mut chunk = Vec::new();
    let mut frames = Vec::new();
    while !done.load(Ordering::SeqCst) {
        while let Some((ts, len)) = meta.try_pop() {
            chunk.resize(len, 0);
            let got = bytes.pop_slice(&mut chunk);
            converter.push_bytes(&chunk[..got], ts, &mut frames);
        }
        // Loopback delivers nothing while nothing plays: keep time moving.
        converter.fill_silence(clock::mono_ms(), &mut frames);
        for f in frames.drain(..) {
            sink(SourceEvent::Frame(f));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub struct WasapiSource {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl WasapiSource {
    pub fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            threads: Vec::new(),
        }
    }
}

impl AudioSource for WasapiSource {
    fn list_microphones(&self) -> Result<Vec<DeviceInfo>, MeetCueError> {
        list(Direction::Capture)
    }

    fn list_outputs(&self) -> Result<Vec<DeviceInfo>, MeetCueError> {
        list(Direction::Render)
    }

    fn start(&mut self, cfg: CaptureConfig, sink: Sink) -> Result<(), MeetCueError> {
        self.stop();
        self.stop = Arc::new(AtomicBool::new(false));
        let sink = Arc::new(sink);
        let restart_me = Arc::new(AtomicBool::new(false));
        let restart_remote = Arc::new(AtomicBool::new(false));

        // Watch for default-device changes and unplugged devices.
        {
            let stop = self.stop.clone();
            let (rm, rr) = (restart_me.clone(), restart_remote.clone());
            let follows_default_mic = cfg.mic_device.is_none();
            let mic_id = cfg.mic_device.clone();
            self.threads.push(thread::spawn(move || {
                let _ = initialize_mta();
                let Ok(enumerator) = DeviceEnumerator::new() else {
                    return;
                };
                let mut callbacks = DeviceEventCallbacks::new();
                {
                    let (rm, rr) = (rm.clone(), rr.clone());
                    callbacks.set_default_device_callback(move |direction, role, _id| {
                        if role != Role::Console {
                            return;
                        }
                        match direction {
                            Direction::Render => rr.store(true, Ordering::SeqCst),
                            Direction::Capture if follows_default_mic => {
                                rm.store(true, Ordering::SeqCst)
                            }
                            Direction::Capture => {}
                        }
                    });
                }
                {
                    let rm = rm.clone();
                    callbacks.set_device_removed_callback(move |id| {
                        if mic_id.as_deref() == Some(id.as_str()) {
                            rm.store(true, Ordering::SeqCst);
                        }
                    });
                }
                let registration = enumerator.register_notification_callback(callbacks);
                while !stop.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(100));
                }
                drop(registration);
            }));
        }

        for (channel, enabled, restart) in [
            (Channel::Me, cfg.capture_me, restart_me),
            (Channel::Remote, cfg.capture_remote, restart_remote),
        ] {
            if !enabled {
                continue;
            }
            let ctx = ChannelCtx {
                channel,
                mic: cfg.mic_device.clone(),
                vad_threshold_db: cfg.vad_threshold_db,
                stop: self.stop.clone(),
                restart,
                sink: sink.clone(),
            };
            self.threads.push(thread::spawn(move || run_channel(ctx)));
        }
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }

    fn play_test_tone(&self, freq_hz: f32, duration_ms: u32) -> Result<(), MeetCueError> {
        with_com(move || play_tone(freq_hz, duration_ms).map_err(|e| map_err(&e)))
    }
}

impl Drop for WasapiSource {
    fn drop(&mut self) {
        self.stop();
    }
}

fn play_tone(freq_hz: f32, duration_ms: u32) -> Result<(), WasapiError> {
    let enumerator = DeviceEnumerator::new()?;
    let device = enumerator.get_default_device(&Direction::Render)?;
    let mut client = device.get_iaudioclient()?;
    let rate = 48_000usize;
    let fmt = WaveFormat::new(32, 32, &SampleType::Float, rate, 2, None);
    let (_, min_period) = client.get_device_period()?;
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: min_period,
    };
    client.initialize_client(&fmt, &Direction::Render, &mode)?;
    let event = client.set_get_eventhandle()?;
    let render = client.get_audiorenderclient()?;
    client.start_stream()?;
    let total = rate * duration_ms as usize / 1000;
    let mut written = 0usize;
    let mut data = Vec::new();
    while written < total {
        let space = client.get_available_space_in_frames()? as usize;
        let n = space.min(total - written);
        data.clear();
        for i in 0..n {
            let t = (written + i) as f32 / rate as f32;
            let s = 0.3 * (2.0 * std::f32::consts::PI * freq_hz * t).sin();
            data.extend_from_slice(&s.to_le_bytes());
            data.extend_from_slice(&s.to_le_bytes());
        }
        render.write_to_device(n, &data, None)?;
        written += n;
        if event.wait_for_event(1000).is_err() {
            break;
        }
    }
    thread::sleep(Duration::from_millis(100));
    client.stop_stream()?;
    Ok(())
}
