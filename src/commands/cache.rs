use crate::common::get_cache_dir;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub enum Cache {
    /// Clear the cache
    #[structopt(name = "clean")]
    Clean,

    /// Display the location of the cache
    #[structopt(name = "dir")]
    Dir,
}

impl Cache {
    pub fn execute(&self) {
        match &self {
            Cache::Clean => {
                use std::fs;
                let cache_dir = get_cache_dir();
                if cache_dir.exists() {
                    fs::remove_dir_all(cache_dir.clone()).expect("Can't remove cache dir");
                }
                fs::create_dir_all(cache_dir).expect("Can't create cache dir");
            }
            Cache::Dir => {
                println!("{}", get_cache_dir().to_string_lossy());
            }
        }
    }
}
