// MacAtrium headless Snow harness.
//
// Boots a Macintosh II (ROM + Macintosh Display Card 8*24 ROM) with a SCSI hard
// disk attached, runs for a cycle budget, periodically dumps the framebuffer to
// PNG, and can tap a scripted sequence of keys at given cycle marks. This is the
// no-GUI observation path for verifying the launch-return keystone and the MVP
// launcher (the dev machine has no display server).
//
// Usage:
//   macatrium_harness <rom> <mdc_rom> <hdd.img> <out_dir> <max_cycles> \
//       [--snap-every N] [--keys "CYCLE:KEY;CYCLE:KEY;..."] [--wall-secs S]
//
// KEY names: l f r q enter return esc up down left right space  (lowercase)

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, bail};

use snow_core::emulator::Emulator;
use snow_core::emulator::comm::{EmulatorCommand, EmulatorEvent, EmulatorSpeed};
use snow_core::keymap::{KeyEvent, Keymap};
use snow_core::mac::{ExtraROMs, MacModel};
use snow_core::tickable::Tickable;

fn scancode(name: &str) -> Option<u8> {
    Some(match name {
        "a" => 0x00, "s" => 0x01, "d" => 0x02, "f" => 0x03, "h" => 0x04,
        "g" => 0x05, "z" => 0x06, "x" => 0x07, "c" => 0x08, "v" => 0x09,
        "b" => 0x0B, "q" => 0x0C, "w" => 0x0D, "e" => 0x0E, "r" => 0x0F,
        "y" => 0x10, "t" => 0x11, "o" => 0x1F, "u" => 0x20, "i" => 0x22,
        "p" => 0x23, "l" => 0x25, "j" => 0x26, "k" => 0x28, "n" => 0x2D,
        "m" => 0x2E,
        "space" => 0x31,
        "enter" | "return" => 0x24,
        "esc" => 0x35,
        "up" => 0x3E, "down" => 0x3D, "left" => 0x3B, "right" => 0x3C,
        _ => return None,
    })
}

