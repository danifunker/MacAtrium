// MacAtrium headless Snow harness.
//
// Boots the Mac model auto-detected from <rom>: a Macintosh II (which needs the
// Macintosh Display Card 8*24 ROM for a framebuffer) or a compact 68000 model —
// Plus / SE / Classic, built-in 512x342 1-bit video, no NuBus, so pass "-" for
// <mdc_rom>. A SCSI hard disk is attached; it runs for a cycle budget, periodically
// dumps the framebuffer to PNG, and can tap a scripted sequence of keys at given
// cycle marks. This is the no-GUI observation path for verifying the launch-return
// keystone and the launcher (the dev machine has no display server). The compact
// (no-Color-QD) path is what verifies the Mac Plus/SE display guards.
//
// Usage:
//   macatrium_harness <rom> <mdc_rom|-> <hdd.img> <out_dir> <max_cycles> \
//       [--snap-every N] [--keys "CYCLE:KEY;CYCLE:KEY;..."] [--wall-secs S] \
//       [--pram FILE] [--disk2 FILE] [--cdrom ISO] [--cd-dir DIR] [--shared-dir DIR]
//
// --cd-dir DIR exposes a folder of CD images to the guest via the BlueSCSI Toolbox
// (LIST CDS / SET NEXT CD): the launcher enumerates it and switches the disc in the
// id-3 CD-ROM drive programmatically. A CD drive is attached at id 3 if --cdrom
// wasn't given, so SET NEXT CD has a drive to (re)mount into.
//
// --shared-dir DIR is the OTHER half of the Toolbox: the SD-card "shared folder" the
// file ops work on (LIST/COUNT FILES, GET FILE, SEND FILE PREP/DATA/END), which is how
// the SD-card file browser and its copy in/out are exercised (docs/46). It is a
// separate directory from --cd-dir on purpose — the firmware keeps the two apart, so
// browsing files can never disturb CD switching.
//
// KEY names: l f r q enter return esc up down left right space  (lowercase)
// A click is scheduled with KEY = `click@X,Y` (absolute framebuffer pixels), e.g.
//   --keys "2500000000:click@320,160;3000000000:click@600,300"
// A press-and-hold (for auto-repeat) is KEY = `hold@X,Y,DUR` — button down DUR
// cycles before release, e.g. holding a scroll arrow:  4000000:hold@621,223,40000000
//
// --pram FILE persists PRAM in FILE (created if absent). Requires the harness to
// be built with snow_core's `mmap` feature; without it persist_pram cannot write
// back (and SCSI disks are read-only too).

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

/// A scheduled input action fired at a given cycle mark.
#[derive(Clone, Copy)]
enum Act {
    Key(u8, bool),      // scancode, is-down
    MouseAbs(u16, u16), // warp cursor to absolute framebuffer pixel (x, y)
    MouseBtn(bool),     // mouse button down/up (position unchanged)
}

