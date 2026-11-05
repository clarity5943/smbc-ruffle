// Builds the bundled Game Music Emu C++ sources (from Jay Pavlina's
// game-music-emu crate, LGPL-2.1). Only the formats SMBC uses are enabled.
fn main() {
    let files = [
        "Blip_Buffer.cpp", "Classic_Emu.cpp", "Data_Reader.cpp", "Dual_Resampler.cpp",
        "Effects_Buffer.cpp", "Fir_Resampler.cpp", "gme.cpp", "Gme_File.cpp",
        "M3u_Playlist.cpp", "Multi_Buffer.cpp", "Music_Emu.cpp",
        // KSS (needs AY + SMS APUs)
        "Ay_Apu.cpp", "Sms_Apu.cpp", "Kss_Cpu.cpp", "Kss_Emu.cpp", "Kss_Scc_Apu.cpp",
        // GBS
        "Gb_Apu.cpp", "Gb_Cpu.cpp", "Gb_Oscs.cpp", "Gbs_Emu.cpp",
        // NSF / NSFE
        "Nes_Apu.cpp", "Nes_Cpu.cpp", "Nes_Fme7_Apu.cpp", "Nes_Namco_Apu.cpp", "Nes_Oscs.cpp",
        "Nes_Vrc6_Apu.cpp", "Nsf_Emu.cpp", "Nsfe_Emu.cpp",
        // SAP
        "Sap_Apu.cpp", "Sap_Cpu.cpp", "Sap_Emu.cpp",
        // SPC
        "Snes_Spc.cpp", "Spc_Cpu.cpp", "Spc_Dsp.cpp", "Spc_Emu.cpp", "Spc_Filter.cpp",
    ];
    let mut build = cc::Build::new();
    build.cpp(true).flag_if_supported("-std=c++11").warnings(false).opt_level(2);
    for d in ["USE_GME_KSS", "USE_GME_GBS", "USE_GME_NSF", "USE_GME_NSFE", "USE_GME_SAP", "USE_GME_SPC"] {
        build.define(d, None);
    }
    for f in files {
        build.file(format!("gme/{f}"));
        println!("cargo:rerun-if-changed=gme/{f}");
    }
    build.compile("smbc_gme");
}
