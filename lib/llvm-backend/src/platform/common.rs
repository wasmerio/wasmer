pub fn round_up_to_page_size(size: usize) -> usize {
    (size + (4096 - 1)) & !(4096 - 1)
}