fn scancode(name: &str) -> Option<u8> {
    Some(match name {
        "a" => 0x00, "s" => 0x01, "d" => 0x02, "f" => 0x03, "h" => 0x04,
        "g" => 0x05, "z" => 0x06, "x" => 0x07, "c" => 0x08, "v" => 0x09,
        "b" => 0x0B, "q" => 0x0C, "w" => 0x0D, "e" => 0x0E, "r" => 0x0F,
        "y" => 0x10, "t" => 0x11, "o" => 0x1F, "u" => 0x20, "i" => 0x22,
        "p" => 0x23, "l" => 0x25, "j" => 0x26, "k" => 0x28, "n" => 0x2D,
        "m" => 0x2E,
        "space" => 0x31,
        "tab" => 0x30,
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
    let mut pram_path: Option<String> = None;
    let mut disk2: Option<String> = None; // 2nd SCSI disk (docs/37 multi-disk verify)
    let mut cdrom: Option<String> = None; // SCSI CD-ROM image (ISO/TOAST) — run-from-CD games
    let mut cd_dir: Option<String> = None; // BlueSCSI Toolbox CD-image folder (LIST CDS / SET NEXT CD)
    let mut shared_dir: Option<String> = None; // Toolbox shared folder for the file ops (docs/46)
    // schedule[cycle] = input actions due at that cycle
    let mut schedule: BTreeMap<u64, Vec<Act>> = BTreeMap::new();
    let mut i = 6;
    while i < a.len() {
        match a[i].as_str() {
            "--snap-every" => { snap_every = a[i + 1].parse()?; i += 2; }
            "--wall-secs"  => { wall_secs  = a[i + 1].parse()?; i += 2; }
            "--pram"       => { pram_path  = Some(a[i + 1].clone()); i += 2; }
            "--disk2"      => { disk2      = Some(a[i + 1].clone()); i += 2; }
            "--cdrom"      => { cdrom      = Some(a[i + 1].clone()); i += 2; }
            "--cd-dir"     => { cd_dir     = Some(a[i + 1].clone()); i += 2; }
            "--shared-dir" => { shared_dir = Some(a[i + 1].clone()); i += 2; }
            "--keys" => {
                const CMD: u8 = 0x37; // Command (universal scancode)
                const OPT: u8 = 0x3A; // Option
                for tok in a[i + 1].split(';').filter(|s| !s.is_empty()) {
                    let (c, k) = tok.split_once(':').expect("CYCLE:KEY");
                    let cyc: u64 = c.parse()?;
                    if let Some(coords) = k.strip_prefix("click@") {
                        // Mouse click at absolute framebuffer (x,y): warp the cursor,
                        // then press + release the button a few ms apart.
                        let (xs, ys) = coords.split_once(',').expect("click@X,Y");
                        let x: u16 = xs.parse()?;
                        let y: u16 = ys.parse()?;
                        schedule.entry(cyc).or_default().push(Act::MouseAbs(x, y));
                        schedule.entry(cyc + 1_000_000).or_default().push(Act::MouseBtn(true));
                        schedule.entry(cyc + 3_000_000).or_default().push(Act::MouseBtn(false));
                    } else if let Some(coords) = k.strip_prefix("dclick@") {
                        // Double-click: warp, then two press/release pairs ~1M cycles
                        // apart (well within GetDblTime) to open/launch an icon.
                        let (xs, ys) = coords.split_once(',').expect("dclick@X,Y");
                        let x: u16 = xs.parse()?;
                        let y: u16 = ys.parse()?;
                        schedule.entry(cyc).or_default().push(Act::MouseAbs(x, y));
                        schedule.entry(cyc + 500_000).or_default().push(Act::MouseBtn(true));
                        schedule.entry(cyc + 1_000_000).or_default().push(Act::MouseBtn(false));
                        schedule.entry(cyc + 1_500_000).or_default().push(Act::MouseBtn(true));
                        schedule.entry(cyc + 2_000_000).or_default().push(Act::MouseBtn(false));
                    } else if let Some(coords) = k.strip_prefix("hold@") {
                        // Press-and-hold at (x,y) for DUR cycles, then release. Drives
                        // hold-to-scroll auto-repeat (a scroll-arrow held down) — the
                        // Control Manager fires the control's action proc the whole time.
                        //   hold@X,Y,DUR
                        let mut it = coords.split(',');
                        let x: u16 = it.next().expect("hold@X,Y,DUR").parse()?;
                        let y: u16 = it.next().expect("hold@X,Y,DUR").parse()?;
                        let dur: u64 = it.next().expect("hold@X,Y,DUR").parse()?;
                        schedule.entry(cyc).or_default().push(Act::MouseAbs(x, y));
                        schedule.entry(cyc + 1_000_000).or_default().push(Act::MouseBtn(true));
                        schedule.entry(cyc + 1_000_000 + dur).or_default().push(Act::MouseBtn(false));
                    } else if let Some(coords) = k.strip_prefix("drag@") {
                        // Press at (x1,y1), warp the cursor through to (x2,y2) while held,
                        // then release — drives a click-drag (e.g. a column divider). The
                        // guest's StillDown/GetMouse loop tracks the intermediate warps.
                        //   drag@X1,Y1,X2,Y2
                        let mut it = coords.split(',');
                        let x1: u16 = it.next().expect("drag@X1,Y1,X2,Y2").parse()?;
                        let y1: u16 = it.next().expect("drag@X1,Y1,X2,Y2").parse()?;
                        let x2: u16 = it.next().expect("drag@X1,Y1,X2,Y2").parse()?;
                        let y2: u16 = it.next().expect("drag@X1,Y1,X2,Y2").parse()?;
                        schedule.entry(cyc).or_default().push(Act::MouseAbs(x1, y1));
                        schedule.entry(cyc + 1_000_000).or_default().push(Act::MouseBtn(true));
                        let steps = 8u64;
                        for s in 1..=steps {                 // glide from p1 to p2 while held
                            let x = x1 as i64 + (x2 as i64 - x1 as i64) * s as i64 / steps as i64;
                            let y = y1 as i64 + (y2 as i64 - y1 as i64) * s as i64 / steps as i64;
                            schedule.entry(cyc + 1_000_000 + s * 1_500_000)
                                .or_default().push(Act::MouseAbs(x as u16, y as u16));
                        }
                        schedule.entry(cyc + 1_000_000 + (steps + 2) * 1_500_000)
                            .or_default().push(Act::MouseBtn(false));
                    } else if let Some(base) = k.strip_prefix("cmd-opt-") {
                        // Cmd+Option chord: both modifiers down, key tap, both up.
                        let sc = scancode(base).unwrap_or_else(|| panic!("unknown key {base}"));
                        schedule.entry(cyc).or_default().push(Act::Key(CMD, true));
                        schedule.entry(cyc).or_default().push(Act::Key(OPT, true));
                        schedule.entry(cyc + 1_000_000).or_default().push(Act::Key(sc, true));
                        schedule.entry(cyc + 3_000_000).or_default().push(Act::Key(sc, false));
                        schedule.entry(cyc + 4_000_000).or_default().push(Act::Key(OPT, false));
                        schedule.entry(cyc + 4_000_000).or_default().push(Act::Key(CMD, false));
                    } else if let Some(base) = k.strip_prefix("cmd-") {
                        // Command-modified chord: Cmd down, key tap, Cmd up.
                        let sc = scancode(base).unwrap_or_else(|| panic!("unknown key {base}"));
                        schedule.entry(cyc).or_default().push(Act::Key(CMD, true));
                        schedule.entry(cyc + 1_000_000).or_default().push(Act::Key(sc, true));
                        schedule.entry(cyc + 3_000_000).or_default().push(Act::Key(sc, false));
                        schedule.entry(cyc + 4_000_000).or_default().push(Act::Key(CMD, false));
                    } else {
                        let sc = scancode(k).unwrap_or_else(|| panic!("unknown key {k}"));
                        // press now, release ~3M cycles later (a few ms)
                        schedule.entry(cyc).or_default().push(Act::Key(sc, true));
                        schedule.entry(cyc + 3_000_000).or_default().push(Act::Key(sc, false));
                    }
                }
                i += 2;
            }
            other => bail!("unknown arg {other}"),
        }
    }

    fs::create_dir_all(out_dir)?;

    let rom = fs::read(rom_path)?;
    let model = MacModel::detect_from_rom(&rom).expect("cannot detect model from ROM");
    log::info!("Detected model: {model}");

    // The Macintosh Display Card ROM is a Mac II NuBus card. Compact models
    // (Plus / SE / Classic) have built-in video and no NuBus, so pass "-" as
    // <mdc_rom> to skip it — the compact bus ignores the extra ROM regardless.
    // A Mac II needs it for a framebuffer.
    let mdc: Option<Vec<u8>> = if mdc_path.as_str() == "-" {
        None
    } else {
        Some(fs::read(mdc_path)?)
    };
    let extra: Vec<ExtraROMs> = match mdc {
        Some(ref m) => vec![ExtraROMs::MDC12(m.as_slice())],
        None => vec![],
    };
    let (mut emu, frame_recv) = Emulator::new(&rom, &extra, model)?;

    // Persist PRAM across runs (boot depth / monitor settings live in slot PRAM).
    // Needs the `mmap` feature or this is a silent no-op (load-only, no write-back).
    if let Some(ref p) = pram_path {
        emu.persist_pram(std::path::Path::new(p));
        log::info!("PRAM persisted in {p}");
    }

    let cmd = emu.create_cmd_sender();
    let events = emu.create_event_recv();

    cmd.send(EmulatorCommand::ScsiAttachHdd(0, PathBuf::from(hdd_path)))?;
    if let Some(ref d2) = disk2 {
        cmd.send(EmulatorCommand::ScsiAttachHdd(1, PathBuf::from(d2)))?;
        log::info!("attached 2nd SCSI disk (id 1): {d2}");
    }
    if let Some(ref cd) = cdrom {
        // SCSI CD-ROM at id 3 (Apple's default). The emulated System needs the
        // 'Apple CD-ROM' + 'ISO 9660/Foreign File Access' extensions; the drive is
        // recognized at cold boot (which this is). ISO/TOAST/CUE-BIN supported.
        cmd.send(EmulatorCommand::ScsiAttachCdrom(3))?;
        cmd.send(EmulatorCommand::ScsiLoadMedia(3, PathBuf::from(cd)))?;
        log::info!("attached SCSI CD-ROM (id 3) with media: {cd}");
    }
    if let Some(ref dir) = cd_dir {
        // BlueSCSI Toolbox CD switching: LIST CDS enumerates this folder and
        // SET NEXT CD remounts a chosen image on the CD-ROM drive. Ensure a CD
        // drive exists at id 3 (empty if --cdrom wasn't given) for the remount.
        if cdrom.is_none() {
            cmd.send(EmulatorCommand::ScsiAttachCdrom(3))?;
        }
        cmd.send(EmulatorCommand::SetCdDir(Some(PathBuf::from(dir))))?;
        log::info!("toolbox CD-image dir (id 3): {dir}");
    }
    if let Some(ref dir) = shared_dir {
        // BlueSCSI Toolbox shared folder — the SD-card side the file ops read and
        // write (docs/46). Needs no drive of its own: the file commands are answered
        // by the Toolbox device itself, not by a CD/disk target.
        cmd.send(EmulatorCommand::SetSharedDir(Some(PathBuf::from(dir))))?;
        log::info!("toolbox shared dir (files): {dir}");
    }
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
            for act in schedule.get(&at).unwrap() {
                match *act {
                    Act::Key(sc, down) => {
                        let ev = if down {
                            KeyEvent::KeyDown(sc, Keymap::Universal)
                        } else {
                            KeyEvent::KeyUp(sc, Keymap::Universal)
                        };
                        cmd.send(EmulatorCommand::KeyEvent(ev))?;
                        log::info!("cyc {at}: key sc=0x{sc:02X} down={down}");
                    }
                    Act::MouseAbs(x, y) => {
                        cmd.send(EmulatorCommand::MouseUpdateAbsolute { x, y })?;
                        log::info!("cyc {at}: mouse abs ({x},{y})");
                    }
                    Act::MouseBtn(down) => {
                        cmd.send(EmulatorCommand::MouseUpdateRelative {
                            relx: 0,
                            rely: 0,
                            btn: Some(down),
                        })?;
                        log::info!("cyc {at}: mouse btn down={down}");
                    }
                }
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
