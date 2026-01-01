use chrono::Utc;

// 微秒级时间戳
pub fn now_us() -> u64 {
    let now = Utc::now();
    let sec = now.timestamp(); // 秒
    let micros = now.timestamp_subsec_micros(); // 微秒部分

    (sec as u64) * 1_000_000 + (micros as u64)
}
