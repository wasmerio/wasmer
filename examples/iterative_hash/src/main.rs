use blake2::{Blake2b, Digest};
use std::time::{Duration, SystemTime};

fn main() {
    let mut data: Vec<u8> = b"test".to_vec();
    let now = SystemTime::now();

    let mut last_millis: u128 = 0;
    let mut round_count: usize = 0;

    for i in 0.. {
        let mut hasher = Blake2b::new();
        hasher.input(&data);
        let out = hasher.result();
        data = out.to_vec();

        if i != 0 && i % 100000 == 0 {
            let millis = now.elapsed().unwrap().as_millis();
            println!("{} rounds in last second", (i - round_count) as f64 / (millis - last_millis) as f64);
            last_millis = millis;
            round_count = i;
        }
    }
}
