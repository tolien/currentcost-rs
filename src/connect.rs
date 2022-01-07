extern crate roxmltree;
use roxmltree::{Document, Node};
extern crate signal_hook;

#[macro_use]
extern crate log;
extern crate fern;

use fern::colors::{Color, ColoredLevelConfig};

use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::BufWriter;
use std::io::Error;
use std::io::Write;
use std::path::Path;
use std::process;
use std::str;
use std::time::Duration;
use toml::Value;

mod reading;
pub use crate::reading::CurrentCostReading;

fn main() {
    let config = parse_config();
    let logger_result = setup_logger(&config);
    assert!(logger_result.is_ok(), "Error applying fern logger: {:?}", logger_result.err());

    let signal_handler_result = setup_signal_handler();
    if signal_handler_result.is_err() {
        error!("Error applying signal handler, won't log SIGINT/SIGTERM");
    };

    let port = get_serial_port(&config).unwrap_or_else(|err| {
        error!("Error opening serial port: {}", err);
        process::exit(1);
    });

    listen_on_port(port, &config);
}

fn setup_logger(config: &ConnectConfig) -> std::result::Result<(), fern::InitError> {
    let colors_line = ColoredLevelConfig::new()
        .error(Color::Red)
        .warn(Color::Yellow)
        // we actually don't need to specify the color for debug and info, they are white by default
        .info(Color::White)
        .debug(Color::White)
        // depending on the terminals color scheme, this is the same as the background color
        .trace(Color::BrightBlack);

    let colors_level = colors_line.info(Color::Green);
    let base_config = fern::Dispatch::new();
    let stdout_config = fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{color_line}[{date}][{level}{color_line}] {message}\x1B[0m",
                color_line = format_args!(
                    "\x1B[{}m",
                    colors_line.get_color(&record.level()).to_fg_str()
                ),
                date = chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                level = colors_level.color(record.level()),
                message = message,
            ));
        })
        // Add blanket level filter -
        .level(log::LevelFilter::Debug)
        .level_for("tokio_reactor", log::LevelFilter::Off)
        .chain(std::io::stdout());

    let file_config = fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{color_line}[{date}][{level}{color_line}] {message}\x1B[0    m",
                color_line = format_args!(
                    "\x1B[{}m",
                    colors_line.get_color(&record.level()).to_fg_str()
                ),
                date = chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                level = colors_level.color(record.level()),
                message = message,
            ));
        })
        // Add blanket level filter -
        .level(log::LevelFilter::Info)
        .level_for("tokio_reactor", log::LevelFilter::Off)
        .chain(fern::log_file(&config.debug_log_path)?);

    base_config
        .chain(file_config)
        .chain(stdout_config)
        .apply()?;

    Ok(())
}

fn setup_signal_handler() -> std::result::Result<(), Error> {
    unsafe {
        signal_hook::low_level::register(signal_hook::consts::SIGTERM, || {
            info!("Terminated by SIGTERM");
            process::exit(0);
        })
    }?;

    unsafe {
        signal_hook::low_level::register(signal_hook::consts::SIGINT, || {
            info!("Terminated by SIGINT");
            process::exit(0);
        })
    }?;

    Ok(())
}

