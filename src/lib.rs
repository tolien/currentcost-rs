use postgres::{Client, NoTls};
use std::cmp::Ordering;
use std::fs;
use std::path::Path;
use std::process;

use toml::Value;

pub struct Config {
    pub filename: String,
    pub database: DatabaseConfig,
}

impl Config {
    pub fn new(args: &[String]) -> Result<Self, &'static str> {
        if args.len() < 2 {
            return Err("not enough arguments");
        }
        let filename = args[1].clone();
        let working_dir = get_path_to_bin_location(args);
        let properties = fs::read_to_string(working_dir.join("config.toml"))
            .unwrap_or_else(|_err| fs::read_to_string("config.toml").unwrap());
        let values = &properties.parse::<Value>().unwrap();
        let database_config = DatabaseConfig::new(&values["database"]).unwrap();

        Ok(Self {
            filename,
            database: database_config,
        })
    }
}

pub struct DatabaseConfig {
    ignore_db: bool,
    database_name: String,
    host: String,
    user: String,
}

impl DatabaseConfig {
    pub fn new(args: &toml::Value) -> Result<Self, &'static str> {
        Ok(Self {
            ignore_db: args["ignore_db"].as_bool().unwrap(),
            database_name: String::from(args["db_name"].as_str().unwrap()),
            host: String::from(args["hostname"].as_str().unwrap()),
            user: String::from(args["user"].as_str().unwrap()),
        })
    }

    pub fn use_database(&self) -> bool {
        !self.ignore_db
    }
}

pub fn get_db_connection(config: &Config) -> postgres::Client {
    Client::configure()
        .user(&config.database.user)
        .host(&config.database.host)
        .dbname(&config.database.database_name)
        .connect(NoTls)
        .unwrap_or_else(|err| {
            println!("Failed to connect to DB: {}", err);
            process::exit(1);
        })
}

#[derive(PartialOrd)]
pub struct CurrentcostLine {
    pub timestamp: i32,
    pub sensor: i32,
    pub power: i32,
}
impl Ord for CurrentcostLine {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timestamp.cmp(&other.timestamp)
    }
}
impl Eq for CurrentcostLine {}
impl PartialEq for CurrentcostLine {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp
            && self.sensor == other.sensor
            && self.power == other.power
    }
}

fn get_path_to_bin_location(args: &[String]) -> &Path {
    assert!(!args.is_empty());
    let path = Path::new(&args[0]);

    path.parent().unwrap_or_else(|| Path::new("."))
}
