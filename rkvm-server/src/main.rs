use clap::{Parser, ValueEnum};
use input::event::keyboard::KeyboardEventTrait;
use input::event::pointer::{Axis, PointerScrollEvent};
use input::event::tablet_pad::KeyState;
use input::event::EventTrait;
use input::{Libinput, LibinputInterface};
use keycode::{KeyMap, KeyMappingId};
use nix::poll::{PollFd, PollFlags};
use nix::{ioctl_write_int_bad, request_code_write};
use rkvm_protocol::Packet;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::{fs::OpenOptionsExt, io::OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

use libc::{O_RDONLY, O_RDWR, O_WRONLY};

#[derive(Debug, Hash)]
pub enum ClipboardType {
    PngImage(Vec<u8>),
    Utf8Text(String),
    HtmlText { html: String, plain: String },
}

mod server;
mod wayland;
mod xclip;

// https://github.com/torvalds/linux/blob/68e77ffbfd06ae3ef8f2abf1c3b971383c866983/include/uapi/linux/input.h#L186
ioctl_write_int_bad!(eviocgrab, request_code_write!('E', 0x90, 4));

lazy_static::lazy_static! {
    static ref DEVICES: Mutex<HashMap<PathBuf, RawFd>> = Mutex::new(HashMap::new());
}

static CLIPBOARD_TIMESTAMP: AtomicU64 = AtomicU64::new(0);

struct Interface;

impl LibinputInterface for Interface {
    #[allow(clippy::bad_bit_mask)]
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        log::info!("Opening {:?} with flags {}", path, flags);

        let fd: OwnedFd = OpenOptions::new()
            .custom_flags(flags)
            .read((flags & O_RDONLY != 0) | (flags & O_RDWR != 0))
            .write((flags & O_WRONLY != 0) | (flags & O_RDWR != 0))
            .open(path)
            .map(|file| file.into())
            .map_err(|err| {
                log::error!("Failed to open {:?}: {}", path, err);
                err.raw_os_error().unwrap_or_default()
            })?;

        DEVICES
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), fd.as_raw_fd());

        Ok(fd)
    }

    fn close_restricted(&mut self, fd: OwnedFd) {
        DEVICES.lock().unwrap().retain(|_, v| *v != fd.as_raw_fd());

        let _ = File::from(fd);
    }
}

fn grab_devices(grab: bool) {
    let devices = DEVICES.lock().unwrap();

    for (path, device) in devices.iter() {
        match unsafe { eviocgrab(*device, grab.into()) } {
            Ok(_) => {}
            Err(e) => {
                log::error!(
                    "Failed to {} {}: {}",
                    if grab { "grab" } else { "ungrab" },
                    path.display(),
                    e
                );
            }
        }
    }
}