fn listen_on_port(mut port: Box<dyn serialport::SerialPort>, config: &ConnectConfig) {
    info!("Port name: {}", port.name().unwrap());

    let mut serial_buf: Vec<u8> = vec![0; 1000];
    let mut line: String = String::new();
    info!(
        "Receiving data on {} at {} baud",
        port.name().unwrap(),
        port.baud_rate().unwrap()
    );
    let mut file_buffer = get_file_buffer(config);
    loop {
        match port.read(serial_buf.as_mut_slice()) {
            Ok(t) => {
                let s = received_bytes_to_string(&serial_buf[..t]);
                line.push_str(s);
                if s.contains('\n') {
                    if let Ok(reading) = parse_line_from_device(&line) {
                        debug!("{:?}", reading);
                        write_to_log(&reading.to_log(), &mut file_buffer);
                    }
                    line = String::new();
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::TimedOut => (),
            Err(e) => error!("{:?}", e),
        }
    }
}

fn get_file_buffer(config: &ConnectConfig) -> BufWriter<File> {
    BufWriter::new(
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(&config.data_log_path)
            .unwrap(),
    )
}

fn received_bytes_to_string(bytes: &[u8]) -> &str {
    str::from_utf8(bytes).unwrap_or_else(|err| {
        error!(
            "Error: {} in string {:?}",
            err,
            String::from_utf8_lossy(bytes)
        );
        ""
    })
}

fn get_serial_port(config: &ConnectConfig) -> Result<Box<dyn serialport::SerialPort>, String> {
    let builder = serialport::new(&config.port, config.bit_rate)
        .timeout(Duration::new(config.timeout.into(), 0))
        .baud_rate(config.bit_rate);

    match builder.open() {
        Ok(port) => Ok(port),
        Err(error_description) => Err(format!(
            "Problem opening serial port: {}",
            error_description
        )),
    }
}

fn write_to_log(line: &str, writer: &mut BufWriter<File>) {
    let write_result = writer.write_all(line.as_bytes());
    assert!(write_result.is_ok(), "Failed to write to file");
    let flush_result = writer.flush();
    assert!(flush_result.is_ok(), "Failed to flush writes");
}

#[derive(Debug)]
struct ConnectConfig {
    port: String,
    bit_rate: u32,
    timeout: u32,
    data_log_path: String,
    debug_log_path: String,
}

impl ConnectConfig {
    pub fn new(args: &toml::Value) -> std::result::Result<Self, &'static str> {
        #![allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let serial_args = &args["serial"];
        let port = String::from(serial_args["port"].as_str().unwrap());
        let bit_rate_int = serial_args["bit_rate"].as_integer().unwrap();
        assert!(bit_rate_int > 0 && bit_rate_int < (i64::from(u32::MAX)));
        let bit_rate = bit_rate_int as u32;
        let timeout = serial_args["timeout"].as_integer().unwrap() as u32;

        let logging_args = &args["logging"];
        let data_log_dir = logging_args["data_log_output_dir"].as_str().unwrap();
        let data_log = logging_args["data_log"].as_str().unwrap();
        let data_log_path = String::from(
            Path::join(Path::new(data_log_dir), data_log)
                .to_str()
                .unwrap(),
        );

        let debug_log_dir = args["logging"]["connect_debug_log_location"]
            .as_str()
            .unwrap();
        let debug_log = args["logging"]["connect_debug_log"].as_str().unwrap();
        let debug_log_path = String::from(
            Path::join(Path::new(debug_log_dir), debug_log)
                .to_str()
                .unwrap(),
        );

        Ok(Self {
            port,
            bit_rate,
            timeout,
            data_log_path,
            debug_log_path,
        })
    }
}

fn parse_config() -> ConnectConfig {
    let properties = fs::read_to_string("config.toml").unwrap();
    let values = &properties.parse::<Value>().unwrap();

    ConnectConfig::new(values).unwrap()
}

fn get_element_from_xmldoc(root: &Document, element_name: &str, expected_count: usize) -> String {
    let nodes: Vec<Node> = root
        .descendants()
        .filter(|n| n.has_tag_name(element_name))
        .collect();
    if nodes.len() != expected_count {
        return String::new();
    }
    assert_eq!(nodes.len(), expected_count);
    let value = nodes[0].text().unwrap();

    String::from(value)
}

fn parse_line_from_device(line: &str) -> std::result::Result<CurrentCostReading, &'static str> {
    if let Ok(parse_state) = Document::parse(line) {
        let doc = parse_state;

        let source = get_element_from_xmldoc(&doc, "src", 1);
        if source.is_empty() {
            return Err("No device found in data");
        }

        let pwr = get_element_from_xmldoc(&doc, "watts", 1);
        let power;
        if pwr.is_empty() {
            return Err("No power value found in data");
        } else if let Ok(parsed_power) = pwr.parse::<i32>() {
            power = parsed_power;
        } else {
            return Err("Invalid power value - couldn't parse an an integer");
        }

        let temp = get_element_from_xmldoc(&doc, "tmpr", 1);
        let temperature;
        if temp.is_empty() {
            return Err("No temperature value found in data");
        } else if let Ok(parsed_temp) = temp.parse::<f32>() {
            temperature = parsed_temp;
        } else {
            return Err("Invalid temperature value - couldn't parse a float");
        }

        let sens = get_element_from_xmldoc(&doc, "sensor", 1);
        if sens.is_empty() {
            return Err("No sensor value found in data");
        }

        let sensor = if let Ok(sensor_value) = sens.parse::<i32>() {
            sensor_value
        } else {
            return Err("Invalid sensor ID - couldn't parse as an integer");
        };

        let reading = CurrentCostReading {
            timestamp: chrono::Utc::now(),
            device: source,
            sensor,
            temperature,
            power,
        };

        Ok(reading)
    } else {
        Err("Error parsing XML")
    }
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
    fn invalid_lines_return_errors() {
        let mut sample_text = "<msg><src>CC128-v1.29</src><dsb>02353</dsb><time>10:27:59</time><tmpr>21.4</tmpr><sensor>0</sensor><id>04066</id><type>1</type><ch1><watts>p</watts></ch1></msg>";
        let parse_result = parse_line_from_device(sample_text);

        assert!(parse_result.is_err());

        sample_text = "<msg><src>CC128-v1.29</src><dsb>02353</dsb><time>10:27:59</time><tmpr>2a.4</tmpr><sensor>0</sensor><id>04066</id><type>1</type><ch1><watts>00479</watts></ch1></msg>";
        let parse_result = parse_line_from_device(sample_text);
        assert!(parse_result.is_err());

        sample_text = "<msg><src>CC128-v1.29</src><dsb>02353</dsb><time>10:27:59</time><tmpr>20.4</tmpr><sensor>p</sensor><id>04066</id><type>1</type><ch1><watts>00479</watts></ch1></msg>";
        let parse_result = parse_line_from_device(sample_text);
        assert!(parse_result.is_err());
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
