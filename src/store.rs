extern crate chrono;
use chrono::offset::TimeZone;
use chrono::prelude::*;
use chrono::DateTime;
use chrono::Utc;

#[macro_use]
extern crate log;
extern crate simplelog;

use simplelog::*;

use std::env;
use std::error::Error;
use std::fs;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use currentcost::get_db_connection;
use currentcost::Config;
use currentcost::CurrentcostLine;

fn main() {
    setup_logger();
    let args: Vec<String> = env::args().collect();
    let config = Config::new(&args).unwrap_or_else(|err| {
        error!("Problem parsing arguments: {}", err);
        process::exit(1);
    });

    if let Err(e) = run(config) {
        error!("Application error: {}", e);

        process::exit(1);
    }
}

fn setup_logger() {
    let term_logger = TermLogger::new(LevelFilter::Info, simplelog::Config::default());
    if term_logger.is_none() {
        CombinedLogger::init(vec![SimpleLogger::new(
            LevelFilter::Info,
            simplelog::Config::default(),
        )])
        .unwrap();
    } else {
        CombinedLogger::init(vec![term_logger.unwrap()]).unwrap();
    }
}
fn format_unixtime(timestamp: i32) -> DateTime<Utc> {
    let naive_datetime = NaiveDateTime::from_timestamp(i64::from(timestamp), 0);
    let datetime: DateTime<Utc> = DateTime::from_utc(naive_datetime, Utc);

    datetime
}

fn run(config: Config) -> Result<(), Box<dyn Error>> {
  let start = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
  
  let mut last_entry = 0;
  if config.database.use_database() {
    let mut db = get_db_connection(&config);
    
    last_entry = get_latest_timestamp_in_db(&mut db);
  }

    info!("Inserting data since {}", format_unixtime(last_entry));
    let filtered_lines = parse_and_filter_log(&config.filename, last_entry)?;
    info!("Lines to insert: {}", filtered_lines.len());
    
    if config.database.use_database() {
      let mut db = get_db_connection(&config);
      insert_lines(&mut db, filtered_lines)?;
    }

    let end = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    println!("run: {:?}", (end - start));

    Ok(())
}

pub fn get_latest_timestamp_in_db(db_connection: &mut postgres::Client) -> i32 {
    let query = "SELECT EXTRACT(epoch FROM max(datetime)) AS max FROM entries";

    let mut max_timestamp = 0;
    for row in db_connection.query(query, &[]).unwrap() {
        assert!(!row.is_empty());
        let float_value: f64 = row.get("max");
        max_timestamp = float_value as i32;
    }

    max_timestamp
}

pub fn parse_and_filter_log(
    filename: &str,
    skip_before_timestamp: i32,
) -> Result<Vec<CurrentcostLine>, Box<dyn Error>> {
    let start = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let contents = fs::read_to_string(filename)?;
    let read_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    println!("read_log: {:?}", (read_time - start));
    let mut parsed = parse_all_lines(contents.lines().collect());
    if skip_before_timestamp > 0 {
        parsed.sort();
        parsed = filter_by_timestamp(parsed, skip_before_timestamp);
    }

    let end = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    println!("parse_and_filter_log: {:?}", (end - start));

    Ok(parsed)
}

fn parse_all_lines(lines: Vec<&str>) -> Vec<CurrentcostLine> {
    let start = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let mut parsed_lines = Vec::new();
    for line in lines {
        let parsed_line_result = parse_line(line);
        if parsed_line_result.is_ok() {
            assert!(parsed_line_result.is_ok());
            let parsed_line = parsed_line_result.unwrap();
            parsed_lines.push(parsed_line);
        } else {
            error!("Skipping invalid line: {}", line);
        }
    }

    let end = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    println!("parse_all_lines: {:?}", (end - start));
    
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
    let split_line: Vec<&str> = line.split(',').collect();
    if split_line.len() == 5 {
        let timestamp_string = split_line[1].trim();
        let sensor_string = split_line[2];
        let power_string = split_line[4];

        let time = timestamp_string.parse::<i32>();
        let timestamp = if time.is_ok() {
            time.unwrap()
        } else {
            return Err("Invalid timestamp");
        };

        let sns = strip_non_numeric(sensor_string).parse::<i32>();
        let sensor = if sns.is_ok() {
            sns.unwrap()
        } else {
            return Err("Invalid sensor");
        };

        let pwr = strip_non_numeric(power_string).parse::<i32>();
        let power = if pwr.is_ok() {
            pwr.unwrap()
        } else {
            return Err("Invalid power");
        };

        Ok(CurrentcostLine {
            timestamp,
            sensor,
            power,
        })
    } else {
        Err("Failed to parse line - not enough pieces")
    }
}

fn strip_non_numeric(input: &str) -> String {
    let number_groups: Vec<&str> = input.matches(char::is_numeric).collect();

    number_groups.join("")
}

fn filter_by_timestamp(lines: Vec<CurrentcostLine>, timestamp: i32) -> Vec<CurrentcostLine> {
    let start = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let mut new_list = Vec::new();

    for line in lines.into_iter().rev() {
        if line.timestamp > timestamp {
            new_list.push(line);
        }
        else {
            break
        }
    }

    let end = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    println!("filter_by_timestamp: {:?}", (end - start));
    new_list
}

#[cfg(test)]
mod tests {
    use super::filter_by_timestamp;
    use super::parse_all_lines;
    use super::parse_line;
    use super::strip_non_numeric;

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

    struct SampleResult<'a> {
        sample: &'a str,
        expected_result: &'a str,
    }

    #[test]
    fn numbers_extracted_from_sensor_and_power() {
        let mut samples = Vec::new();
        samples.push(SampleResult {
            sample: "0W",
            expected_result: "0",
        });
        samples.push(SampleResult {
            sample: "Sensor",
            expected_result: "",
        });
        samples.push(SampleResult {
            sample: "544W",
            expected_result: "544",
        });
        samples.push(SampleResult {
            sample: "",
            expected_result: "",
        });

        for sample in samples {
            let result = strip_non_numeric(sample.sample);
            assert_eq!(sample.expected_result, result);
        }
    }
}
