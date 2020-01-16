extern crate chrono;
use chrono::offset::TimeZone;
use chrono::prelude::*;
use chrono::DateTime;
use chrono::Utc;

#[macro_use]
extern crate log;

extern crate fern;

use fern::colors::{Color, ColoredLevelConfig};

use std::env;
use std::error::Error;
use std::fs;
use std::process;

use currentcost::get_db_connection;
use currentcost::Config;
use currentcost::CurrentcostLine;

fn main() {
    let logger_result = setup_logger();
    if logger_result.is_err() {
        panic!("Error applying fern logger");
    }

    let args: Vec<String> = env::args().collect();
    let config = Config::new(&args).unwrap_or_else(|err| {
        error!("Problem parsing arguments: {}", err);
        process::exit(1);
    });

    if let Err(e) = run(&config) {
        error!("Application error: {}", e);

        process::exit(1);
    }
}

fn setup_logger() -> Result<(), fern::InitError> {
    let colors_line = ColoredLevelConfig::new()
        .error(Color::Red)
        .warn(Color::Yellow)
        // we actually don't need to specify the color for debug and info, they are white by default
        .info(Color::White)
        .debug(Color::White)
        // depending on the terminals color scheme, this is the same as the background color
        .trace(Color::BrightBlack);

    let colors_level = colors_line.info(Color::Green);
    fern::Dispatch::new()
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
        .level_for("tokio_postgres", log::LevelFilter::Off)
        .chain(std::io::stdout())
        .apply()?;
    Ok(())
}
fn format_unixtime(timestamp: i32) -> DateTime<Utc> {
    let naive_datetime = NaiveDateTime::from_timestamp(i64::from(timestamp), 0);
    let datetime: DateTime<Utc> = DateTime::from_utc(naive_datetime, Utc);

    datetime
}

fn run(config: &Config) -> Result<(), Box<dyn Error>> {
    let last_entry = if config.database.use_database() {
        let mut db = get_db_connection(&config);

        get_latest_timestamp_in_db(&mut db)
    } else {
        0
    };

    info!("Inserting data since {}", format_unixtime(last_entry));
    let filtered_lines = parse_and_filter_log(&config.filename, last_entry)?;
    info!("Lines to insert: {}", filtered_lines.len());

    if config.database.use_database() {
        let mut db = get_db_connection(&config);
        insert_lines(&mut db, filtered_lines)?;
    }

    Ok(())
}

pub fn get_latest_timestamp_in_db(db_connection: &mut postgres::Client) -> i32 {
    #![allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let query = "SELECT EXTRACT(epoch FROM max(datetime)) AS max FROM entries";

    let mut max_timestamp = 0;
    for row in db_connection.query(query, &[]).unwrap() {
        assert!(!row.is_empty());
        let float_value: f64 = row.get("max");
        assert!(float_value >= 0.0 && float_value < (f64::from(i32::max_value())));
        max_timestamp = float_value as i32;
    }

    max_timestamp
}

pub fn parse_and_filter_log(
    filename: &str,
    skip_before_timestamp: i32,
) -> Result<Vec<CurrentcostLine>, Box<dyn Error>> {
    let contents = fs::read_to_string(filename)?;
    let to_parse = contents.lines().collect();

    let mut parsed = parse_all_lines(to_parse);
    if skip_before_timestamp > 0 {
        parsed.sort();
        parsed = filter_by_timestamp(parsed, skip_before_timestamp);
    }

    Ok(parsed)
}

fn parse_all_lines(lines: Vec<&str>) -> Vec<CurrentcostLine> {
    let mut parsed_lines = Vec::new();
    for line in lines {
        if let Ok(parsed_line) = parse_line(line) {
            parsed_lines.push(parsed_line);
        } else {
            error!("Skipping invalid line: {}", line);
        }
    }

    parsed_lines
}

fn insert_lines(
    db_client: &mut postgres::Client,
    lines: Vec<CurrentcostLine>,
) -> Result<(), Box<dyn Error>> {
    let mut transaction = db_client.transaction()?;
    let query = "INSERT INTO entries (sensor, datetime, power) VALUES ($1, $2, $3)";
    let prep_statement = transaction.prepare(&query)?;
    for line in lines {
        let unixtime = Utc.timestamp(i64::from(line.timestamp), 0);
        transaction.execute(&prep_statement, &[&line.sensor, &unixtime, &line.power])?;
    }

    transaction.commit()?;
    Ok(())
}

