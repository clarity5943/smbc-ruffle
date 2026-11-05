use smbc_gme::{Emu, EmuType};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&a[1]).unwrap();
    let ty = match a[2].as_str() { "kss" => EmuType::Kss, "nsf" => EmuType::Nsf, "spc" => EmuType::Spc, "gbs" => EmuType::Gbs, "sap" => EmuType::Sap, _ => EmuType::Nsfe };
    let mut e = Emu::new(ty, 44100).unwrap();
    println!("load {:?}", e.load_data(&data));
    println!("tracks {}", e.track_count());
    let lim: i32 = std::env::args().nth(3).map(|x| x.parse().unwrap()).unwrap_or(4);
    for t in 0..e.track_count().min(lim) {
        let _ = e.start_track(t);
        let mut buf = vec![0i16; 44100 * 2 * 3];
        let _ = e.play(&mut buf);
        let rms = (buf.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / buf.len() as f64).sqrt();
        if rms > 0.0 { println!("track {t} rms {rms:.0}"); }
    }
}
