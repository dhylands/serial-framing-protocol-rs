use std::io;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

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
    println!("Client connected from: {}", stream.peer_addr().unwrap());
    stream.set_read_timeout(Some(Duration::new(5, 0)))?;
    let mut buf = [0u8; 4096];
    loop {
        let bytes_read = stream.read(&mut buf)?;
        if bytes_read == 0 {
            // Is it possible? Or IoError will be raised anyway?
            break;
        }
        println!("Read: {:?}", (&buf[0..bytes_read]).hex_dump());

        stream.write(&buf[0..bytes_read])?;
    }
    Ok(())
}

fn main() {
    let opt = Opt::from_args();

    if opt.verbose {
        println!("{:#?}", opt);
    }

    let server_addr = format!("0.0.0.0:{}", opt.port);
    let listener = TcpListener::bind(server_addr).unwrap();
    println!("Server listening on port {} ...", opt.port);
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                handle_connection(s).unwrap();
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
    }
}