async fn get_clipboard_content(
    event_tx: tokio::sync::mpsc::Sender<Packet>,
    mode: ClipboardMode,
) -> anyhow::Result<()> {
    let content = match mode {
        ClipboardMode::X11 => {
            let timestamp = xclip::get_xclip_timestamp().await?;
            if let Some(ts) = timestamp {
                if CLIPBOARD_TIMESTAMP.load(std::sync::atomic::Ordering::Relaxed) == ts {
                    return Ok(());
                }
                CLIPBOARD_TIMESTAMP.store(ts, std::sync::atomic::Ordering::Relaxed);
            }

            if let Some(c) = xclip::get_xclip_clipboard().await? {
                c
            } else {
                return Ok(());
            }
        }
        ClipboardMode::Wayland => {
            let content = if let Some(c) = wayland::get_wayland_clipboard().await? {
                c
            } else {
                return Ok(());
            };

            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            content.hash(&mut hasher);
            let hash = hasher.finish();

            if CLIPBOARD_TIMESTAMP.load(std::sync::atomic::Ordering::Relaxed) == hash {
                return Ok(());
            }
            CLIPBOARD_TIMESTAMP.store(hash, std::sync::atomic::Ordering::Relaxed);

            content
        }
    };

    match content {
        ClipboardType::PngImage(img) => {
            let _ = event_tx
                .send(Packet {
                    id: 0,
                    event: rkvm_protocol::Event::ImageClipboard { png: img },
                })
                .await;
        }
        ClipboardType::Utf8Text(text) => {
            let _ = event_tx
                .send(Packet {
                    id: 0,
                    event: rkvm_protocol::Event::TextClipboard { content: text },
                })
                .await;
        }
        ClipboardType::HtmlText { html, plain } => {
            let _ = event_tx
                .send(Packet {
                    id: 0,
                    event: rkvm_protocol::Event::HtmlClipboard { html, plain },
                })
                .await;
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ClipboardMode {
    X11,
    Wayland,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    #[arg(short, long)]
    clipboard_mode: Option<ClipboardMode>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut logger = simple_logger::SimpleLogger::new();
    if args.verbose {
        logger = logger.with_level(log::LevelFilter::Trace);
    } else {
        logger = logger.with_level(log::LevelFilter::Info);
    }
    logger.init()?;

    let mut grabbed = false;

    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<Packet>(128);

    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    tokio_rt.spawn(async move { server::sender(event_rx).await });
    tokio_rt.spawn(async move {
        if let Err(e) = server::server().await {
            log::error!("Error running server: {}", e);
        }
    });

    let mut libinput = Libinput::new_with_udev(Interface);
    libinput.udev_assign_seat("seat0").unwrap();

    let mut packet_id = 0;

    let mut mouse_dx = 0.0f64;
    let mut mouse_dy = 0.0f64;
    let mut wheel_dx = 0;
    let mut wheel_dy = 0;

    let pollfd = PollFd::new(libinput.as_raw_fd(), PollFlags::POLLIN);

    loop {
        nix::poll::poll(&mut [pollfd], -1)?;
        libinput.dispatch()?;

        for event in &mut libinput {
            let mut event_to_send = None;
            match event {
                input::Event::Device(ev) => {
                    let device = ev.device();
                    let device_name = device.name();
                    match ev {
                        input::event::DeviceEvent::Added(_) => {
                            log::info!("Device added: {}", device_name);
                        }
                        input::event::DeviceEvent::Removed(_) => {
                            log::info!("Device removed: {}", device_name);
                        }
                        _ => {}
                    }
                }
                input::Event::Keyboard(ev) => {
                    // let time = ev.time();
                    let key: u16 = match ev.key().try_into() {
                        Ok(key) => key,
                        Err(_) => {
                            log::warn!("Unknown key that exceeds u16: {}", ev.key());
                            continue;
                        }
                    };
                    let state = ev.key_state();

                    let keymap = match KeyMap::from_key_mapping(keycode::KeyMapping::Evdev(key)) {
                        Ok(keymap) => keymap,
                        Err(_) => {
                            log::warn!("Unknown key: {}", key);
                            continue;
                        }
                    };

                    if keymap.id == KeyMappingId::ControlRight {
                        if state == KeyState::Released {
                            if grabbed {
                                grab_devices(false);
                                grabbed = false;
                                log::info!("Ungrabbed all devices");
                            } else {
                                grab_devices(true);
                                grabbed = true;
                                log::info!("Grabbed all devices");

                                if let Some(mode) = args.clipboard_mode {
                                    // Send clipboard to client
                                    let event_tx = event_tx.clone();
                                    tokio_rt.spawn(async move {
                                        if let Err(e) = get_clipboard_content(event_tx, mode).await
                                        {
                                            log::error!("Failed to send clipboard: {}", e);
                                        }
                                    });
                                }
                            }
                        }

                        // Ignore this key
                        continue;
                    }

                    event_to_send = Some(rkvm_protocol::Event::Keyboard {
                        key: keymap.win,
                        pressed: state == KeyState::Pressed,
                    });
                }
                input::Event::Pointer(ev) => {
                    if !grabbed {
                        continue;
                    }

                    match ev {
                        input::event::PointerEvent::Motion(ev) => {
                            mouse_dx += ev.dx_unaccelerated();
                            mouse_dy += ev.dy_unaccelerated();

                            if mouse_dx.abs() > 1.0 || mouse_dy.abs() > 1.0 {
                                let dx = mouse_dx as i32;
                                let dy = mouse_dy as i32;

                                mouse_dx -= dx as f64;
                                mouse_dy -= dy as f64;

                                event_to_send = Some(rkvm_protocol::Event::MouseMotion { dx, dy });
                            }
                        }
                        input::event::PointerEvent::Button(ev) => {
                            let pressed =
                                ev.button_state() == input::event::pointer::ButtonState::Pressed;
                            let button = match ev.button() {
                                272 => rkvm_protocol::MouseButton::Left,
                                273 => rkvm_protocol::MouseButton::Right,
                                274 => rkvm_protocol::MouseButton::Middle,
                                _ => continue,
                            };
                            event_to_send =
                                Some(rkvm_protocol::Event::MouseButton { button, pressed });
                        }
                        input::event::PointerEvent::ScrollWheel(ev) => {
                            if ev.has_axis(Axis::Horizontal) {
                                wheel_dx += ev.scroll_value_v120(Axis::Horizontal) as i32;
                            }

                            if ev.has_axis(Axis::Vertical) {
                                wheel_dy += ev.scroll_value_v120(Axis::Vertical) as i32;
                            }

                            if wheel_dx.abs() >= 120 || wheel_dy.abs() >= 120 {
                                let dx = wheel_dx / 120;
                                let dy = wheel_dy / 120;

                                wheel_dx -= dx * 120;
                                wheel_dy -= dy * 120;

                                event_to_send = Some(rkvm_protocol::Event::MouseWheel { dx, dy });
                            }
                        }
                        _ => {}
                    }
                }
                _ => {
                    println!("Got event: {:?}", event);
                }
            }

            if let (true, Some(event)) = (grabbed, event_to_send) {
                let _ = event_tx.blocking_send(rkvm_protocol::Packet {
                    id: packet_id,
                    event,
                });
                packet_id = packet_id.wrapping_add(1);
            }
        }
    }
}