fn write_png(path: &str, w: u16, h: u16, rgba: &[u8]) -> Result<()> {
    let mut enc = png::Encoder::new(File::create(path)?, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header()?;
    wr.write_image_data(rgba)?;
    Ok(())
}

fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    let a: Vec<String> = std::env::args().collect();
    if a.len() < 6 {
        bail!("usage: macatrium_harness <rom> <mdc_rom> <hdd> <out_dir> <max_cycles> [--snap-every N] [--keys CYCLE:KEY;...] [--wall-secs S]");
    }
    let rom_path = &a[1];
    let mdc_path = &a[2];
    let hdd_path = &a[3];
    let out_dir = &a[4];
    let max_cycles: u64 = a[5].parse()?;

    let mut snap_every: u64 = 100_000_000;
    let mut wall_secs: u64 = 1800;
    // schedule[cycle] = (scancode, down?)
    let mut schedule: BTreeMap<u64, Vec<(u8, bool)>> = BTreeMap::new();
    let mut i = 6;
    while i < a.len() {
        match a[i].as_str() {
            "--snap-every" => { snap_every = a[i + 1].parse()?; i += 2; }
            "--wall-secs"  => { wall_secs  = a[i + 1].parse()?; i += 2; }
            "--keys" => {
                const CMD: u8 = 0x37; // Command (universal scancode)
                for tok in a[i + 1].split(';').filter(|s| !s.is_empty()) {
                    let (c, k) = tok.split_once(':').expect("CYCLE:KEY");
                    let cyc: u64 = c.parse()?;
                    if let Some(base) = k.strip_prefix("cmd-") {
                        // Command-modified chord: Cmd down, key tap, Cmd up.
                        let sc = scancode(base).unwrap_or_else(|| panic!("unknown key {base}"));
                        schedule.entry(cyc).or_default().push((CMD, true));
                        schedule.entry(cyc + 1_000_000).or_default().push((sc, true));
                        schedule.entry(cyc + 3_000_000).or_default().push((sc, false));
                        schedule.entry(cyc + 4_000_000).or_default().push((CMD, false));
                    } else {
                        let sc = scancode(k).unwrap_or_else(|| panic!("unknown key {k}"));
                        // press now, release ~3M cycles later (a few ms)
                        schedule.entry(cyc).or_default().push((sc, true));
                        schedule.entry(cyc + 3_000_000).or_default().push((sc, false));
                    }
                }
                i += 2;
            }
            other => bail!("unknown arg {other}"),
        }
    }

    fs::create_dir_all(out_dir)?;

    let rom = fs::read(rom_path)?;
    let mdc = fs::read(mdc_path)?;
    let model = MacModel::detect_from_rom(&rom).expect("cannot detect model from ROM");
    log::info!("Detected model: {model}");

    let extra = [ExtraROMs::MDC12(&mdc)];
    let (mut emu, frame_recv) = Emulator::new(&rom, &extra, model)?;
    let cmd = emu.create_cmd_sender();
    let events = emu.create_event_recv();

    cmd.send(EmulatorCommand::ScsiAttachHdd(0, PathBuf::from(hdd_path)))?;
    cmd.send(EmulatorCommand::Run)?;
    cmd.send(EmulatorCommand::SetSpeed(EmulatorSpeed::Uncapped))?;

    let start = Instant::now();
    let mut next_snap: u64 = snap_every;
    let mut last_frame: Option<(u16, u16, Vec<u8>)> = None;
    let mut snap_idx = 0u32;
    let mut fired: Vec<u64> = schedule.keys().copied().collect();
    fired.sort_unstable();
    let mut fire_i = 0usize;

    loop {
        let cyc = emu.get_cycles();
        if cyc >= max_cycles { break; }
        if start.elapsed().as_secs() >= wall_secs {
            log::warn!("wall-clock limit reached at {cyc} cycles");
            break;
        }

        // drain frames, keep the latest
        loop {
            let taken = { frame_recv.lock().unwrap().take() };
            match taken {
                Some(buf) => {
                    let (w, h) = (buf.width(), buf.height());
                    last_frame = Some((w, h, buf.into_inner()));
                }
                None => break,
            }
        }

        // drain events (so the channel doesn't back up)
        while let Ok(ev) = events.try_recv() {
            if let EmulatorEvent::Status(s) = ev {
                if !s.running && s.cycles > 100 {
                    log::warn!("emulator stopped at {} cycles", s.cycles);
                }
            }
        }

        // fire any scheduled key edges that are due
        while fire_i < fired.len() && fired[fire_i] <= cyc {
            let at = fired[fire_i];
            for (sc, down) in schedule.get(&at).unwrap() {
                let ev = if *down {
                    KeyEvent::KeyDown(*sc, Keymap::Universal)
                } else {
                    KeyEvent::KeyUp(*sc, Keymap::Universal)
                };
                cmd.send(EmulatorCommand::KeyEvent(ev))?;
                log::info!("cyc {at}: key sc=0x{sc:02X} down={down}");
            }
            fire_i += 1;
        }

        // periodic snapshot
        if cyc >= next_snap {
            if let Some((w, h, ref px)) = last_frame {
                let p = format!("{out_dir}/snap_{snap_idx:03}_{cyc}.png");
                write_png(&p, w, h, px)?;
                log::info!("snapshot {p} ({w}x{h})");
            }
            snap_idx += 1;
            next_snap += snap_every;
        }

        emu.tick(1, ())?;
    }

    if let Some((w, h, ref px)) = last_frame {
        let p = format!("{out_dir}/final.png");
        write_png(&p, w, h, px)?;
        log::info!("final {p} ({w}x{h}) after {} cycles", emu.get_cycles());
    } else {
        log::warn!("no frames captured");
    }
    log::info!("done in {:.1}s", start.elapsed().as_secs_f64());
    Ok(())
}
