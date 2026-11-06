//! Headless timing harness for SMBC: runs frames as fast as possible and
//! reports slow frames and traces. Usage: smbc_bench <swf> <seconds-of-game-time>
use ruffle_core::backend::log::LogBackend;
use ruffle_core::backend::navigator::{NullExecutor, NullNavigatorBackend};
use ruffle_core::tag_utils::movie_from_path;
use ruffle_core::PlayerBuilder;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::target::TextureTarget;
use std::time::{Duration, Instant};
use ruffle_core::backend::audio::{
    AudioBackend, AudioMixer, DecodeError, RegisterError, SoundHandle, SoundInstanceHandle,
    SoundStreamInfo, SoundTransform, swf,
};
use ruffle_core::impl_audio_mixer_backend;

pub struct BenchAudio { mixer: AudioMixer, buffer: Vec<i16>, pub out: std::sync::Arc<std::sync::Mutex<Vec<i16>>> }
impl AudioBackend for BenchAudio {
    impl_audio_mixer_backend!(mixer);
    fn play(&mut self) {}
    fn pause(&mut self) {}
    fn set_frame_rate(&mut self, _f: f64) {}
    fn tick(&mut self) {
        // 60 Hz host ticks: 735 stereo frames each.
        self.buffer.resize(735 * 2, 0);
        self.mixer.mix::<i16>(self.buffer.as_mut());
        self.out.lock().unwrap().extend_from_slice(&self.buffer);
    }
}


struct Log(Instant);
impl LogBackend for Log {
    fn avm_trace(&self, m: &str) {
        println!("[{:7.2}s] trace: {m}", self.0.elapsed().as_secs_f64());
    }
    fn avm_warning(&self, m: &str) {
        println!("[{:7.2}s] warn: {m}", self.0.elapsed().as_secs_f64());
    }
}

thread_local!(static STATS: std::cell::RefCell<Vec<f64>> = std::cell::RefCell::new(Vec::new()));

fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).with_writer(std::io::stderr).init();
    let args: Vec<String> = std::env::args().collect();
    let path = std::path::PathBuf::from(&args[1]);
    let game_secs: f64 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(30.0);
    let mut executor = NullExecutor::new();
    let nav = NullNavigatorBackend::with_base_path(path.parent().unwrap(), &executor).unwrap();
    let movie = movie_from_path(&path, None).unwrap();
    let start = Instant::now();
    let (vw, vh): (u32, u32) = std::env::var("SMBC_VIEW").ok().and_then(|v| { let (a, b) = v.split_once('x')?; Some((a.parse().ok()?, b.parse().ok()?)) }).unwrap_or((512, 480));
    let out = std::sync::Arc::new(std::sync::Mutex::new(Vec::<i16>::new()));
    let audio = BenchAudio { mixer: AudioMixer::new(2, 44100), buffer: vec![], out: out.clone() };
    let player = PlayerBuilder::new()
        .with_audio(audio)
        .with_renderer(WgpuRenderBackend::<TextureTarget>::for_offscreen((vw, vh), ruffle_render_wgpu::wgpu::Backends::GL, ruffle_render_wgpu::wgpu::PowerPreference::LowPower).expect("offscreen renderer"))
        .with_log(Log(start))
        .with_navigator(nav)
        .with_max_execution_duration(Duration::from_secs(3600))
        .with_viewport_dimensions(vw, vh, 1.0)
        .with_movie(movie)
        .with_autoplay(true)
        .build();
    let dt = 1000.0 / 60.0;
    let frames = (game_secs * 60.0) as u64;
    let mut slow = 0;
    let mut total_ms = 0.0;
    for f in 0..frames {
        let t = Instant::now();
        {
            let mut p = player.lock().unwrap();
            if let Ok(script) = std::env::var("SMBC_KEYS") {
                use ruffle_core::events::{KeyDescriptor, KeyLocation, LogicalKey, NamedKey, PhysicalKey, PlayerEvent};
                for item in script.split(',') {
                    let mut parts = item.split(':');
                    let fr: u64 = parts.next().unwrap().parse().unwrap();
                    let key = parts.next().unwrap();
                    let dur: u64 = parts.next().map(|d| d.parse().unwrap()).unwrap_or(6);
                    let (pk, lk) = match key {
                        "enter" => (PhysicalKey::Enter, LogicalKey::Named(NamedKey::Enter)),
                        "z" => (PhysicalKey::KeyZ, LogicalKey::Character('z')),
                        "x" => (PhysicalKey::KeyX, LogicalKey::Character('x')),
                        "right" => (PhysicalKey::ArrowRight, LogicalKey::Named(NamedKey::ArrowRight)),
                        "left" => (PhysicalKey::ArrowLeft, LogicalKey::Named(NamedKey::ArrowLeft)),
                        "up" => (PhysicalKey::ArrowUp, LogicalKey::Named(NamedKey::ArrowUp)),
                        "down" => (PhysicalKey::ArrowDown, LogicalKey::Named(NamedKey::ArrowDown)),
                        _ => panic!("key {key}"),
                    };
                    let k = KeyDescriptor { physical_key: pk, logical_key: lk, key_location: KeyLocation::Standard };
                    if f == fr { p.handle_event(PlayerEvent::KeyDown { key: k }); println!("[{:7.2}s] key {key}", start.elapsed().as_secs_f64()); }
                    if f == fr + dur { p.handle_event(PlayerEvent::KeyUp { key: k }); }
                }
            } else if f >= 600 && f % 300 == 0 {
                use ruffle_core::events::{MouseButton, PlayerEvent};
                p.handle_event(PlayerEvent::MouseMove { x: 256.0, y: 240.0 });
                p.handle_event(PlayerEvent::MouseDown { x: 256.0, y: 240.0, button: MouseButton::Left, index: None });
                p.handle_event(PlayerEvent::MouseUp { x: 256.0, y: 240.0, button: MouseButton::Left });
                println!("[{:7.2}s] click", start.elapsed().as_secs_f64());
            }
            p.run_frame();
            p.update_timers(ruffle_core::FloatDuration::from_millis(dt));
            p.audio_mut().tick();
            p.render();
            if let Ok(every) = std::env::var("SMBC_SHOT_EVERY") {
                let every: u64 = every.parse().unwrap();
                if f % every == 0 {
                    let r = <dyn std::any::Any>::downcast_mut::<WgpuRenderBackend<TextureTarget>>(p.renderer_mut()).unwrap();
                    if let Some(img) = r.capture_frame() { let _ = img.save(format!("/tmp/frames/f{f:05}.png")); }
                }
            }
        }
        executor.run();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        if ms < dt { std::thread::sleep(Duration::from_secs_f64((dt - ms) / 1000.0)); }
        total_ms += ms;
        if let Ok(from) = std::env::var("SMBC_STATS_FROM") {
            let from: u64 = from.parse().unwrap();
            if f >= from { STATS.with(|st| st.borrow_mut().push(ms)); }
            #[cfg(feature = "profile")]
            if f + 1 == from { ruffle_core::profile_reset(); }
        }
        if ms > 30.0 {
            slow += 1;
            if slow < 400 {
                println!("[{:7.2}s] frame {f}: {ms:.1} ms", start.elapsed().as_secs_f64());
            }
        }
        #[cfg(feature = "profile")]
        if f % 300 == 0 {
            println!("[{:7.2}s] frame {f}: gpu syncs so far {}", start.elapsed().as_secs_f64(), ruffle_core::SMBC_GPU_SYNCS.load(std::sync::atomic::Ordering::Relaxed));
        }
        if f % 600 == 0 {
            println!("[{:7.2}s] frame {f} avg so far {:.2} ms", start.elapsed().as_secs_f64(), total_ms / (f + 1) as f64);
        }
    }
    #[cfg(feature = "profile")]
    ruffle_core::profile_dump(80);
    {
        let o = out.lock().unwrap();
        let mut w: Vec<u8> = Vec::new();
        let n = (o.len() * 2) as u32;
        w.extend_from_slice(b"RIFF"); w.extend_from_slice(&(36 + n).to_le_bytes()); w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes()); w.extend_from_slice(&1u16.to_le_bytes()); w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&44100u32.to_le_bytes()); w.extend_from_slice(&(44100u32 * 4).to_le_bytes()); w.extend_from_slice(&4u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes()); w.extend_from_slice(b"data"); w.extend_from_slice(&n.to_le_bytes());
        for v in o.iter() { w.extend_from_slice(&v.to_le_bytes()); }
        std::fs::write("/tmp/smbc_out.wav", w).unwrap();
    }
    STATS.with(|st| {
        let mut v = st.borrow().clone();
        if !v.is_empty() {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let n = v.len();
            println!("frame ms over {n} frames: avg {:.2} p50 {:.2} p90 {:.2} p99 {:.2} max {:.2}",
                v.iter().sum::<f64>() / n as f64, v[n / 2], v[n * 9 / 10], v[n * 99 / 100], v[n - 1]);
        }
    });
    println!("done: {frames} frames in {:.1}s, avg {:.2} ms, slow frames {slow}", start.elapsed().as_secs_f64(), total_ms / frames as f64);
}
