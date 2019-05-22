extern crate roxmltree;
use roxmltree::*;

use serialport;
use serialport::prelude::*;
use std::time::Duration;
use std::error::Error;
use std::fs;
use std::io;
use std::str;
use std::process;
use toml::Value;

fn main() {
    let config = parse_config();
    let mut port = get_serial_port(config).unwrap_or_else(|err| {
        println!("Error opening serial port: {}", err);
        process::exit(1);
    });
    println!("Port name: {}", port.name().unwrap());

        let mut serial_buf: Vec<u8> = vec![0; 1000];
        let mut line: String = String::new();
        println!("Receiving data on {} at {} baud:", port.name().unwrap(), port.baud_rate().unwrap());
        loop {
            match port.read(serial_buf.as_mut_slice()) {
                Ok(t) => {
                    let s = received_bytes_to_string(&serial_buf[..t]);
                    line.push_str(s);
                    if s.contains('\n') {
                        //println!("Newline found! Total string is: {}", line);
                        let parsed_line = parse_line_from_device(&line);
                        if parsed_line.is_ok() {
                            println!("Parsed line: {:?}", parsed_line.unwrap());
                        }
                        else {
                            println!("Received: {}", s);
                        }
                        line = String::new();
                    }

                },
                Err(ref e) if e.kind() == io::ErrorKind::TimedOut => (),
                Err(e) => eprintln!("{:?}", e),
            }
        }
}

fn received_bytes_to_string(bytes: &[u8]) -> &str {
    str::from_utf8(bytes).unwrap_or_else(|err| {
        println!("Error: {}", err);
        ""
    })
}

fn get_serial_port(config: ConnectConfig) -> Result<Box<dyn serialport::SerialPort>, String>  {
    let mut settings: SerialPortSettings = Default::default();
    settings.timeout = Duration::from_millis(config.timeout.into());
    settings.baud_rate = config.bit_rate;

let port = serialport::open_with_settings(&config.port, &settings);
    if port.is_err() {
        let error_description = port.err().unwrap().description;
        Err(format!("Problem opening serial port: {}", error_description))
    }
    else {
        Ok(port.unwrap())
    }

}
fn read_to_eol(source: &mut dyn io::Read, buffer: &mut String) -> io::Result<usize> {
    loop {
        let read_result = source.read_to_string(buffer);
        if read_result.is_ok() {
            println!("Buffer: {}", buffer);
            if buffer.ends_with('\n') {
                return Ok(buffer.len());
            }
        }
        else {
            let read_error = read_result.err().unwrap();
            println!("Read error: {}", read_error);
            return Err(io::Error::new(read_error.kind(), read_error));
        }
    }
}

#[derive(Debug)]
struct ConnectConfig {
    port: String,
    bit_rate: u32,
    timeout: u32,
}

impl ConnectConfig {
    pub fn new(args: &toml::Value) -> Result<ConnectConfig, &'static str> {
        let port = String::from(args["port"].as_str().unwrap());
        let bit_rate = args["bit_rate"].as_integer().unwrap() as u32;
        let timeout = args["timeout"].as_integer().unwrap() as u32;

        Ok(ConnectConfig {
            port,
            bit_rate,
            timeout,
        })
    }
}

#[derive(Debug)]
struct CurrentCostReading {
    device: String,
    sensor: i32,
    temperature: f32,
    power: i32,
}

fn parse_config() -> ConnectConfig {
    let properties = fs::read_to_string("config.toml").unwrap();
    let values = &properties.parse::<Value>().unwrap();

    ConnectConfig::new(&values["serial"]).unwrap()
}

fn get_element_from_xmldoc(root: &Document, element_name: &str, expected_count: usize) -> String {
    let nodes: Vec<Node> = root
        .descendants()
        .filter(|n| n.has_tag_name(element_name))
        .collect();
    assert_eq!(nodes.len(), expected_count);

    let value = nodes[0].text().unwrap();

    String::from(value)
}

fn parse_line_from_device(line: &str) -> Result<CurrentCostReading, Box<dyn Error>> {
    println!("Line: {}", line);
    let doc = Document::parse(line).unwrap();

    let source = get_element_from_xmldoc(&doc, "src", 1);

    let pwr = get_element_from_xmldoc(&doc, "watts", 1);
    let power = pwr.parse::<i32>().unwrap();

    let temp = get_element_from_xmldoc(&doc, "tmpr", 1);
    let temperature = temp.parse::<f32>().unwrap();

    let sens = get_element_from_xmldoc(&doc, "sensor", 1);
    let sensor = sens.parse::<i32>().unwrap();

    let reading = CurrentCostReading {
        device: source,
        sensor ,
        temperature,
        power,
    };

    Ok(reading)
}

