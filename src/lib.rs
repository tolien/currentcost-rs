use postgres::{Client, NoTls};
use std::fs;
use std::process;

use toml::Value;

pub struct Config {
    pub filename: String,
    pub database: DatabaseConfig,
}

impl Config {
    pub fn new(args: &[String]) -> Result<Config, &'static str> {
        if args.len() < 2 {
            return Err("not enough arguments");
        }
        let filename = args[1].clone();
        let properties = fs::read_to_string("config.toml").unwrap();
        let values = &properties.parse::<Value>().unwrap();
        let database_config = DatabaseConfig::new(&values["database"]).unwrap();

        Ok(Config { filename: filename, database: database_config 
        })
    }
}

pub struct DatabaseConfig {
    database_name: String,
    host: String,
    user: String,
}

impl DatabaseConfig {
    pub fn new(args: &toml::Value) -> Result<DatabaseConfig, &'static str> {
        Ok(DatabaseConfig { 
            database_name: String::from(args["db_name"].as_str().unwrap()),
            host: String::from(args["hostname"].as_str().unwrap()),
            user: String::from(args["user"].as_str().unwrap()), 
        })
    }
}

pub fn get_db_connection(config: &Config) -> postgres::Client {
    let db_connection = Client::configure()
        .user(&config.database.user)
        .host(&config.database.host)
        .dbname(&config.database.database_name)
        .connect(NoTls)
        .unwrap_or_else(|err| {
            println!("Failed to connect to DB: {}", err);
            process::exit(1);
        });
    db_connection
}

pub struct CurrentcostLine {
    pub timestamp: i32,
    pub sensor: i32,
    pub power: i32,
}




