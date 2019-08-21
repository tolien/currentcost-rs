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
    pub fn to_log(&self) -> String {
        format!(
            "{}, {}, Sensor {}, {:.2}°C, {}W\n",
            self.timestamp.format("%d/%m/%Y %H:%M:%S"),
            self.timestamp.timestamp(),
            self.sensor,
            self.temperature,
            self.power
        )
    }
}

#[cfg(test)]
mod tests {

    use crate::reading::CurrentCostReading;
    use chrono::prelude::*;

    #[test]
    fn convert_reading_to_log_line() {
        let reading = CurrentCostReading {
            timestamp: Utc.ymd(2019, 8, 20).and_hms(15, 40, 42),
            device: String::from("CC128-v1.29"),
            sensor: 0,
            temperature: 24.8,
            power: 3000,
        };

        let log_line = "20/08/2019 15:40:42, 1566315642, Sensor 0, 24.80°C, 3000W\n";
        assert_eq!(reading.to_log(), log_line);
    }
}
