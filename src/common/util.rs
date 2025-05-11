// Stuff I don't know where to put.

pub fn format_bytes(bytes: u64) -> String {
    let mut total_allocation_str;
    if bytes >= 2u64.pow(30) {
        total_allocation_str = format!("{:.3}GiB", bytes as f32 / 2f32.powf(30.0));
    } else if bytes >= 2u64.pow(20) {
        total_allocation_str = format!("{:.3}MiB", bytes as f32 / 2f32.powf(20.0));
    } else if bytes >= 2u64.pow(10) {
        total_allocation_str = format!("{:.3}KiB", bytes as f32 / 2f32.powf(10.0));
    } else {
        total_allocation_str = format!("{:.3}B", bytes);
    }
    return total_allocation_str;
}
