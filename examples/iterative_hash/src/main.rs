use blake2::{Blake2b, Digest};
use std::time::{Duration, SystemTime};

fn main() {
    let mut data: Vec<u8> = b"test".to_vec();
    let now = SystemTime::now();

    let mut last_millis: u128 = 0;
    let mut round_count: usize = 0;
    let mut record_count: usize = 0;

    for i in 0.. {
        let mut hasher = Blake2b::new();
        hasher.input(&data);
        let out = hasher.result();
        data = out.to_vec();

        if i != 0 && i % 1000 == 0 {
            let millis = now.elapsed().unwrap().as_millis();
            let diff = millis - last_millis;
            if diff >= 100 {
                record_count += 1;
                println!("{}", ((i - round_count) as u128) * 1000000 / diff );
                last_millis = millis;
                round_count = i;
            }
        }
    }
}
