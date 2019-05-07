use chrono::offset::TimeZone;
use chrono::Utc;
use std::env;
use std::error::Error;
use std::fs;
use std::process;

use currentcost::Config;
use currentcost::CurrentcostLine;
use currentcost::get_db_connection;

fn main() {
    let args: Vec<String> = env::args().collect();
    let config = Config::new(&args).unwrap_or_else(|err| {
        println!("Problem parsing arguments: {}", err);
        process::exit(1);
    });

    if let Err(e) = run(config) {
        println!("Application error: {}", e);

        process::exit(1);
    }
}

fn run(config: Config) -> Result<(), Box<dyn Error>> {
    let mut db = get_db_connection(&config);
    let last_entry = get_latest_timestamp_in_db(&mut db);
    let filtered_lines = parse_and_filter_log(&config.filename, last_entry)?;
    //println!("Lines: {}", filtered_lines.len());
    insert_lines(&mut db, filtered_lines)?;

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

    let contents = fs::read_to_string(filename)?;
    let mut parsed = parse_all_lines(contents.lines().collect());
    if skip_before_timestamp > 0 {
        parsed = filter_by_timestamp(parsed, skip_before_timestamp);
    }
    
    Ok(parsed)
}

fn parse_all_lines(lines: Vec<&str>) -> Vec<CurrentcostLine> {
    let mut parsed_lines = Vec::new();
    for line in lines {
        parsed_lines.push(parse_line(line));
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

fn parse_line(line: &str) -> CurrentcostLine {
    let split_line: Vec<&str> = line.split(',').collect();

    let timestamp_string = split_line[1].trim();
    let sensor_string = split_line[2];
    let power_string = split_line[4];

    let timestamp = timestamp_string.parse::<i32>().unwrap();
    let sensor = strip_non_numeric(sensor_string).parse::<i32>().unwrap();
    let power = strip_non_numeric(power_string).parse::<i32>().unwrap();

    CurrentcostLine {
        timestamp,
        sensor,
        power,
    }
}

fn strip_non_numeric(input: &str) -> String {
    let number_groups: Vec<&str> = input.matches(char::is_numeric).collect();

    number_groups.join("")
}

fn filter_by_timestamp(lines: Vec<CurrentcostLine>, timestamp: i32) -> Vec<CurrentcostLine> {
    let mut new_list = Vec::new();

    for line in lines {
        if line.timestamp > timestamp {
            new_list.push(line);
        }
    }

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
        let parsed = parse_line(sample_text);

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
        samples.push(SampleResult { sample: "0W", expected_result: "0"} );
        samples.push(SampleResult { sample: "Sensor", expected_result: ""});
        samples.push(SampleResult { sample: "544W", expected_result: "544"});
        samples.push(SampleResult { sample: "", expected_result: ""});
        
        for sample in samples {
            let result = strip_non_numeric(sample.sample);
            assert_eq!(sample.expected_result, result);
        }
    }
}

