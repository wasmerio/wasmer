use std::fs::File;
use std::path::PathBuf;

use std::io::{BufRead, BufReader};

pub const CFG_TARGET_OS: &'static str = env!("CFG_TARGET_OS");
pub const CFG_TARGET_ARCH: &'static str = env!("CFG_TARGET_OS");

#[derive(Debug, Clone)]
struct IgnorePattern {
    os: Option<String>,
    arch: Option<String>,
    engine: Option<String>,
    compiler: Option<String>,
    pattern_to_ignore: String,
}

impl IgnorePattern {
    fn should_ignore(
        &self,
        os: &str,
        arch: &str,
        engine: &str,
        compiler: &str,
        canonical_path: &str,
    ) -> bool {
        self.os.as_ref().map_or(true, |val| val == os)
            && self.arch.as_ref().map_or(true, |val| val == arch)
            && self.engine.as_ref().map_or(true, |val| val == engine)
            && self.compiler.as_ref().map_or(true, |val| val == compiler)
            && (self.pattern_to_ignore == "*" || canonical_path.contains(&*self.pattern_to_ignore))
    }
}

#[derive(Debug, Clone)]
pub struct Ignores {
    /// The canonical path, and the set of features
    patterns: Vec<IgnorePattern>,
}

impl Ignores {
    /// If the path matches any of the paths on the list
    pub fn should_ignore(
        &self,
        os: &str,
        arch: &str,
        engine: &str,
        compiler: &str,
        canonical_path: &str,
    ) -> bool {
        self.patterns.iter().any(|p| {
            // println!(" -> {:?}", p);
            p.should_ignore(os, arch, engine, compiler, canonical_path)
        })
    }

    pub fn should_ignore_host(&self, engine: &str, compiler: &str, canonical_path: &str) -> bool {
        self.should_ignore(
            CFG_TARGET_OS,
            CFG_TARGET_ARCH,
            engine,
            compiler,
            canonical_path,
        )
    }

    /// Build a Ignore structure from a file path
    pub fn build_from_path(path: PathBuf) -> Ignores {
        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        let mut patterns = Vec::new();

        for (i, line) in reader.lines().enumerate() {
            let line = line.unwrap();
            // If the line has a `#` we discard all the content that comes after
            let line = if line.contains('#') {
                let l: Vec<&str> = line.splitn(2, '#').collect();
                l[0].to_string()
            } else {
                line
            };

            let line = line.trim().to_string();

            // If the lines contains ` ` it means the test should be ignored
            // on the features exposed
            if line.contains(" ") {
                let l: Vec<&str> = line.splitn(2, " ").collect();
                let mut os: Option<String> = None;
                let mut arch: Option<String> = None;
                let mut engine: Option<String> = None;
                let mut compiler: Option<String> = None;
                for alias in l[0].trim().split("+") {
                    match alias {
                        "aarch64" | "x86" | "x64" => {
                            arch = Some(alias.to_string());
                        }
                        "windows" | "macos" | "linux" => {
                            os = Some(alias.to_string());
                        }
                        "jit" | "native" => {
                            engine = Some(alias.to_string());
                        }
                        "cranelift" | "llvm" | "singlepass" => {
                            compiler = Some(alias.to_string());
                        }
                        other => {
                            panic!("Alias {:?} not currently supported (defined in ignores.txt in line {})", other, i+1);
                        }
                    }
                }
                let pattern_to_ignore = l[1].trim().to_string();
                patterns.push(IgnorePattern {
                    os,
                    arch,
                    engine,
                    compiler,
                    pattern_to_ignore,
                });
            } else {
                if line.is_empty() {
                    continue;
                }
                patterns.push(IgnorePattern {
                    os: None,
                    arch: None,
                    engine: None,
                    compiler: None,
                    pattern_to_ignore: line,
                });
            };
        }
        Ignores { patterns }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_match() -> Result<(), ()> {
        assert!(IgnorePattern {
            os: None,
            arch: None,
            engine: None,
            compiler: None,
            pattern_to_ignore: "*".to_string()
        }
        .should_ignore(
            "unknown",
            "unknown",
            "engine",
            "compiler",
            "some::random::text"
        ));
        assert!(IgnorePattern {
            os: None,
            arch: None,
            engine: None,
            compiler: None,
            pattern_to_ignore: "some::random".to_string()
        }
        .should_ignore(
            "unknown",
            "unknown",
            "engine",
            "compiler",
            "some::random::text"
        ));
        assert!(!IgnorePattern {
            os: Some("macos".to_string()),
            arch: None,
            engine: None,
            compiler: None,
            pattern_to_ignore: "other".to_string()
        }
        .should_ignore(
            "unknown",
            "unknown",
            "engine",
            "compiler",
            "some::random::text"
        ));
        assert!(!IgnorePattern {
            os: Some("macos".to_string()),
            arch: None,
            engine: Some("jit".to_string()),
            compiler: None,
            pattern_to_ignore: "other".to_string()
        }
        .should_ignore("macos", "unknown", "jit", "compiler", "some::random::text"));
        Ok(())
    }
}
