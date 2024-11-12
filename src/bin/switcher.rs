use clap::{Command, Arg};
use gstreamer as gst;
use std::net::SocketAddr;
use switcher::http::Server;
use thiserror::Error;

#[derive(Debug, Error)]
enum RTMPSwitcherError {
    #[error("failed setting up gstreamer {0}")]
    FailedInitGstreamer(#[from] gst::glib::Error),

    #[error("invalid listen address `{0}`")]
    InvalidSocketAddr(String),
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt::init();

    let matches = Command::new("rtmpswitcher")
        .version("0.1.0")
        .about("It switches things")
        .arg(
            Arg::new("addr")
                .short('a')
                .long("addr")
                .value_name("ADDRESS")
                .help("sets the server listen address"),
        )
        .get_matches();
    let addr: SocketAddr = parse_addr(matches.get_one::<String>("addr").unwrap_or({
        eprintln!("using 0.0.0.0:3030 as addr");
        &String::from("0.0.0.0:3030")
    }))?;

    gst::init().map_err(RTMPSwitcherError::FailedInitGstreamer)?;

    let server = Server::new_with_config(addr);
    server.run().await;

    Ok(())
}

fn parse_addr(raw_addr: &str) -> Result<SocketAddr, RTMPSwitcherError> {
    raw_addr
        .parse::<SocketAddr>()
        .map_err(|_| RTMPSwitcherError::InvalidSocketAddr(raw_addr.to_string()))
}
