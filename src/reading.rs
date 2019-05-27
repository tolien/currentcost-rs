use chrono::Utc;

#[derive(Debug)]
pub struct CurrentCostReading {
    pub timestamp: chrono::DateTime<Utc>,
    pub device: String,
    pub sensor: i32,
    pub temperature: f32,
    pub power: i32,
}

impl CurrentCostReading {

}
