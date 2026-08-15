use std::time::Instant;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = &args[1];
    let rom = contra_assets::NesRom::load(rom_path).expect("rom");
    let mirroring = if rom.vertical_mirroring { contra_nes::Mirroring::Vertical } else { contra_nes::Mirroring::Horizontal };
    let mut nes = contra_nes::Nes::new_with_audio(rom.prg_rom, mirroring, 44100.0);
    let frames = 1800u32; // 30 sim-seconds
    let start = Instant::now();
    for _ in 0..frames {
        nes.run_frame();
        let _ = nes.take_audio_samples();
    }
    let elapsed = start.elapsed();
    let sim_seconds = frames as f64 / 60.0;
    println!("simulated {sim_seconds:.1}s of gameplay ({frames} frames) in {:.3}s wall-clock", elapsed.as_secs_f64());
    println!("avg per-frame: {:.3}ms (budget is 16.667ms for real-time 60fps)", elapsed.as_secs_f64() * 1000.0 / frames as f64);
    println!("realtime factor: {:.2}x (>1.0 means faster than real hardware speed)", sim_seconds / elapsed.as_secs_f64());
}
