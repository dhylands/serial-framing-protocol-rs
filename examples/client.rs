use std::io;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};

use pretty_hex::*;

use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "server")]
struct Opt {
    /// TCP port to listen on
    #[structopt(short, long, default_value = "3300")]
    port: u32,

    /// Turn on debugging
    #[structopt(short, long)]
    debug: bool,

    /// Turn on verbose messages
    #[structopt(short, long)]
    verbose: bool,
}

fn handle_connection(mut stream: TcpStream) -> io::Result<()> {
    println!("Connected to: {}", stream.peer_addr().unwrap());
    let mut buf = [0u8; 4096];
    stream.write(b"Hello World")?;

    let bytes_read = stream.read(&mut buf)?;
    if bytes_read == 0 {
        // Is it possible? Or IoError will be raised anyway?
        return Ok(());
    }
    println!("Read: {:?}", (&buf[0..bytes_read]).hex_dump());

    stream.shutdown(Shutdown::Both)?;

    Ok(())
}

fn main() {
    let opt = Opt::from_args();

    if opt.verbose {
        println!("{:#?}", opt);
    }

    let server_addr = format!("127.0.0.1:{}", opt.port);
    match TcpStream::connect(server_addr) {
        Ok(stream) => {
            handle_connection(stream).unwrap();
        }
        Err(e) => {
            println!("Error writing data {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::{error, info, warn};
    use simple_logger;
    use std::sync::Once;
    use std::vec::Vec;

    static INIT: Once = Once::new();

    fn setup() {
        INIT.call_once(|| {
            simple_logger::init().unwrap();
        });
    }
}
