//!
//! ID Generator
//!
//!

pub struct IDGenerator;

impl IDGenerator {
    pub fn gen_order_id(_: u64) -> String {
        format!(
            "ON{}",
            uuid::Uuid::new_v4()
                .to_string()
                .to_uppercase()
                .replace("-", ""),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::IDGenerator;

    #[test]
    fn test_gen_order_id() {
        let account_id = 123456789;
        let order_id = IDGenerator::gen_order_id(account_id);
        println!("Generated Order ID: {}", order_id);
        assert!(order_id.starts_with("O"));
    }
}