fn parse_line(line: &str) -> Result<CurrentcostLine, &'static str> {
    let mut position = 0;
    let mut timestamp = 0;
    let mut power = 0;
    let mut sensor = 0;

    for item in line.split(',') {
        if position == 1 {
            let timestamp_string = item.trim();
            if let Ok(time) = timestamp_string.parse::<i32>() {
                timestamp = time
            } else {
                return Err("Invalid timestamp");
            };
        } else if position == 2 {
            let sensor_string = item;
            let start_section = "Sensor ";
            if let Ok(sns) = sensor_string
                .split_at(start_section.len())
                .1
                .trim()
                .parse::<i32>()
            {
                sensor = sns;
            } else {
                return Err("Invalid sensor");
            };
        } else if position == 4 {
            let power_string = item;
            if let Ok(pwr) = power_string
                .split_at(power_string.len() - 1)
                .0
                .trim()
                .parse::<i32>()
            {
                power = pwr
            } else {
                return Err("Invalid power");
            };
        }
        position += 1;
    }

    if position == 5 {
        Ok(CurrentcostLine {
            timestamp,
            sensor,
            power,
        })
    } else {
        Err("Failed to parse line - not enough pieces")
    }
}

fn filter_by_timestamp(lines: Vec<CurrentcostLine>, timestamp: i32) -> Vec<CurrentcostLine> {
    let mut new_list = Vec::new();
    let mut last_timestamp = timestamp;

    for line in lines.into_iter().rev() {
        if line.timestamp > timestamp {
            if line.timestamp != last_timestamp {
                last_timestamp = line.timestamp;
                new_list.push(line);
            }
        } else {
            break;
        }
    }
    new_list
}

#[cfg(test)]
mod tests {
    use super::filter_by_timestamp;
    use super::parse_all_lines;
    use super::parse_line;

    #[test]
    fn line_gets_parsed() {
        let sample_text = "13/04/2019 20:44:48, 1555188288, Sensor 0, 21.200000°C, 631W";
        let parsed_result = parse_line(sample_text);
        let parsed = parsed_result.unwrap();

        assert_eq!(1555188288, parsed.timestamp);
        assert_eq!(0, parsed.sensor);
        assert_eq!(631, parsed.power);
    }

    #[test]
    fn multilines_get_parsed() {
        let sample_text = "14/04/2019 23:25:26, 1555284326, Sensor 1, 22.100000°C, 0W
        14/04/2019 23:25:29, 1555284329, Sensor 0, 22.100000°C, 544W
        14/04/2019 23:25:32, 1555284332, Sensor 1, 22.100000°C, 0W";
        let parsed = parse_all_lines(sample_text.lines().collect());

        assert_eq!(3, parsed.len());
        assert_eq!(1555284326, parsed[0].timestamp);
        assert_eq!(1555284329, parsed[1].timestamp);
        assert_eq!(1555284332, parsed[2].timestamp);
    }

    #[test]
    fn empty_string_get_parsed() {
        let sample_text = "";
        let parsed = parse_all_lines(sample_text.lines().collect());

        assert_eq!(0, parsed.len());
    }

    #[test]
    fn invalid_line_gets_dropped() {
        let sample_text = "14/04/2019 23:25:26, Sensor 1, 22.100000°C, 0W
        14/04/2019 23:25:29, 1555284329, Sensor 0, 22.100000°C, 544W
        14/04/2019 23:25:32, 1555284332, Sensor, 22.100000°C, 0W";

        let parsed = parse_all_lines(sample_text.lines().collect());

        assert_eq!(1, parsed.len());
    }

    #[test]
    fn lines_get_skipped_if_before_last_run() {
        let sample_text = "14/04/2019 23:25:26, 1555284326, Sensor 1, 22.100000°C, 0W
        14/04/2019 23:25:29, 1555284329, Sensor 0, 22.100000°C, 544W
        14/04/2019 23:25:32, 1555284332, Sensor 1, 22.100000°C, 0W";
        let parsed = parse_all_lines(sample_text.lines().collect());
        let filtered = filter_by_timestamp(parsed, 1555284331);

        assert_eq!(1, filtered.len());
        assert_eq!(1555284332, filtered[0].timestamp);
    }

    #[test]
    fn lines_with_the_same_timestamp_get_skipped() {
        let sample_text = "11/08/2019 21:04:03, 1565557443, Sensor 0, 25.200000°C, 2637W
        11/08/2019 21:04:03, 1565557443, Sensor 0, 25.200000°C, 2637W
        11/08/2019 21:04:03, 1565557443, Sensor 1, 25.200000°C, 2637W";

        let parsed = parse_all_lines(sample_text.lines().collect());
        let filtered = filter_by_timestamp(parsed, 0);

        assert_eq!(1, filtered.len());
        assert_eq!(1565557443, filtered[0].timestamp);
    }
}
