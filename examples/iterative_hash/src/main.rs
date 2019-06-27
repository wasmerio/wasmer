use blake2::{Blake2b, Digest};

fn main() {
    let mut data: Vec<u8> = b"test".to_vec();

    for i in 0.. {
        let mut hasher = Blake2b::new();
        hasher.input(&data);
        let out = hasher.result();
        data = out.to_vec();

        if i % 1000000 == 0 {
            println!("Round {}: {:?}", i, data);
        }
    }
}
