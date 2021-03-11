fn main() {
    #[cfg(feature = "tracing")]
    {
        sonde::Builder::new().file("tracing.d").compile();
    }
}
