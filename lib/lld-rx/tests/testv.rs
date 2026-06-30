#[cfg(test)]
mod tests {
    use lld_rx::link_native;

    #[test]
    fn test_via_version() {
        let res = link_native(vec!["--version".to_string()]);
        res.debug_print();
    }
}
