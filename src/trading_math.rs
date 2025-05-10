use std::{thread, time::Duration};

pub fn increased_amount_by_percentage(value: f64, x: f64) -> f64 {
    let increase = value * (x / 100.0);
    let increased_value = value + increase;
    increased_value
}

pub fn decreased_amount_by_percentage(value: f64, x: f64) -> f64 {
    let decrease = value * (x / 100.0);
    let decreased_value = value - decrease;
    decreased_value
}

pub fn countdown() {
    for i in (0..=10).rev() {
        println!("{}", i);
        thread::sleep(Duration::from_secs(1)); // Pause for 1 second
    }
}