#[cfg(test)]
mod tests {
    use super::parse_line_from_device;

    #[test]
    fn line_gets_parsed() {
        let sample_text = " <msg><src>CC128-v1.29</src><dsb>02353</dsb><time>10:27:59</time><tmpr>21.4</tmpr><sensor>0</sensor><id>04066</id><type>1</type><ch1><watts>00479</watts></ch1></msg>";
        let parsed = parse_line_from_device(sample_text).unwrap();

        //assert_eq!(1555188288, parsed.timestamp);
        assert_eq!("CC128-v1.29", parsed.device);
        assert_eq!(0, parsed.sensor);
        assert_eq!(479, parsed.power);
        assert_eq!(21.4, parsed.temperature);
    }

    #[test]
    fn history_line_gets_ignored() {
        let sample_text = "<msg><src>CC128-v1.29</src><dsb>02371</dsb><time>09:23:30</time><hist><dsw>02373</dsw><type>1</type><units>kwhr</units><data><sensor>0</sensor><m003>597.250</m003><m002>681.250</m002><m001>613.250</m001></data><data><sensor>1</sensor><m003>4.750</m003><m002>2.250</m002><m001>2.000</m001></data><data><sensor>2</sensor><m003>0.000</m003><m002>0.000</m002><m001>0.000</m001></data><data><sensor>3</sensor><m003>0.000</m003><m002>0.000</m002><m001>0.000</m001></data><data><sensor>4</sensor><m003>0.000</m003><m002>0.000</m002><m001>0.000</m001></data><data><sensor>5</sensor><m003>0.000</m003><m002>0.000</m002><m001>0.000</m001></data><data><sensor>6</sensor><m003>0.000</m003><m002>0.000</m002><m001>0.000</m001></data><data><sensor>7</sensor><m003>0.000</m003><m002>0.000</m002><m001>0.000</m001></data><data><sensor>8</sensor><m003>0.000</m003><m002>0.000</m002><m001>0.000</m001></data><data><sensor>9</sensor><m003>0.000</m003><m002>0.000</m002><m001>0.000</m001></data></hist></msg>";
        let parse_result = parse_line_from_device(sample_text);
        assert!(parse_result.is_err());

    }

    #[test]
    fn history_line_gets_ignored_again() {
        let sample_text = "<msg><src>CC128-v1.29</src><dsb>02371</dsb><time>23:01:20</time><hist><dsw>02373</dsw><type>1</type><units>kwhr</units><data><sensor>0</sensor><h730>1.799</h730><h728>1.553</h728><h726>2.986</h726><h724>1.125</h724></data><data><sensor>1</sensor><h730>0.000</h730><h728>0.000</h728><h726>0.023</h726><h724>0.000</h724></data><data><sensor>2</sensor><h730>0.000</h730><h728>0.000</h728><h726>0.000</h726><h724>0.000</h724></data><data><sensor>3</sensor><h730>0.000</h730><h728>0.000</h728><h726>0.000</h726><h724>0.000</h724></data><data><sensor>4</sensor><h730>0.000</h730><h728>0.000</h728><h726>0.000</h726><h724>0.000</h724></data><data><sensor>5</sensor><h730>0.000</h730><h728>0.000</h728><h726>0.000</h726><h724>0.000</h724></data><data><sensor>6</sensor><h730>0.000</h730><h728>0.000</h728><h726>0.000</h726><h724>0.000</h724></data><data><sensor>7</sensor><h730>0.000</h730><h728>0.000</h728><h726>0.000</h726><h724>0.000</h724></data><data><sensor>8</sensor><h730>0.000</h730><h728>0.000</h728><h726>0.000</h726><h724>0.000</h724></data><data><sensor>9</sensor><h730>0.000</h730><h728>0.000</h728><h726>0.000</h726><h724>0.000</h724></data></hist></msg>\n<msg>";
        let parse_result = parse_line_from_device(sample_text);
        assert!(parse_result.is_err());

    }
}
