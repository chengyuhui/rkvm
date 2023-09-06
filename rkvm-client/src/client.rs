use anyhow::Result;
use arboard::{Clipboard, ImageData};
use enigo::{Enigo, KeyboardControllable, MouseControllable};
use keycode::KeyMap;
use tokio::{
    io::{AsyncReadExt, BufReader},
    net::ToSocketAddrs,
};

#[cfg(target_os = "windows")]
fn convert_keycode(code: u16) -> Option<u16> {
    let vk = unsafe {
        windows::Win32::UI::Input::KeyboardAndMouse::MapVirtualKeyW(
            code as u32,
            windows::Win32::UI::Input::KeyboardAndMouse::MAPVK_VSC_TO_VK_EX,
        )
    };
    if vk == 0 {
        None
    } else {
        Some(vk as u16)
    }
}

#[cfg(not(target_os = "windows"))]
fn convert_keycode(code: u16) -> Option<u16> {
    Some(code)
}

#[cfg(target_os = "windows")]
fn move_mouse_relative(_enigo: &mut Enigo, dx: i32, dy: i32) {
    use windows::Win32::{
        Foundation::POINT,
        UI::{
            Input::KeyboardAndMouse,
            WindowsAndMessaging::{GetCursorPos, GetSystemMetrics, SetCursorPos},
        },
    };

    let mut mouse_input = KeyboardAndMouse::INPUT_0::default();
    mouse_input.mi.dx = dx;
    mouse_input.mi.dy = dy;
    mouse_input.mi.dwFlags = KeyboardAndMouse::MOUSEEVENTF_MOVE;

    let input = KeyboardAndMouse::INPUT {
        r#type: KeyboardAndMouse::INPUT_MOUSE,
        Anonymous: mouse_input,
    };

    unsafe {
        KeyboardAndMouse::SendInput(
            &[input],
            std::mem::size_of::<KeyboardAndMouse::INPUT>() as i32,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn move_mouse_relative(enigo: &mut Enigo, dx: i32, dy: i32) {
    enigo.mouse_move_relative(dx, dy);
}

pub async fn connect<A>(remote_addr: A, enigo: &mut Enigo) -> Result<()>
where
    A: ToSocketAddrs + std::fmt::Debug,
{
    log::info!("Connecting to {:?}", remote_addr);

    let stream = tokio::net::TcpStream::connect(remote_addr).await?;
    let mut stream = BufReader::new(stream);

    log::info!("Connection established");

    let mut clipboard = match Clipboard::new() {
        Ok(c) => Some(c),
        Err(e) => {
            log::error!("Failed to open clipboard: {}", e);
            None
        }
    };

    let mut buf = vec![0u8; 128];

    loop {
        let len = stream.read_u32().await?;
        buf.resize(len as usize, 0);
        stream.read_exact(&mut buf).await?;

        let packet = rkvm_protocol::Packet::from_slice(&buf)?;

        if packet.event.is_high_freq() {
            log::trace!("Received event {}: {:?}", packet.id, packet.event);
        } else {
            log::debug!("Received event {}: {:?}", packet.id, packet.event);
        }

        match packet.event {
            rkvm_protocol::Event::MouseMotion { dx, dy } => {
                move_mouse_relative(enigo, dx, dy);
            }
            rkvm_protocol::Event::MouseWheel { dx, dy } => {
                if dx != 0 {
                    enigo.mouse_scroll_x(dx);
                }
                if dy != 0 {
                    enigo.mouse_scroll_y(dy);
                }
            }
            rkvm_protocol::Event::MouseButton { button, pressed } => {
                let button = match button {
                    rkvm_protocol::MouseButton::Left => enigo::MouseButton::Left,
                    rkvm_protocol::MouseButton::Middle => enigo::MouseButton::Middle,
                    rkvm_protocol::MouseButton::Right => enigo::MouseButton::Right,
                };

                if pressed {
                    enigo.mouse_down(button);
                } else {
                    enigo.mouse_up(button);
                }
            }
            rkvm_protocol::Event::Keyboard { key, pressed } => {
                let keymap = if let Ok(km) = KeyMap::from_key_mapping(keycode::KeyMapping::Win(key))
                {
                    km
                } else {
                    continue;
                };

                let raw_key = if cfg!(target_os = "windows") {
                    if let Some(vk) = convert_keycode(keymap.win) {
                        vk
                    } else {
                        log::warn!("Unknown windows scan code: {}", keymap.win);
                        continue;
                    }
                } else if cfg!(target_os = "macos") {
                    keymap.mac
                } else {
                    keymap.xkb
                };

                if pressed {
                    log::debug!("[{}] Key {:?} pressed", packet.id, keymap.id);
                    enigo.key_down(enigo::Key::Raw(raw_key));
                } else {
                    log::debug!("[{}] Key {:?} released", packet.id, keymap.id);
                    enigo.key_up(enigo::Key::Raw(raw_key));
                }
            }
            rkvm_protocol::Event::TextClipboard { content } => {
                if let Some(c) = &mut clipboard {
                    if let Err(e) = c.set_text(content) {
                        log::error!("Failed to set clipboard: {}", e);
                    }
                }
            }
            rkvm_protocol::Event::HtmlClipboard { html, plain } => {
                if let Some(c) = &mut clipboard {
                    if let Err(e) = c.set_html(html, Some(plain)) {
                        log::error!("Failed to set clipboard: {}", e);
                    }
                }
            }
            rkvm_protocol::Event::ImageClipboard { png } => {
                let png_image = match image::load_from_memory(&png) {
                    Ok(i) => i,
                    Err(e) => {
                        log::error!("Failed to decode clipboard image: {}", e);
                        continue;
                    }
                };

                let rgba8 = png_image.into_rgba8();
                let (width, height) = rgba8.dimensions();
                let data = rgba8.into_raw();

                if let Some(c) = &mut clipboard {
                    if let Err(e) = c.set_image(ImageData {
                        width: width as usize,
                        height: height as usize,
                        bytes: std::borrow::Cow::Owned(data),
                    }) {
                        log::error!("Failed to set clipboard: {}", e);
                    }
                }
            }
        }
    }
}
