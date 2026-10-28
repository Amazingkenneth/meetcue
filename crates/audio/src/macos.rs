//! ScreenCaptureKit system sound and AVFoundation microphone capture.
//! Native callbacks copy into a bounded queue; conversion and the sink run
//! on a worker. Native stop drains callbacks before their context is freed.

use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use meetcue_core::{clock, codes, Channel, MeetCueError};

use crate::convert::{FrameConverter, InputFormat, SampleFormat};
use crate::{looks_bluetooth, AudioSource, CaptureConfig, DeviceInfo, Sink, SourceEvent};

type Samples = unsafe extern "C" fn(*mut c_void, i32, *const f32, usize, u32, f64);
type Error = unsafe extern "C" fn(*mut c_void, i32, i32, *const c_char);
type Device = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, bool);

extern "C" {
    fn mc_audio_devices(input: bool, context: *mut c_void, callback: Device);
    fn mc_audio_start(
        mic: bool,
        remote: bool,
        uid: *const c_char,
        context: *mut c_void,
        samples: Samples,
        error: Error,
        out: *mut *mut c_void,
        message: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn mc_audio_stop(handle: *mut c_void);
    fn mc_audio_tone(frequency: f32, duration_ms: u32) -> i32;
}

enum Packet {
    Samples(Channel, Vec<f32>, u32, i64),
    Error(Channel, MeetCueError),
}

fn channel(index: i32) -> Channel {
    if index == 0 {
        Channel::Me
    } else {
        Channel::Remote
    }
}

fn error_code(code: i32) -> &'static str {
    match code {
        1 => codes::AUD_MIC_DENIED,
        2 => codes::AUD_SYSTEM_DENIED,
        4 => codes::AUD_UNSUPPORTED,
        _ => codes::AUD_NO_DEVICE,
    }
}

unsafe extern "C" fn samples(
    context: *mut c_void,
    index: i32,
    data: *const f32,
    count: usize,
    rate: u32,
    age_ms: f64,
) {
    if data.is_null() || count == 0 || rate == 0 {
        return;
    }
    // SAFETY: native capture owns a borrowed context until stop has drained
    // all callbacks; the samples are valid for this call only.
    let tx = &*(context as *const mpsc::SyncSender<Packet>);
    let age = if age_ms.is_finite() {
        age_ms.clamp(0.0, 10_000.0) as i64
    } else {
        0
    };
    let packet = Packet::Samples(
        channel(index),
        std::slice::from_raw_parts(data, count).to_vec(),
        rate,
        clock::mono_ms() - age,
    );
    // Never block Apple's capture queue if processing falls behind.
    let _ = tx.try_send(packet);
}

unsafe extern "C" fn error(context: *mut c_void, index: i32, code: i32, message: *const c_char) {
    let tx = &*(context as *const mpsc::SyncSender<Packet>);
    let detail = CStr::from_ptr(message).to_string_lossy().into_owned();
    let _ = tx.try_send(Packet::Error(
        channel(index),
        MeetCueError::new(error_code(code), detail),
    ));
}

unsafe extern "C" fn device(
    context: *mut c_void,
    uid: *const c_char,
    name: *const c_char,
    default: bool,
) {
    let devices = &mut *(context as *mut Vec<DeviceInfo>);
    let name = CStr::from_ptr(name).to_string_lossy().into_owned();
    devices.push(DeviceInfo {
        id: CStr::from_ptr(uid).to_string_lossy().into_owned(),
        is_bluetooth: looks_bluetooth(&name),
        name,
        is_default: default,
    });
}

fn devices(input: bool) -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    unsafe {
        mc_audio_devices(input, &mut out as *mut _ as *mut c_void, device);
    }
    out
}

fn microphone_uid(requested: Option<&str>) -> Option<String> {
    let devices = devices(true);
    devices
        .iter()
        .find(|d| Some(d.id.as_str()) == requested)
        .or_else(|| devices.iter().find(|d| d.is_default))
        .map(|d| d.id.clone())
}

struct Native {
    handle: *mut c_void,
    // The address stays stable as Native moves onto the processing thread.
    _context: Box<mpsc::SyncSender<Packet>>,
}

// SAFETY: the handle is exclusively owned, used from one worker at a time,
// and native callbacks access only the thread-safe sender, never the handle.
unsafe impl Send for Native {}

impl Native {
    fn start(cfg: &CaptureConfig, tx: mpsc::SyncSender<Packet>) -> Result<Self, MeetCueError> {
        let uid = CString::new(cfg.mic_device.as_deref().unwrap_or_default())
            .map_err(|_| MeetCueError::new(codes::AUD_NO_DEVICE, "invalid microphone ID"))?;
        let mut context = Box::new(tx);
        let mut handle = std::ptr::null_mut();
        let mut message = [0 as c_char; 1024];
        let code = unsafe {
            mc_audio_start(
                cfg.capture_me,
                cfg.capture_remote,
                uid.as_ptr(),
                context.as_mut() as *mut _ as *mut c_void,
                samples,
                error,
                &mut handle,
                message.as_mut_ptr(),
                message.len(),
            )
        };
        if code != 0 {
            let detail = unsafe { CStr::from_ptr(message.as_ptr()) }.to_string_lossy();
            return Err(MeetCueError::new(error_code(code), detail));
        }
        Ok(Self {
            handle,
            _context: context,
        })
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        unsafe {
            mc_audio_stop(self.handle);
        }
    }
}

#[derive(Default)]
pub struct MacSource {
    worker: Option<(Arc<AtomicBool>, JoinHandle<()>)>,
}

