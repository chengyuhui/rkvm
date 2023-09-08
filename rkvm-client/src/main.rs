#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::{net::SocketAddr, path::PathBuf};

use anyhow::Result;
use clap::Parser;

use quinn::Endpoint;
use serde::Deserialize;
use tao::{
    event::StartCause,
    event_loop::{ControlFlow, EventLoop},
    menu::{ContextMenu, MenuItemAttributes},
    system_tray::SystemTrayBuilder,
    TrayId,
};

mod client;

fn load_icon(png_data: &[u8]) -> Result<tao::system_tray::Icon> {
    let (icon_rgba, icon_width, icon_height) = {
        let image = image::load_from_memory(png_data)?.into_rgba8();

        let (width, height) = image.dimensions();
        let rgba = image.into_raw();

        (rgba, width, height)
    };

    Ok(tao::system_tray::Icon::from_rgba(
        icon_rgba,
        icon_width,
        icon_height,
    )?)
}

#[derive(Debug, Deserialize)]
struct Config {
    /// IP address or domain name of the server
    address: String,
    /// Port on the server to connect to
    port: u16,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to configuration file, default to `config.toml` in the same directory as the executable
    #[arg(short, long)]
    config: Option<PathBuf>,
    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

async fn tokio_main(config: Config) -> Result<()> {
    let remote_addr = SocketAddr::new(config.address.parse()?, config.port);

    let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())?;
    endpoint.set_default_client_config(client::configure_client());

    let mut sleep_secs = 1;

    loop {
        if let Err(e) = client::connect(&endpoint, remote_addr).await {
            log::error!("Error handling connection: {}", e);
        }

        log::info!("Reconnecting in {} seconds", sleep_secs);
        tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
        sleep_secs *= 2;
        if sleep_secs > 30 {
            sleep_secs = 30;
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut logger = simple_logger::SimpleLogger::new();
    if args.verbose {
        logger = logger.with_level(log::LevelFilter::Trace);
    } else {
        logger = logger.with_level(log::LevelFilter::Info);
    }
    logger.init()?;

    let config_path = if let Some(p) = args.config {
        p
    } else {
        std::env::current_exe()?.with_file_name("config.toml")
    };

    let config_string = std::fs::read_to_string(config_path)?;
    let config: Config = toml::from_str(&config_string)?;

    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio_rt.spawn(async move {
        if let Err(e) = tokio_main(config).await {
            log::error!("Error in tokio_main: {}", e);

            std::process::exit(1);
        }
    });

    let event_loop = EventLoop::new();

    let main_tray_id = TrayId::new("main-tray");
    let mut tray_menu = ContextMenu::new();
    let quit_item = tray_menu.add_item(MenuItemAttributes::new("Quit"));

    let icon = load_icon(include_bytes!("./icon.png"))?;

    let system_tray = SystemTrayBuilder::new(icon, Some(tray_menu))
        .with_id(main_tray_id)
        .with_tooltip("RKVM Client")
        .build(&event_loop)
        .unwrap();

    event_loop.run(move |event, _event_loop, control_flow| {
        let _ = tokio_rt;
        let _ = system_tray;

        *control_flow = ControlFlow::Wait;

        match event {
            tao::event::Event::NewEvents(StartCause::Init) => {}
            tao::event::Event::MenuEvent {
                menu_id,
                // specify only context menu's
                origin: tao::menu::MenuType::ContextMenu,
                ..
            } => {
                if menu_id == quit_item.clone().id() {
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}
