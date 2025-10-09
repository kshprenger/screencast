use std::net::Ipv4Addr;

#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
pub(super) struct Args {
    #[arg(long, default_value_t = 35080)]
    pub(super) port: u16,
    #[arg(long)]
    pub(super) address: Ipv4Addr,
}