impl AudioSource for MacSource {
    fn list_microphones(&self) -> Result<Vec<DeviceInfo>, MeetCueError> {
        Ok(devices(true))
    }
    fn list_outputs(&self) -> Result<Vec<DeviceInfo>, MeetCueError> {
        Ok(devices(false))
    }

    fn start(&mut self, cfg: CaptureConfig, sink: Sink) -> Result<(), MeetCueError> {
        self.stop();
        let (tx, rx) = mpsc::sync_channel(128);
        let native = Native::start(&cfg, tx.clone())?;
        // Permission dialogs may have left old packets queued while the
        // other channel was opening. Start recognition at the current time.
        while rx.try_recv().is_ok() {}
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let worker = thread::spawn(move || {
            let mut native = Some(native);
            let mut uid = microphone_uid(cfg.mic_device.as_deref());
            let mut converters: [Option<FrameConverter>; 2] = [None, None];
            // Clock the remote channel even before ScreenCaptureKit sends
            // its first packet, so silent output keeps recognition alive.
            if cfg.capture_remote {
                let mut c = converter(Channel::Remote, 48_000, &cfg);
                c.start_clock(clock::mono_ms());
                converters[1] = Some(c);
            }
            let mut last_device_check = Instant::now();
            while !stop.load(Ordering::Relaxed) {
                match rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(Packet::Samples(ch, data, rate, ts)) => {
                        let slot = &mut converters[usize::from(ch == Channel::Remote)];
                        if slot.as_ref().is_none_or(|c| c.format().rate != rate) {
                            *slot = Some(converter(ch, rate, &cfg));
                        }
                        let mut frames = Vec::new();
                        slot.as_mut().unwrap().push_mono(&data, ts, &mut frames);
                        for f in frames {
                            sink(SourceEvent::Frame(f));
                        }
                    }
                    Ok(Packet::Error(ch, e)) => {
                        sink(SourceEvent::Error(ch, e));
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                for c in converters.iter_mut().flatten() {
                    let mut frames = Vec::new();
                    c.fill_silence(clock::mono_ms(), &mut frames);
                    for f in frames {
                        sink(SourceEvent::Frame(f));
                    }
                }
                // Follow the default microphone and recover when a selected
                // headset disappears. Device UIDs remain stable across runs.
                if cfg.capture_me && last_device_check.elapsed() >= Duration::from_secs(1) {
                    last_device_check = Instant::now();
                    let current = microphone_uid(cfg.mic_device.as_deref());
                    if current != uid || native.is_none() {
                        drop(native.take());
                        // Discard packets belonging to the previous stream.
                        while rx.try_recv().is_ok() {}
                        match Native::start(&cfg, tx.clone()) {
                            Ok(n) => {
                                native = Some(n);
                                uid = current;
                                converters = [None, None];
                                sink(SourceEvent::DeviceChanged(Channel::Me));
                            }
                            Err(e) => sink(SourceEvent::Error(Channel::Me, e)),
                        }
                    }
                }
            }
            drop(native);
        });
        self.worker = Some((stopped, worker));
        Ok(())
    }

    fn stop(&mut self) {
        if let Some((stop, worker)) = self.worker.take() {
            stop.store(true, Ordering::Relaxed);
            let _ = worker.join();
        }
    }

    fn play_test_tone(&self, freq_hz: f32, duration_ms: u32) -> Result<(), MeetCueError> {
        if unsafe { mc_audio_tone(freq_hz, duration_ms) } == 0 {
            Ok(())
        } else {
            Err(MeetCueError::new(
                codes::AUD_NO_DEVICE,
                "could not play the test tone",
            ))
        }
    }
}

fn converter(channel: Channel, rate: u32, cfg: &CaptureConfig) -> FrameConverter {
    let remote = channel == Channel::Remote;
    let mut c = FrameConverter::new(
        channel,
        InputFormat {
            rate,
            channels: 1,
            format: SampleFormat::F32,
        },
        if remote {
            cfg.remote_vad_threshold_db
        } else {
            cfg.vad_threshold_db
        },
    );
    c.set_boost(remote && cfg.boost_remote);
    c
}

impl Drop for MacSource {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_callback_copies_samples_and_preserves_channel() {
        let (tx, rx) = mpsc::sync_channel(1);
        let mut tx = Box::new(tx);
        let data = vec![0.25f32; 960];
        unsafe {
            samples(
                tx.as_mut() as *mut _ as *mut c_void,
                1,
                data.as_ptr(),
                data.len(),
                48_000,
                20.0,
            );
        }
        drop(data);
        let Packet::Samples(ch, copied, rate, ts) = rx.try_recv().unwrap() else {
            panic!("missing samples")
        };
        assert_eq!(ch, Channel::Remote);
        assert_eq!(copied, vec![0.25; 960]);
        assert_eq!(rate, 48_000);
        assert!(ts <= clock::mono_ms());
    }

    #[test]
    fn permission_errors_are_actionable_and_distinct() {
        assert_eq!(error_code(1), codes::AUD_MIC_DENIED);
        assert_eq!(error_code(2), codes::AUD_SYSTEM_DENIED);
        assert_eq!(error_code(3), codes::AUD_NO_DEVICE);
        assert_eq!(error_code(4), codes::AUD_UNSUPPORTED);
    }

    #[test]
    fn native_lifecycle_without_requesting_permissions() {
        let mut source = MacSource::default();
        for _ in 0..3 {
            source
                .start(
                    CaptureConfig {
                        capture_me: false,
                        capture_remote: false,
                        ..CaptureConfig::default()
                    },
                    Box::new(|_| {}),
                )
                .unwrap();
            source.stop();
            source.stop();
        }
    }
}
