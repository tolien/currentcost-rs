use std::fs;
use std::io;
use std::process;
use toml::Value;

fn main() {
    let config = parse_config();
    serial::open(&config.port).unwrap_or_else(|err| {
        println!("Problem opening serial port: {}", err);
        process::exit(1);
    });
}

fn read_to_eol(source: &mut dyn io::Read, buffer: &mut String) -> io::Result<usize> {
   source.read_to_string(buffer);

   Ok(buffer.len().clone())
}
#[derive(Debug)]
struct ConnectConfig {
    port: String,
    bit_rate: u32,
    timeout: u32
}

impl<> ConnectConfig<> {
    pub fn new(args: &toml::Value ) -> Result<ConnectConfig<>, &'static str> {
        let port = String::from(args["port"].as_str().unwrap());
        let bit_rate = args["bit_rate"].as_integer().unwrap() as u32;
        let timeout = args["timeout"].as_integer().unwrap() as u32;

        Ok(ConnectConfig {
            port: port,
            bit_rate: bit_rate,
            timeout: timeout, 
        })
    }
}

fn parse_config<>() -> ConnectConfig<> {
    let properties = fs::read_to_string("config.toml").unwrap();
    let values = &properties.parse::<Value>().unwrap();
    let config = ConnectConfig::new(&values["serial"]).unwrap();

    config
}
