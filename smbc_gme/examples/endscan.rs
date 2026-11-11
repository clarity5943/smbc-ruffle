use smbc_gme::{Emu, EmuType};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&a[1]).unwrap();
    let ty = match a[2].as_str() { "kss" => EmuType::Kss, "nsf" => EmuType::Nsf, "spc" => EmuType::Spc, "gbs" => EmuType::Gbs, "sap" => EmuType::Sap, _ => EmuType::Nsfe };
    let secs: i32 = a[3].parse().unwrap();
    let mut e = Emu::new(ty, 44100).unwrap();
    e.load_data(&data).unwrap();
    let n = e.track_count();
    let only: Option<i32> = a.get(4).map(|x| x.parse().unwrap());
    for t in 0..n {
        if only.is_some() && only != Some(t) { continue; }
        let _ = e.start_track(t);
        let mut buf = vec![0i16; 4096];
        let mut ended = None;
        while e.tell() < secs * 1000 {
            let _ = e.play(&mut buf);
            if e.track_ended() { ended = Some(e.tell()); break; }
        }
        println!("track {t}: {}", ended.map(|ms| format!("ENDED at {:.1}s", ms as f64 / 1000.0)).unwrap_or("ok".into()));
    }
}
